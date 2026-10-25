//! The run: a backup, changes from shared and from the repository, and send (`printerkeep sync`). Without
//! the changes, only a backup and send (`printerkeep backup`).

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
    /// The problems of this run (sending, the repository check, changes from shared).
    pub problems: Vec<String>,
    /// The problems are not the same as at the last run.
    pub problem_changed: bool,
    /// The skipped symlinks are not the same as at the last backup.
    pub skipped_changed: bool,
    /// The last run had a problem, and this run has none.
    pub solved: bool,
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
    /// No backup: the changed files have only bundled changes. The text says which files.
    Waiting(String),
    /// No backup yet, and the config folder has no files.
    NoFiles,
}

/// The start of the line with the skipped symlinks.
const SKIPPED: &str = "Not backed up (symlinks): ";

impl BackupReport {
    /// The lines for the log: the same, but a problem only when it is new or changed, and nothing for a
    /// run in which nothing happened.
    pub fn log_lines(&self) -> Vec<String> {
        if self.quiet {
            return Vec::new();
        }
        let mut lines = self.lines();
        if !self.problem_changed {
            lines.retain(|line| !self.problems.contains(line));
        }
        lines
    }

    /// The lines for the terminal. The skipped symlinks only when the list changed (`status` shows them
    /// always), and the sent backups only when backups waited: more than one, or none made in this run.
    pub fn lines(&self) -> Vec<String> {
        let mut lines = match &self.outcome {
            Outcome::Made { summary, .. } => {
                let mut lines = vec![format!("Backup made: {summary}")];
                if self.skipped_changed && !self.skipped.is_empty() {
                    lines.push(format!("{SKIPPED}{}", self.skipped.join(", ")));
                }
                lines
            }
            Outcome::NoChange => vec!["No files changed. No backup made.".to_string()],
            Outcome::Waiting(text) => vec![format!("No backup made: {text}")],
            Outcome::NoFiles => vec!["The config folder has no files. No backup made.".to_string()],
        };
        lines.extend(self.shared.iter().cloned());
        lines.extend(self.from_repository.iter().cloned());
        let made = matches!(self.outcome, Outcome::Made { .. });
        match &self.sent {
            Some(Ok(Sent::Sent(1))) if !made => {
                lines.push("Sent 1 backup to the repository.".to_string())
            }
            Some(Ok(Sent::Sent(n))) if *n > 1 => {
                lines.push(format!("Sent {n} backups to the repository."))
            }
            _ => {}
        }
        lines.extend(self.problems.iter().cloned());
        if self.solved {
            lines.push("The problem of the last runs is solved.".to_string());
        }
        lines
    }
}

/// The full run: makes a backup if a file changed, writes the changes from shared and from the repository,
/// and sends new backups to the repository. `check_now`: check the repository for changes whatever
/// `check_repository` says (for a run by hand).
pub fn sync(paths: &Paths, check_now: bool) -> Result<BackupReport, Error> {
    let (settings, printer_name) = load(paths)?;
    let _lock = lock(paths)?;
    backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            check_now,
            bundle: true,
            ..Run::default()
        },
    )
}

/// Makes a backup if a file changed, and sends new backups to the repository. Never writes a file in the
/// config folder: changes from shared and from the repository wait for `sync`.
pub fn backup(paths: &Paths) -> Result<BackupReport, Error> {
    let (settings, printer_name) = load(paths)?;
    let _lock = lock(paths)?;
    backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            only_backup: true,
            ..Run::default()
        },
    )
}

/// How a run works.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Run<'a> {
    /// The message of the first backup of the run.
    pub(super) message: backup::Message<'a>,
    /// The backups stay on the printer.
    pub(super) no_send: bool,
    /// Check the repository for changes now, whatever `check_repository` says.
    pub(super) check_now: bool,
    /// Only a backup and send: no changes from shared or from the repository, so no file is written.
    pub(super) only_backup: bool,
    /// Bundled changes alone make no backup. Only for `sync`: a command that asks for a backup, or that
    /// writes files after it, gets all changes in the backup.
    pub(super) bundle: bool,
}

/// A run of a command: check the repository for changes now.
pub(super) const COMMAND: Run = Run {
    message: backup::Message::Changes,
    no_send: false,
    check_now: true,
    only_backup: false,
    bundle: false,
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
        .map(|url| branch_readme::content(printer_name, url, &settings.follow));
    let files = Files {
        rules: &rules,
        readme: readme.as_deref(),
    };
    let bundle = if run.bundle {
        Some(BundleRules::new(settings.bundled_changes())?)
    } else {
        None
    };
    let (outcome, skipped) = make_backup(
        paths,
        files,
        &git,
        printer_name,
        run.message,
        bundle.as_ref(),
    )?;
    let remote = match &settings.repository {
        Some(url) => Some(repository::remote(url, paths)?),
        None => None,
    };

    // What the repository has now. `None`: no check in this run.
    let mut problems = Vec::new();
    let heads = match &remote {
        Some(remote) if !run.only_backup && (run.check_now || check_due(settings, paths)) => {
            mark_checked(paths);
            match git.remote_heads(remote, &[SHARED_BRANCH, printer_name]) {
                Ok(heads) => Some(heads),
                Err(error) => {
                    problems.push(format!(
                        "Can't check the repository for changes. {} printerkeep tries again at the next run.",
                        sentence(&repository::classify(error, &remote.url).to_string())
                    ));
                    None
                }
            }
        }
        _ => None,
    };
    let (shared, shared_news, shared_problem) = if run.only_backup {
        (Vec::new(), false, None)
    } else {
        follow_shared(
            paths,
            settings,
            files,
            &git,
            printer_name,
            remote.as_ref(),
            heads.as_ref(),
        )?
    };
    problems.extend(shared_problem);
    // printerkeep.conf names this printer (connect and restore write it): the printer was connected
    // to this branch before. If their histories have no common backup, the run continues the repository's.
    let continue_unrelated = settings.printer_name.is_some();

    // Changes on this printer's branch in the repository (M6).
    let mut from_repository = Vec::new();
    if let (Some(remote), Some(tip)) = (&remote, heads.as_ref().and_then(|h| h.get(printer_name)))
        && branch_moved(&git, printer_name, tip)?
    {
        let fetched = repository::fetched_ref(printer_name);
        match git.fetch(remote, printer_name, &fetched) {
            Ok(()) => {
                if let Some(lines) = take_repository_changes(paths, files, &git, printer_name, continue_unrelated)? {
                    from_repository = lines;
                }
            }
            Err(error) => problems.push(format!(
                "Can't get the changes from the repository. {} printerkeep tries again at the next run.",
                sentence(&repository::classify(error, &remote.url).to_string())
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
        && !run.only_backup
        && let Some(lines) =
            take_repository_changes(paths, files, &git, printer_name, continue_unrelated)?
    {
        from_repository.extend(lines);
        sent = send_now();
    }
    // The problems of this run, as one text. status has no network: it shows the text of the last run. The
    // log gets a problem when it starts, when it changes and when it is solved, not at each run.
    if run.only_backup
        && let Some(Err(RepositoryError::RepositoryAhead)) = &sent
    {
        problems.push(
            "The repository has changes on this printer's branch, so printerkeep didn't send the backup. \
             To get the changes and send it, run:\n  printerkeep sync"
                .to_string(),
        );
    } else if let Some(Err(error)) = &sent {
        // The repository cannot be reached: the check found the same. One line says it.
        if matches!(error, RepositoryError::Connection { .. }) {
            problems.retain(|p| !p.starts_with("Can't check the repository for changes."));
        }
        problems.push(send_problem(error));
    }
    let problem = (!problems.is_empty()).then(|| problems.join("\n"));
    let previous = std::fs::read_to_string(paths.send_problem())
        .ok()
        .map(|text| text.trim_end().to_string());
    let problem_changed = problem != previous;
    if problem_changed {
        let _ = match &problem {
            Some(text) => std::fs::write(paths.send_problem(), format!("{text}\n")),
            None => std::fs::remove_file(paths.send_problem()),
        };
    }
    let solved = previous.is_some() && problem.is_none();
    // The same for the skipped symlinks: they are the same at almost each backup.
    let skipped_text = skipped.join("\n");
    let skipped_changed = matches!(outcome, Outcome::Made { .. })
        && std::fs::read_to_string(paths.skipped())
            .unwrap_or_default()
            .trim_end()
            != skipped_text;
    if skipped_changed {
        let _ = if skipped.is_empty() {
            std::fs::remove_file(paths.skipped())
        } else {
            std::fs::write(paths.skipped(), format!("{skipped_text}\n"))
        };
    }
    let quiet = matches!(outcome, Outcome::NoChange | Outcome::Waiting(_))
        && !shared_news
        && from_repository.is_empty()
        && !problem_changed
        && matches!(sent, None | Some(Ok(Sent::Nothing)) | Some(Err(_)));
    Ok(BackupReport {
        printer_name: printer_name.to_string(),
        outcome,
        skipped,
        shared,
        from_repository,
        sent,
        problems,
        problem_changed,
        skipped_changed,
        solved,
        quiet,
    })
}

/// `text` as a sentence: with a capital letter and a period at the end.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    let mut out: String = match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    };
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// The text for a send that did not work: what happened, and what to do.
fn send_problem(error: &RepositoryError) -> String {
    match error {
        RepositoryError::RepositoryAhead => "The repository changed while printerkeep sent the backups. \
            printerkeep did not send them, so nothing in the repository is overwritten. It tries again at \
            the next run."
            .to_string(),
        RepositoryError::LoginFailed { .. } => error.to_string(),
        RepositoryError::Refused { .. } => format!(
            "Can't send the backups. {error}\nprinterkeep tries again at the next run."
        ),
        error => format!(
            "Can't send the backups. {} printerkeep tries again at the next run.",
            sentence(&error.to_string())
        ),
    }
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
    continue_unrelated: bool,
) -> Result<Option<Vec<String>>, Error> {
    let reference = branch(printer_name);
    let (Some(local), Some(remote)) = (
        git.resolve(&reference)?,
        git.resolve(&repository::fetched_ref(printer_name))?,
    ) else {
        return Ok(None);
    };
    let Some(base) = git.merge_base(&local, &remote)? else {
        if !continue_unrelated {
            return Ok(None);
        }
        // No common backup, but printerkeep.conf names this branch: printerkeep's data on the printer was
        // removed, or the printer was set up again. Continue the repository's history; the files do not
        // change, and the next backup has them.
        let kept = match repository::continue_history(git, printer_name, &remote)? {
            History::Continued { kept } => kept,
            _ => None,
        };
        let mut lines = vec![format!(
            "The branch {printer_name} in the repository has backups that aren't in this printer's \
             history. That happens when printerkeep's data on the printer was removed, or the printer was \
             set up again. printerkeep.conf names this branch, so printerkeep continues its history. Your \
             files didn't change."
        )];
        if let Some(kept) = kept {
            lines.push(format!(
                "The backups that were only on this printer stay on it as {kept}, and aren't sent."
            ));
        }
        make_backup(
            paths,
            files,
            git,
            printer_name,
            backup::Message::Changes,
            None,
        )?;
        return Ok(Some(lines));
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
        make_backup(
            paths,
            files,
            git,
            printer_name,
            backup::Message::Changes,
            None,
        )?;
        return Ok(Some(lines));
    }
    let scan = backup_scan(paths, files)?;
    let prepared = backup::prepare(git, &scan, &reference, true, None)?;
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
    message: backup::Message,
    bundle: Option<&BundleRules>,
) -> Result<(Outcome, Vec<String>), Error> {
    let scan = backup_scan(paths, files)?;
    let reference = branch(printer_name);
    let prepared = backup::prepare(git, &scan, &reference, true, bundle)?;
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
            None => match prepared.waiting_text() {
                Some(text) => Outcome::Waiting(text),
                None => Outcome::NoChange,
            },
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
) -> Result<(Vec<String>, bool, Option<String>), Error> {
    let state_file = paths.config.join(STATE_FILE);
    let mut state = state::load(&state_file)?;
    if settings.follow.is_empty() && state.files.is_empty() {
        return Ok((Vec::new(), false, None));
    }
    let mut lines = Vec::new();
    let mut problem = None;
    let Some(remote) = remote else {
        lines.push(
            "printerkeep.conf has files to follow, but this printer isn't connected to a \
             repository. To connect it: printerkeep connect <url>"
                .to_string(),
        );
        return Ok((lines, false, None));
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
                problem = Some(format!(
                    "Can't get changes from shared. {} printerkeep tries again at the next run.",
                    sentence(&error.to_string())
                ));
                // The last copy is still a correct shared version.
                local
            }
        },
    };
    let events = shared::apply(git, paths, &settings.follow, tip.as_deref(), &mut state)?;
    state::save(&state_file, &state)?;
    lines.extend(events.iter().map(FileEvent::line));
    let message = shared::message(&events);
    if let Some(message) = &message {
        make_backup(
            paths,
            files,
            git,
            printer_name,
            backup::Message::Replace(message),
            None,
        )?;
    }
    // Lines that only repeat a status (for example, a file that still needs decision) are not news.
    let news = message.is_some()
        || events
            .iter()
            .any(|e| matches!(e.event, shared::Event::Placeholders(_)));
    Ok((lines, news, problem))
}
