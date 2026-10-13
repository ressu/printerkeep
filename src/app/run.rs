//! The backup run: a backup, changes from shared and from the repository, and send (`printerkeep backup`).

use super::*;

/// What a backup run did.
#[derive(Debug)]
pub struct BackupReport {
    pub printer_name: String,
    pub outcome: Outcome,
    /// Symlinks and files that printerkeep does not back up.
    pub skipped: Vec<String>,
    /// What happened to the followed files.
    pub shared: Vec<String>,
    /// What happened with changes in the repository on this printer's branch.
    pub from_repository: Vec<String>,
    /// What happened with the repository. `None` if there is no repository.
    pub sent: Option<Result<Sent, RepositoryError>>,
    /// Nothing happened: no backup, no change, nothing sent, no problem. The log does not get this run, so
    /// that a run with no changes writes nothing to the SD card.
    pub quiet: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// A backup was made. The summary is the first line of its message.
    Made {
        commit: String,
        summary: String,
    },
    NoChange,
    /// No backup yet, and the config folder has no files.
    NoFiles,
}

impl BackupReport {
    /// The lines for the terminal and the log.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = match &self.outcome {
            Outcome::Made { summary, .. } => {
                let mut lines = vec![format!("Backup made: {summary}")];
                if !self.skipped.is_empty() {
                    lines.push(format!(
                        "Not backed up (symlinks): {}",
                        self.skipped.join(", ")
                    ));
                }
                lines
            }
            Outcome::NoChange => vec!["No files changed. No backup made.".to_string()],
            Outcome::NoFiles => vec!["The config folder has no files. No backup made.".to_string()],
        };
        lines.extend(self.shared.iter().cloned());
        lines.extend(self.from_repository.iter().cloned());
        match &self.sent {
            None | Some(Ok(Sent::Nothing)) => {}
            Some(Ok(Sent::Sent(1))) => lines.push("Sent 1 backup to the repository.".to_string()),
            Some(Ok(Sent::Sent(n))) => lines.push(format!("Sent {n} backups to the repository.")),
            Some(Err(RepositoryError::RepositoryAhead)) => lines.push(
                "The repository has changes that are not on this printer. printerkeep did not send the \
                 backups, so nothing in the repository is overwritten. The backups stay on this printer."
                    .to_string(),
            ),
            Some(Err(error @ RepositoryError::LoginFailed { .. })) => lines.push(error.to_string()),
            Some(Err(error)) => lines.push(format!(
                "Cannot send the backups: {error}. printerkeep tries again at the next backup."
            )),
        }
        lines
    }
}

/// Makes a backup if a file changed, and sends new backups to the repository. `check_now`: check the
/// repository for changes whatever `check_repository` says (for a run by hand).
pub fn backup(paths: &Paths, check_now: bool) -> Result<BackupReport, Error> {
    let (settings, printer_name) = load(paths)?;
    let _lock = lock(paths)?;
    backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            check_now,
            ..Run::default()
        },
    )
}

/// How a run works.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Run<'a> {
    /// Replaces the message of the first backup.
    pub(super) message: Option<&'a str>,
    /// The backups stay on the printer.
    pub(super) no_send: bool,
    /// Check the repository for changes now, whatever `check_repository` says.
    pub(super) check_now: bool,
}

/// A run of a command: check the repository for changes now.
pub(super) const COMMAND: Run = Run {
    message: None,
    no_send: false,
    check_now: true,
};

/// The backup run: a backup of the config folder, then changes from shared and from the repository, then
/// send. The caller holds the lock.
///
/// A run with no changes writes nothing to the SD card: the repository check is `git ls-remote`, and
/// printerkeep fetches a branch only when it moved.
pub(super) fn backup_and_send(
    paths: &Paths,
    settings: &Settings,
    printer_name: &str,
    run: Run,
) -> Result<BackupReport, Error> {
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let git = open_git(paths, printer_name)?;
    // The files on the printer go into the history before printerkeep writes a file.
    // README.md for the printer branch in the repository (not in the config folder).
    let readme = settings
        .repository
        .as_deref()
        .map(|url| readme::content(printer_name, url, &settings.follow));
    let files = Files {
        rules: &rules,
        readme: readme.as_deref(),
    };
    let (outcome, skipped) = make_backup(paths, files, &git, printer_name, run.message)?;
    let remote = match &settings.repository {
        Some(url) => Some(repository::remote(url, paths)?),
        None => None,
    };

    // What the repository has now. `None`: no check in this run.
    let mut problems = Vec::new();
    let heads = match &remote {
        Some(remote) if run.check_now || check_due(settings, paths) => {
            mark_checked(paths);
            match git.remote_heads(remote, &[SHARED_BRANCH, printer_name]) {
                Ok(heads) => Some(heads),
                Err(error) => {
                    problems.push(format!(
                        "Cannot check the repository for changes: {}. printerkeep tries again later.",
                        repository::classify(error, &remote.url)
                    ));
                    None
                }
            }
        }
        _ => None,
    };
    let (mut shared, shared_news) = follow_shared(
        paths,
        settings,
        files,
        &git,
        printer_name,
        remote.as_ref(),
        heads.as_ref(),
    )?;

    // Changes on this printer's branch in the repository (M6).
    let mut from_repository = Vec::new();
    if let (Some(remote), Some(tip)) = (&remote, heads.as_ref().and_then(|h| h.get(printer_name)))
        && branch_moved(&git, printer_name, tip)?
    {
        let fetched = repository::fetched_ref(printer_name);
        match git.fetch(remote, printer_name, &fetched) {
            Ok(()) => {
                if let Some(lines) = take_repository_changes(paths, files, &git, printer_name)? {
                    from_repository = lines;
                }
            }
            Err(error) => problems.push(format!(
                "Cannot get the changes from the repository: {}. printerkeep tries again later.",
                repository::classify(error, &remote.url)
            )),
        }
    }

    let send_now = || {
        remote
            .as_ref()
            .filter(|_| !run.no_send)
            .map(|remote| repository::send(&git, remote, printer_name))
    };
    let mut sent = send_now();
    // The repository has changes on this printer's branch that the check did not see: merge them, then
    // send again.
    if let Some(Err(RepositoryError::RepositoryAhead)) = &sent
        && let Some(lines) = take_repository_changes(paths, files, &git, printer_name)?
    {
        from_repository.extend(lines);
        sent = send_now();
    }
    // status has no network. It shows the problem from the last try.
    match &sent {
        Some(Err(error)) => {
            let _ = std::fs::write(paths.send_problem(), format!("{error}\n"));
        }
        Some(Ok(_)) => {
            if paths.send_problem().exists() {
                let _ = std::fs::remove_file(paths.send_problem());
            }
        }
        None => {}
    }
    let quiet = outcome == Outcome::NoChange
        && !shared_news
        && from_repository.is_empty()
        && problems.is_empty()
        && matches!(sent, None | Some(Ok(Sent::Nothing)));
    shared.extend(problems);
    Ok(BackupReport {
        printer_name: printer_name.to_string(),
        outcome,
        skipped,
        shared,
        from_repository,
        sent,
        quiet,
    })
}

/// `true` if the repository's commit `tip` of this printer's branch is not in the printer's history.
fn branch_moved(git: &Git, printer_name: &str, tip: &str) -> Result<bool, Error> {
    if git.resolve(&repository::sent_ref(printer_name))?.as_deref() == Some(tip) {
        return Ok(false);
    }
    let Some(local) = git.resolve(&branch(printer_name))? else {
        return Ok(true);
    };
    // A commit that the printer does not have cannot be in its history.
    if git.resolve(tip)?.is_none() {
        return Ok(true);
    }
    Ok(!git.is_ancestor(tip, &local)?)
}

/// The file with the time of the last repository check. It is in `XDG_RUNTIME_DIR` (RAM, `/run/user/<id>`),
/// so it never writes to the SD card. One file for each printer_data folder. `None` without
/// `XDG_RUNTIME_DIR`: then printerkeep checks at each run.
fn last_check_file(paths: &Paths) -> Option<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").filter(|v| !v.is_empty())?;
    // FNV-1a: a short, stable name for the path.
    let hash = paths
        .printer_data
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325_u64, |hash, &b| {
            (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
        });
    Some(
        PathBuf::from(runtime)
            .join("printerkeep")
            .join(format!("last-check-{hash:016x}")),
    )
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `true` if `check_repository` says that a check is due.
fn check_due(settings: &Settings, paths: &Paths) -> bool {
    let settings::Check::Every(interval) = settings.check_repository else {
        return false;
    };
    let Some(last) = last_check_file(paths)
        .and_then(|file| std::fs::read_to_string(file).ok())
        .and_then(|text| text.trim().parse::<u64>().ok())
    else {
        return true;
    };
    let now = now();
    // The timer starts up to 30 seconds late: one minute less, so that "5m" checks at each run.
    now < last || now - last + 60 >= interval.as_secs()
}

fn mark_checked(paths: &Paths) {
    if let Some(file) = last_check_file(paths) {
        if let Some(folder) = file.parent() {
            let _ = std::fs::create_dir_all(folder);
        }
        let _ = std::fs::write(file, format!("{}\n", now()));
    }
}

/// Merges the changes on this printer's branch in the repository (fetched by `send`) into the config
/// folder, and makes a merge commit with both histories. Returns `None` if the histories have no common
/// commit: then printerkeep does not merge.
fn take_repository_changes(
    paths: &Paths,
    files: Files,
    git: &Git,
    printer_name: &str,
) -> Result<Option<Vec<String>>, Error> {
    let reference = branch(printer_name);
    let (Some(local), Some(remote)) = (
        git.resolve(&reference)?,
        git.resolve(&repository::fetched_ref(printer_name))?,
    ) else {
        return Ok(None);
    };
    let Some(base) = git.merge_base(&local, &remote)? else {
        return Ok(None);
    };
    let events = repository_changes::apply(git, paths, files.rules, &base, &remote)?;
    let lines = events.iter().map(|e| e.line()).collect();
    if base == local {
        // No new backups on the printer: continue the repository's history. A backup follows only if the
        // config folder is different from it (for example, a file that the repository removed).
        git.update_ref(
            &reference,
            &remote,
            Some(&local),
            "printerkeep: continue the repository's history",
        )?;
        git.set_ref(
            &repository::sent_ref(printer_name),
            &remote,
            "printerkeep: in the repository",
        )?;
        make_backup(paths, files, git, printer_name, None)?;
        return Ok(Some(lines));
    }
    let scan = backup_scan(paths, files)?;
    let prepared = backup::prepare(git, &scan, &reference, true)?;
    let tree = git.write_tree(&prepared.tree, &paths.index())?;
    let message = format!("{}\n", repository_changes::message(&events));
    let commit = git.commit(&tree, &[&local, &remote], &message, &identity(printer_name))?;
    git.update_ref(
        &reference,
        &commit,
        Some(&local),
        "printerkeep: join the repository's history",
    )?;
    // The repository has its own commits: they are not new backups to send.
    git.set_ref(
        &repository::sent_ref(printer_name),
        &remote,
        "printerkeep: in the repository",
    )?;
    Ok(Some(lines))
}

/// Makes a backup if a file changed. Returns what happened and the files that were not backed up.
fn make_backup(
    paths: &Paths,
    files: Files,
    git: &Git,
    printer_name: &str,
    message: Option<&str>,
) -> Result<(Outcome, Vec<String>), Error> {
    let scan = backup_scan(paths, files)?;
    let reference = branch(printer_name);
    let prepared = backup::prepare(git, &scan, &reference, true)?;
    let outcome = if prepared.parent.is_none() && prepared.tree.is_empty() {
        Outcome::NoFiles
    } else {
        match backup::commit(
            git,
            &prepared,
            &reference,
            &paths.index(),
            &identity(printer_name),
            message,
        )? {
            Some((commit, message)) => Outcome::Made {
                commit,
                summary: message.lines().next().unwrap_or_default().to_string(),
            },
            None => Outcome::NoChange,
        }
    };
    Ok((outcome, scan.skipped))
}

/// Changes from shared for the followed files, and decisions. Makes a backup of the result. Returns the
/// lines for the report.
fn follow_shared(
    paths: &Paths,
    settings: &Settings,
    files: Files,
    git: &Git,
    printer_name: &str,
    remote: Option<&git::Remote>,
    heads: Option<&std::collections::BTreeMap<String, String>>,
) -> Result<(Vec<String>, bool), Error> {
    let state_file = paths.config.join(STATE_FILE);
    let mut state = state::load(&state_file)?;
    if settings.follow.is_empty() && state.files.is_empty() {
        return Ok((Vec::new(), false));
    }
    let mut lines = Vec::new();
    let Some(remote) = remote else {
        lines.push(
            "printerkeep.conf has files to follow, but this printer has no repository. \
             To add one: printerkeep add-repository <url>"
                .to_string(),
        );
        return Ok((lines, false));
    };
    let local = git.resolve(SHARED_REF)?;
    let tip = match heads.map(|h| h.get(SHARED_BRANCH)) {
        // No check in this run: printerkeep's last copy of shared.
        None => local,
        // The repository has no shared branch.
        Some(None) => None,
        Some(Some(tip)) if Some(tip) == local.as_ref() => local,
        Some(Some(_)) => match shared::fetch(git, remote) {
            Ok(tip) => tip,
            Err(error) => {
                lines.push(format!(
                    "Cannot get changes from shared: {error}. printerkeep tries again at the next \
                     backup."
                ));
                // The last copy is still a correct shared version.
                local
            }
        },
    };
    let fetch_problem = !lines.is_empty();
    let events = shared::apply(git, paths, &settings.follow, tip.as_deref(), &mut state)?;
    state::save(&state_file, &state)?;
    lines.extend(events.iter().map(FileEvent::line));
    let message = shared::message(&events);
    if let Some(message) = &message {
        make_backup(paths, files, git, printer_name, Some(message))?;
    }
    // Lines that only repeat a status (for example, a file that still needs decision) are not news.
    let news = fetch_problem
        || message.is_some()
        || events
            .iter()
            .any(|e| matches!(e.event, shared::Event::Placeholders(_)));
    Ok((lines, news))
}
