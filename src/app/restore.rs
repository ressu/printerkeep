//! `restore`: put files back from a backup. From this printer's history (for example, to undo an edit),
//! from another printer or repository, or from the repository after a new install.

use super::connection::{Ask, AskToken, TokenRequest, list, log_in, printer_branches};
use super::*;
use crate::git::Tree;
use crate::restore::{self as pick, Choice, Source, Version};
use std::time::SystemTime;

/// What `restore` takes from the command line.
#[derive(Debug, Default, Clone)]
pub struct RestoreChoices {
    /// One file, a path in the config folder. `None`: all files.
    pub file: Option<String>,
    /// `--at`: a time, as git reads it ("2 days ago", "2026-10-12 14:00").
    pub at: Option<String>,
    /// `--repository`: the repository to restore from.
    pub repository: Option<String>,
    /// `--printer-name`: the printer to restore from.
    pub printer_name: Option<String>,
}

/// What `restore` did.
#[derive(Debug)]
pub struct RestoreReport {
    /// The file to restore. `None`: all files.
    pub file: Option<String>,
    /// The backup, for example "the backup of printer-2 of 2026-10-12 13:41".
    pub from: String,
    /// The files that restore wrote.
    pub written: Vec<String>,
    /// The files that already had the content of the backup.
    pub same: usize,
    /// Files in the config folder that aren't in the backup. They stay.
    pub not_in_backup: Vec<String>,
    /// The local time of the backup with the files from before the restore.
    pub before: Option<String>,
    /// After a new install: the reference with the backups that were only on this printer.
    pub kept: Option<String>,
    /// Moonraker reads a restored file too (a new install, or moonraker.conf).
    pub restart_moonraker: bool,
    /// The backup run that recorded the restore.
    pub backup: Option<BackupReport>,
}

impl RestoreReport {
    pub fn lines(&self) -> Vec<String> {
        let what = match &self.file {
            Some(file) => file.clone(),
            None => count(self.written.len(), "file"),
        };
        if self.written.is_empty() {
            let folder = match &self.file {
                Some(file) => file.clone(),
                None => "The config folder".to_string(),
            };
            return vec![format!(
                "{folder} already has the content of {}. Nothing changed.",
                self.from
            )];
        }
        let mut lines = vec![format!("Restored {what} from {}.", self.from)];
        if self.file.is_none() && self.same > 0 {
            lines.push(format!(
                "{} already had the same content.",
                count(self.same, "file")
            ));
        }
        if !self.not_in_backup.is_empty() {
            lines.push(format!(
                "These files aren't in that backup, and they stay as they are: {}",
                self.not_in_backup.join(", ")
            ));
        }
        if let Some(kept) = &self.kept {
            lines.push(format!(
                "The backups that were only on this printer stay on it as {kept}, and aren't sent."
            ));
        }
        if let Some(before) = &self.before {
            let short = before.get(..16).unwrap_or(before);
            lines.push(match &self.file {
                Some(file) => format!(
                    "Your version from before the restore is in the backup of {short}. To go back to it, \
                     run:\n  printerkeep restore {file} --at \"{before}\""
                ),
                None => format!(
                    "Your files from before the restore are in the backup of {short}. To go back to them, \
                     run:\n  printerkeep restore --at \"{before}\""
                ),
            });
        }
        if let Some(backup) = &self.backup {
            let mut more = backup.lines();
            more.retain(|line| {
                !line.starts_with("Backup made: ") && line != "No files changed. No backup made."
            });
            lines.extend(more);
        }
        let files = if self.written.len() == 1 {
            "file"
        } else {
            "files"
        };
        lines.push(if self.restart_moonraker {
            format!(
                "Restart Moonraker and Klipper to use the restored {files}: sudo systemctl restart \
                 moonraker klipper"
            )
        } else {
            format!("Restart Klipper to use the restored {files}: sudo systemctl restart klipper")
        });
        lines
    }
}

/// The reference that holds a fetched branch of another printer or repository during a restore.
const SOURCE_REF: &str = "refs/printerkeep/restore-source";

const DAY: u64 = 86_400;

/// Removes the fetched source branch at the end of the restore, also after an error.
struct SourceRef<'a>(&'a Git);

impl Drop for SourceRef<'_> {
    fn drop(&mut self) {
        let _ = self.0.delete_ref(SOURCE_REF);
    }
}

/// Where the backups come from.
struct Origin {
    source: Source,
    /// The reference with the backups.
    reference: String,
    /// The printer, when it isn't this printer's own history.
    printer: Option<String>,
    /// After a new install: the printer's name from now on.
    printer_name: String,
    kept: Option<String>,
    /// After a new install: the repository, for printerkeep.conf.
    url: Option<String>,
}

/// Restores files from a backup. First a backup run keeps the files now (and gets the repository's
/// changes). Then printerkeep picks the backup, writes its files, and makes a backup that says what it
/// restored. Never removes files.
///
/// - Without `repository` and `printer_name`: this printer's history.
/// - With another printer or another repository: the files come from there, and this printer keeps its
///   name, repository and history.
/// - With `repository` on a printer that isn't connected (a new install): printerkeep connects, finds which
///   printer this is (asks, or uses `printer_name`), and continues its history.
pub fn restore(
    paths: &Paths,
    choices: &RestoreChoices,
    token: AskToken,
    ask: &dyn Ask,
) -> Result<RestoreReport, Error> {
    let (settings, current) = load(paths)?;
    let file = match &choices.file {
        Some(file) => Some(restore_path(paths, file)?),
        None => None,
    };
    if let Some(name) = &choices.printer_name {
        check_name(name)?;
    }
    let _lock = lock(paths)?;
    let git = open_git(paths, &current)?;
    let _source_ref = SourceRef(&git);

    std::fs::create_dir_all(&paths.config).map_err(|source| Error::Io {
        context: format!("Can't make the config folder {}", paths.config.display()),
        source,
    })?;
    // The files now go into the history first: a restore never loses an edit.
    backup_and_send(paths, &settings, &current, COMMAND)?;
    let before = match git.resolve(&branch(&current))? {
        Some(tip) => git.log(&tip, None, None, None, Some(1))?.pop(),
        None => None,
    };

    let from = source(paths, &git, &settings, &current, choices, token, ask)?;
    let path = file.as_deref().map(str::as_bytes);
    let chosen = choose(
        paths,
        &git,
        &from,
        file.as_deref(),
        choices.at.as_deref(),
        ask,
    )?;

    let named = file.is_some();
    let tree: Tree = git
        .read_tree(&chosen.commit)?
        .into_iter()
        .filter(|(p, _)| path.is_none_or(|f| p.as_slice() == f))
        .filter(|(p, _)| pick::writes(p, named, from.source))
        .collect();
    if let Some(file) = &file
        && tree.is_empty()
    {
        return Err(Error::CannotUseFile {
            file: file.clone(),
            reason: "it names this printer and its repository, so restore takes it only from this \
                     printer's own backups"
                .to_string(),
        });
    }
    let not_in_backup = match file {
        Some(_) => Vec::new(),
        None => not_in(paths, &settings, &git.read_tree(&chosen.commit)?)?,
    };
    let backup_of = match &from.printer {
        Some(printer) => format!("the backup of {printer} of {}", chosen.shown_time()),
        None => format!("the backup of {}", chosen.shown_time()),
    };

    let plan = repository::plan_restore(&git, &tree, &paths.config)?;
    let written: Vec<String> = plan
        .write
        .iter()
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect();
    let mut report = RestoreReport {
        file: file.clone(),
        from: backup_of,
        written,
        same: plan.same,
        not_in_backup,
        before: None,
        kept: from.kept.clone(),
        restart_moonraker: false,
        backup: None,
    };
    // The printer name and the repository go into printerkeep.conf after the restored files: printerkeep.conf
    // from the backup can have an older URL.
    let set_options = || -> Result<(), Error> {
        if let Some(url) = &from.url {
            settings::set_options(
                &paths.settings(),
                &[("printer_name", &from.printer_name), ("repository", url)],
            )?;
        }
        Ok(())
    };
    if plan.write.is_empty() {
        set_options()?;
        return Ok(report);
    }
    if from.source != Source::NewInstall
        && let Some(before) = &before
    {
        // The backup of the restore gets a later second, so that --at with this time finds the files from
        // before the restore.
        while now() == before.time {
            std::thread::sleep(Duration::from_millis(100));
        }
        report.before = Some(before.local_time.clone());
    }
    repository::write_files(&git, &tree, &paths.config, &plan.write)?;
    set_options()?;
    report.restart_moonraker =
        from.source == Source::NewInstall || report.written.iter().any(|f| f == "moonraker.conf");

    // A new install that takes the last backup continues it: there's no restore to tell about.
    let tip = git.resolve(&from.reference)?;
    let subject = format!(
        "Restored {} from {}",
        file.clone()
            .unwrap_or_else(|| count(report.written.len(), "file")),
        report.from
    );
    let message = if from.source == Source::NewInstall && tip.as_deref() == Some(&chosen.commit) {
        backup::Message::Changes
    } else {
        backup::Message::Subject(&subject)
    };
    let settings = settings::load(&paths.settings())?;
    report.backup = Some(backup_and_send(
        paths,
        &settings,
        &from.printer_name,
        Run { message, ..COMMAND },
    )?);
    Ok(report)
}

/// Finds the reference with the backups to restore from. After a new install, printerkeep connects to the
/// repository and continues the printer's history.
fn source(
    paths: &Paths,
    git: &Git,
    settings: &Settings,
    current: &str,
    choices: &RestoreChoices,
    token: AskToken,
    ask: &dyn Ask,
) -> Result<Origin, Error> {
    let other_printer = choices
        .printer_name
        .as_deref()
        .is_some_and(|name| name != current);
    let url = match (&choices.repository, &settings.repository) {
        (Some(url), None) => return new_install(paths, git, current, url, choices, token, ask),
        (Some(url), _) => url.clone(),
        (None, Some(url)) if other_printer => url.clone(),
        (None, None) if other_printer => return Err(Error::NoRepository),
        (None, _) => {
            return Ok(Origin {
                source: Source::Own,
                reference: branch(current),
                printer: None,
                printer_name: current.to_string(),
                kept: None,
                url: None,
            });
        }
    };
    let connected = settings.repository.as_deref() == Some(url.as_str());
    if connected && !other_printer {
        return Ok(Origin {
            source: Source::Own,
            reference: branch(current),
            printer: None,
            printer_name: current.to_string(),
            kept: None,
            url: None,
        });
    }

    // Another printer or another repository: only a place to restore from.
    let (remote, temporary_token) = if connected {
        (repository::remote(&url, paths)?, false)
    } else {
        log_in_once(git, paths, &url, token)?
    };
    let fetched = (|| {
        let available = printer_branches(git, &remote)?;
        let printer = which_printer(
            &available,
            choices.printer_name.as_deref(),
            current,
            &url,
            "Which printer do you want to restore from? Type its name",
            ask,
        )?;
        git.fetch(&remote, &printer, SOURCE_REF)
            .map_err(|e| repository::classify(e, &url))?;
        Ok::<_, Error>(printer)
    })();
    if temporary_token {
        repository::discard_new_token(paths);
    }
    let printer = fetched?;
    Ok(Origin {
        source: Source::Other,
        reference: SOURCE_REF.to_string(),
        printer: Some(printer),
        printer_name: current.to_string(),
        kept: None,
        url: None,
    })
}

/// A new install: connects to the repository, finds which printer this is, and continues its history. The
/// backups that were only on this printer (for example the install's first backup) are set aside.
fn new_install(
    paths: &Paths,
    git: &Git,
    current: &str,
    url: &str,
    choices: &RestoreChoices,
    token: AskToken,
    ask: &dyn Ask,
) -> Result<Origin, Error> {
    let kind = repository::url_kind(url)?;
    let remote = log_in(git, paths, url, kind, current, token)?.remote;
    let available = printer_branches(git, &remote)?;
    let printer_name = which_printer(
        &available,
        choices.printer_name.as_deref(),
        current,
        url,
        "Which printer is this? Type its name",
        ask,
    )?;
    let fetched = repository::fetched_ref(&printer_name);
    git.fetch(&remote, &printer_name, &fetched)
        .map_err(|e| repository::classify(e, url))?;
    let commit = git.resolve(&fetched)?.unwrap_or_default();
    if printer_name != current {
        repository::move_local(git, current, &printer_name)?;
    }
    let kept = match repository::continue_history(git, &printer_name, &commit)? {
        History::Continued { kept } => kept,
        _ => None,
    };
    Ok(Origin {
        source: Source::NewInstall,
        reference: fetched,
        printer: Some(printer_name.clone()),
        printer_name,
        kept,
        url: Some(url.to_string()),
    })
}

/// Logs in to a repository that isn't this printer's, without storing anything. Returns the remote, and
/// `true` if it uses a new token that the caller must discard.
fn log_in_once(
    git: &Git,
    paths: &Paths,
    url: &str,
    token: AskToken,
) -> Result<(git::Remote, bool), Error> {
    if repository::url_kind(url)? != UrlKind::Https {
        return Ok((repository::remote(url, paths)?, false));
    }
    let mut request = TokenRequest::New;
    if repository::has_token(paths, url) {
        let remote = repository::remote(url, paths)?;
        match git.remote_branches(&remote) {
            Ok(_) => return Ok((remote, false)),
            Err(error) => match repository::classify(error, url) {
                RepositoryError::LoginFailed { .. } => request = TokenRequest::StoredRefused,
                error => return Err(error.into()),
            },
        }
    }
    let new_token = token(request, url)
        .map_err(|source| Error::Io {
            context: "Can't read the token".into(),
            source,
        })?
        .trim()
        .to_string();
    if new_token.is_empty() {
        return Err(Error::NoToken);
    }
    repository::store_new_token(paths, url, &new_token)?;
    let remote = repository::new_token_remote(url, paths);
    match git.remote_branches(&remote) {
        Ok(_) => Ok((remote, true)),
        Err(error) => {
            repository::discard_new_token(paths);
            Err(match repository::classify(error, url) {
                RepositoryError::LoginFailed { .. } => Error::TokenRefused {
                    url: url.to_string(),
                },
                error => error.into(),
            })
        }
    }
}

/// The printer in the repository: `wanted`, this printer's name, the only printer, or the user's answer.
fn which_printer(
    available: &[String],
    wanted: Option<&str>,
    current: &str,
    url: &str,
    question: &str,
    ask: &dyn Ask,
) -> Result<String, Error> {
    let no_such = |name: &str| Error::NoSuchPrinter {
        name: name.to_string(),
        url: url.to_string(),
        available: available.to_vec(),
    };
    match wanted {
        Some(name) if available.iter().any(|b| b == name) => Ok(name.to_string()),
        Some(name) => Err(no_such(name)),
        None if available.iter().any(|b| b == current) => Ok(current.to_string()),
        None => match available {
            [] => Err(Error::NothingToRestore),
            [one] => Ok(one.clone()),
            _ => {
                let question = format!(
                    "The repository has these printers:\n{}\n{question}",
                    list(available)
                );
                match ask.text(&question).map(|a| a.trim().to_string()) {
                    Some(name) if available.contains(&name) => Ok(name),
                    Some(name) => Err(no_such(&name)),
                    None => Err(Error::SelectPrinter {
                        url: url.to_string(),
                        printers: available.to_vec(),
                    }),
                }
            }
        },
    }
}

/// Picks the backup: at the time `at`, or from a list on a terminal.
fn choose(
    paths: &Paths,
    git: &Git,
    from: &Origin,
    file: Option<&str>,
    at: Option<&str>,
    ask: &dyn Ask,
) -> Result<Version, Error> {
    let reference = from.reference.as_str();
    let path = file.map(str::as_bytes);
    let what = file.unwrap_or("the config folder");
    let Some(at) = at else {
        let newest = versions(git, reference, path, None, None, Some(1))?;
        if from.source != Source::Own {
            // The last backup of another printer, or of this printer after a new install.
            return newest.into_iter().next().ok_or_else(|| missing(file));
        }
        if newest.is_empty() {
            return Err(missing(file));
        }
        let now_oid = match file {
            Some(file) if paths.config.join(file).is_file() => {
                git.hash_files(&[paths.config.join(file)], false)?.pop()
            }
            _ => None,
        };
        let tip = git.resolve(reference)?;
        let mut earlier = Vec::new();
        for version in versions(
            git,
            reference,
            path,
            None,
            None,
            Some(pick::LIST_LENGTH + 1),
        )? {
            let same_as_now = match path {
                Some(path) => now_oid.is_some() && git.file_oid(&version.commit, path)? == now_oid,
                None => tip.as_deref() == Some(version.commit.as_str()),
            };
            if !same_as_now {
                earlier.push(version);
            }
        }
        earlier.truncate(pick::LIST_LENGTH);
        if earlier.is_empty() {
            return Err(Error::NoOtherVersion(what.to_string()));
        }
        let list: Vec<&Version> = earlier.iter().collect();
        let title = match file {
            Some(file) => format!("These are the earlier versions of {file}:"),
            None => "These are the earlier backups:".to_string(),
        };
        let Some(answer) = ask.text(&pick::question(&title, &list, None)) else {
            return Err(Error::PickVersion(restore_command(file, "1 hour ago")));
        };
        return picked(&answer, &list, None);
    };

    let time = read_time(git, at)?;
    let Some(at_or_before) = versions(git, reference, path, None, Some(time), Some(1))?
        .into_iter()
        .next()
    else {
        let oldest = versions(git, reference, path, None, None, None)?
            .pop()
            .ok_or_else(|| missing(file))?;
        return Err(Error::NoBackupThen {
            what: what.to_string(),
            oldest: oldest.shown_time().to_string(),
            command: restore_command(file, &oldest.local_time),
        });
    };
    let day = git.approxidate(&format!("@{time} 00:00"))?;
    let on_day = versions(
        git,
        reference,
        path,
        Some(day),
        Some(day + DAY - 1),
        Some(50),
    )?;
    match pick::choose_at(&on_day, &at_or_before) {
        Choice::Use(version) => Ok(version.clone()),
        Choice::Ask { list, default } => {
            let title = match file {
                Some(file) => format!("{file} has these versions around {}:", at_or_before.date()),
                None => format!("These backups are around {}:", at_or_before.date()),
            };
            match ask.text(&pick::question(&title, &list, default)) {
                // No terminal: the state at that time.
                None => Ok(at_or_before.clone()),
                Some(answer) => picked(&answer, &list, default),
            }
        }
    }
}

/// The backups on `reference` (newest first) that have the file. Without `path`: all backups.
fn versions(
    git: &Git,
    reference: &str,
    path: Option<&[u8]>,
    since: Option<u64>,
    until: Option<u64>,
    limit: Option<usize>,
) -> Result<Vec<Version>, Error> {
    let Some(path) = path else {
        return Ok(git.log(reference, None, since, until, limit)?);
    };
    // A backup that removed the file changed it too: skip those, and ask for some more.
    let mut versions = Vec::new();
    for version in git.log(reference, Some(path), since, until, limit.map(|n| n + 5))? {
        if git.file_oid(&version.commit, path)?.is_some() {
            versions.push(version);
        }
    }
    if let Some(limit) = limit {
        versions.truncate(limit);
    }
    Ok(versions)
}

fn picked(answer: &str, list: &[&Version], default: Option<usize>) -> Result<Version, Error> {
    if answer.trim().is_empty() && default.is_none() {
        return Err(Error::NoVersionPicked);
    }
    match pick::answer(answer, list.len(), default) {
        Some(index) => Ok(list[index].clone()),
        None => Err(Error::NotAChoice(answer.trim().to_string())),
    }
}

fn missing(file: Option<&str>) -> Error {
    match file {
        Some(file) => Error::NotInHistory(file.to_string()),
        None => Error::NoBackups,
    }
}

/// The restore command for a file (or all files) at a time.
fn restore_command(file: Option<&str>, at: &str) -> String {
    match file {
        Some(file) => format!("printerkeep restore {file} --at \"{at}\""),
        None => format!("printerkeep restore --at \"{at}\""),
    }
}

/// Reads a time with git. A date alone means the end of that day. git reads text that it doesn't
/// understand as the time now: that's an error, unless the text is "now".
fn read_time(git: &Git, at: &str) -> Result<u64, Error> {
    let text = at.trim();
    let date_only = text.len() == 10
        && text.bytes().enumerate().all(|(i, b)| {
            if i == 4 || i == 7 {
                b == b'-'
            } else {
                b.is_ascii_digit()
            }
        });
    let query = if date_only {
        format!("{text} 23:59:59")
    } else {
        text.to_string()
    };
    let time = git.approxidate(&query)?;
    let now = now();
    if text != "now" && !exact_time(text) && time.abs_diff(now) <= 2 {
        return Err(Error::UnreadableTime(at.to_string()));
    }
    Ok(time.min(now))
}

/// `true` for a time such as `2026-10-14 09:24:58` or `2026-10-14 09:24`, which git always reads, or
/// `@<seconds since 1970>`.
fn exact_time(text: &str) -> bool {
    if let Some(seconds) = text.strip_prefix('@') {
        return !seconds.is_empty() && seconds.bytes().all(|b| b.is_ascii_digit());
    }
    let pattern = b"dddd-dd-dd dd:dd:dd";
    (text.len() == 16 || text.len() == 19)
        && text.bytes().zip(pattern).all(|(b, &p)| {
            if p == b'd' {
                b.is_ascii_digit()
            } else {
                b == p
            }
        })
}

/// The path of the file to restore, in the config folder: relative, or absolute in the config folder.
fn restore_path(paths: &Paths, file: &str) -> Result<String, Error> {
    let relative = match Path::new(file).strip_prefix(&paths.config) {
        Ok(relative) => relative.to_string_lossy().into_owned(),
        Err(_) => file.strip_prefix("./").unwrap_or(file).to_string(),
    };
    let path = Path::new(&relative);
    if relative.is_empty()
        || !path
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::CannotUseFile {
            file: file.to_string(),
            reason: "use a path in the config folder, for example printer.cfg".to_string(),
        });
    }
    if relative == STATE_FILE {
        return Err(Error::CantRestoreState);
    }
    Ok(relative)
}

/// The files in the config folder that aren't in `tree`, except printerkeep's own files.
fn not_in(paths: &Paths, settings: &Settings, tree: &Tree) -> Result<Vec<String>, Error> {
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    Ok(scan(paths, &rules)?
        .files
        .into_iter()
        .filter(|f| !tree.contains_key(&f.path))
        .filter(|f| pick::writes(&f.path, false, Source::Other))
        .map(|f| String::from_utf8_lossy(&f.path).into_owned())
        .collect())
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
