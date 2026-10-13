//! The commands: settings, lock, printer name, backup, status, add-repository, restore, and the commands
//! for shared files.

use crate::backup::{self, Changes};
use crate::git::{self, Git, GitError, Identity};
use crate::ignore::{IgnoreRules, RuleError};
use crate::overrides;
use crate::paths::Paths;
use crate::readme;
use crate::repository::{self, History, RepositoryError, Sent, UrlKind};
use crate::repository_changes;
use crate::settings::{self, Settings, SettingsError};
use crate::shared::{self, FileEvent, SHARED_REF, SharedError};
use crate::snapshot::{self, Scan};
use crate::state::{self, FileState, STATE_FILE, StateError};
use std::fmt;
use std::fs::{File, TryLockError};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error(transparent)]
    Rule(#[from] RuleError),
    #[error(transparent)]
    Git(#[from] GitError),
    #[error(transparent)]
    Repository(#[from] RepositoryError),
    #[error(transparent)]
    Install(#[from] crate::install::InstallError),
    #[error(transparent)]
    State(#[from] StateError),
    #[error(transparent)]
    Shared(#[from] SharedError),
    #[error("Shared files need a repository. To add one: printerkeep add-repository <url>")]
    SharedNeedsRepository,
    #[error("printerkeep cannot use {file}: {reason}.")]
    CannotUseFile { file: String, reason: String },
    #[error("{0} is not in the config folder.")]
    NoSuchFile(String),
    #[error("{0} is not in shared. To add it to shared: printerkeep copy-to-shared {0}")]
    NotInShared(String),
    #[error("printerkeep does not follow {0}.")]
    NotFollowed(String),
    #[error("{0} does not need decision. It has no #### printerkeep lines.")]
    NoDecision(String),
    #[error(
        "printerkeep cannot read the #### lines in {0}: a part of a block is changed or missing. Edit the \
         file: keep the lines that you want, and delete all #### lines."
    )]
    BlockChanged(String),
    #[error(
        "{0} needs decision. Open the file and look for the #### lines. After the decision, printerkeep can \
         copy it to shared."
    )]
    FileNeedsDecision(String),
    #[error(
        "Shared changed {0} at the same time. Run printerkeep backup to get the changes, then try again."
    )]
    SharedChanged(String),
    #[error(
        "The config folder {0} does not exist. Use --printer-data to select the printer_data folder."
    )]
    NoConfigFolder(PathBuf),
    #[error("Another printerkeep run is active. Try again later.")]
    Locked,
    #[error(
        "The printer name '{0}' is not valid. Set printer_name in printerkeep.conf. \
         Use letters, numbers and '-', for example printer-1."
    )]
    InvalidPrinterName(String),
    #[error(
        "The printer name 'shared' is not available. Set a different printer_name in printerkeep.conf."
    )]
    ReservedPrinterName,
    #[error("No token. printerkeep needs a personal access token for an https:// repository.")]
    NoToken,
    #[error(
        "The repository did not accept the token, or the URL is wrong ({url}). The stored login did not change."
    )]
    TokenRefused { url: String },
    #[error("This printer has no repository. To add one: printerkeep add-repository <url>")]
    NoRepository,
    #[error(
        "The repository {0} uses an SSH key, not a token. If the repository does not accept the key, run: \
         printerkeep add-repository {0}"
    )]
    NoTokenForUrl(String),
    #[error(
        "The repository did not accept printerkeep's SSH key. Add this public key to the repository as a \
         deploy key with write access:\n\n{public_key}\n\n\
         (GitHub: the repository's Settings > Deploy keys > Add deploy key, then select \"Allow write access\".)\n\
         Then run this command again."
    )]
    DeployKeyNeeded { public_key: String },
    #[error("The repository has no backups.")]
    NothingToRestore,
    #[error(
        "The repository has backups of more printers: {}. Select one: printerkeep restore <url> <printer name>",
        .0.join(", ")
    )]
    SelectPrinter(Vec<String>),
    #[error("The repository has no printer named '{name}'. Printers in the repository: {}", .available.join(", "))]
    NoSuchPrinter {
        name: String,
        available: Vec<String>,
    },
    #[error(
        "These files in the config folder are different from the backup: {}. \
         To replace them with the files from the backup, add --overwrite.",
        .0.join(", ")
    )]
    RestoreWouldOverwrite(Vec<String>),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

pub use crate::shared::SHARED_BRANCH;

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
    let settings = settings::load(&paths.settings())?;
    let printer_name = printer_name(&settings)?;
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
struct Run<'a> {
    /// Replaces the message of the first backup.
    message: Option<&'a str>,
    /// The backups stay on the printer.
    no_send: bool,
    /// Check the repository for changes now, whatever `check_repository` says.
    check_now: bool,
}

/// A run of a command: check the repository for changes now.
const COMMAND: Run = Run {
    message: None,
    no_send: false,
    check_now: true,
};

/// The backup run: a backup of the config folder, then changes from shared and from the repository, then
/// send. The caller holds the lock.
///
/// A run with no changes writes nothing to the SD card: the repository check is `git ls-remote`, and
/// printerkeep fetches a branch only when it moved.
fn backup_and_send(
    paths: &Paths,
    settings: &Settings,
    printer_name: &str,
    run: Run,
) -> Result<BackupReport, Error> {
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(printer_name)?;
    }
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
    let commit =
        git.commit_with_parents(&tree, &[&local, &remote], &message, &identity(printer_name))?;
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

fn identity(printer_name: &str) -> Identity {
    Identity {
        name: "printerkeep".to_string(),
        email: format!("printerkeep@{printer_name}"),
    }
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

/// What `add-repository` did.
#[derive(Debug)]
pub struct AddReport {
    pub url: String,
    pub token: TokenUse,
    pub printer_name: String,
    pub history: History,
    pub backup: BackupReport,
}

impl AddReport {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Connected to the repository {}.", self.url)];
        match self.token {
            TokenUse::Stored => lines.push("Used the stored token.".to_string()),
            TokenUse::New => lines.push("Stored the token.".to_string()),
            TokenUse::None => {}
        }
        match &self.history {
            History::New => lines.push(format!(
                "The repository has no backups of {}. printerkeep sends the backups of this printer.",
                self.printer_name
            )),
            History::Same => {}
            History::Continued { kept } => {
                lines.push(format!(
                    "The repository has backups of {}. printerkeep continues that history.",
                    self.printer_name
                ));
                if let Some(kept) = kept {
                    lines.push(format!(
                        "The backups that were only on this printer are not sent. They stay on this \
                         printer as {kept}."
                    ));
                }
            }
        }
        lines.extend(self.backup.lines());
        lines
    }
}

/// Why printerkeep asks for a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRequest {
    /// No token is stored for the repository.
    New,
    /// The repository did not accept the stored token (for example, it expired).
    StoredRefused,
    /// The user replaces the token (`change-token`).
    Replace,
}

/// Asks the user for a token for the repository URL.
pub type AskToken<'a> = &'a dyn Fn(TokenRequest, &str) -> std::io::Result<String>;

/// Which token a connection used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenUse {
    /// Not an https:// repository.
    None,
    /// The token that was stored before.
    Stored,
    /// A new token from the user. It is stored now.
    New,
}

/// Connects the printer to a repository. `token` is called only for an https:// URL without a stored token
/// that works.
pub fn add_repository(paths: &Paths, url: &str, token: AskToken) -> Result<AddReport, Error> {
    let settings = settings::load(&paths.settings())?;
    let printer_name = printer_name(&settings)?;
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;

    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(&printer_name)?;
    }
    let connection = connect(&git, paths, url, kind, &printer_name, token)?;
    let remote = connection.remote;

    let history = match connection.commit {
        None => History::New,
        Some(_) => {
            let fetched = repository::fetched_ref(&printer_name);
            git.fetch(&remote, &printer_name, &fetched)
                .map_err(|e| repository::classify(e, url))?;
            let commit = git.resolve(&fetched)?.unwrap_or_default();
            repository::continue_history(&git, &printer_name, &commit)?
        }
    };

    // The URL only: never a token. The printer name, so that a new host name does not start a new history.
    settings::set_options(
        &paths.settings(),
        &[("printer_name", &printer_name), ("repository", url)],
    )?;
    let settings = settings::load(&paths.settings())?;
    let backup = backup_and_send(paths, &settings, &printer_name, COMMAND)?;
    Ok(AddReport {
        url: url.to_string(),
        token: connection.token,
        printer_name,
        history,
        backup,
    })
}

/// A working connection to the repository.
struct Connection {
    remote: git::Remote,
    /// The repository's commit for the printer, if there is one.
    commit: Option<String>,
    token: TokenUse,
}

/// Connects to the repository and tests the login. For https://, printerkeep uses the stored token, and
/// asks for a new one only if there is no stored token or the repository does not accept it. For SSH,
/// printerkeep makes its key, and shows it if the repository does not accept it.
fn connect(
    git: &Git,
    paths: &Paths,
    url: &str,
    kind: UrlKind,
    printer_name: &str,
    token: AskToken,
) -> Result<Connection, Error> {
    match kind {
        UrlKind::Https => connect_with_token(git, paths, url, printer_name, token, false),
        UrlKind::Ssh => {
            repository::ensure_ssh_key(paths, printer_name)?;
            let remote = repository::remote(url, paths)?;
            match git.remote_branch(&remote, printer_name) {
                Ok(commit) => Ok(Connection {
                    remote,
                    commit,
                    token: TokenUse::None,
                }),
                Err(error) => match repository::classify(error, url) {
                    RepositoryError::LoginFailed { .. } => Err(Error::DeployKeyNeeded {
                        public_key: repository::ensure_ssh_key(paths, printer_name)?,
                    }),
                    error => Err(error.into()),
                },
            }
        }
        UrlKind::Folder => {
            let remote = repository::remote(url, paths)?;
            let commit = git
                .remote_branch(&remote, printer_name)
                .map_err(|e| repository::classify(e, url))?;
            Ok(Connection {
                remote,
                commit,
                token: TokenUse::None,
            })
        }
    }
}

/// `replace`: do not use the stored token, ask for a new one.
fn connect_with_token(
    git: &Git,
    paths: &Paths,
    url: &str,
    printer_name: &str,
    token: AskToken,
    replace: bool,
) -> Result<Connection, Error> {
    let mut request = if replace {
        TokenRequest::Replace
    } else {
        TokenRequest::New
    };
    if !replace && repository::has_token(paths, url) {
        let remote = repository::remote(url, paths)?;
        match git.remote_branch(&remote, printer_name) {
            Ok(commit) => {
                return Ok(Connection {
                    remote,
                    commit,
                    token: TokenUse::Stored,
                });
            }
            Err(error) => match repository::classify(error, url) {
                RepositoryError::LoginFailed { .. } => request = TokenRequest::StoredRefused,
                error => return Err(error.into()),
            },
        }
    }

    let new_token = token(request, url)
        .map_err(|source| Error::Io {
            context: "Cannot read the token".into(),
            source,
        })?
        .trim()
        .to_string();
    if new_token.is_empty() {
        return Err(Error::NoToken);
    }
    // The new token replaces the stored one only when the repository accepts it.
    repository::store_new_token(paths, url, &new_token)?;
    match git.remote_branch(&repository::new_token_remote(url, paths), printer_name) {
        Ok(commit) => {
            repository::keep_new_token(paths)?;
            Ok(Connection {
                remote: repository::remote(url, paths)?,
                commit,
                token: TokenUse::New,
            })
        }
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

/// Replaces the token of the printer's repository. The new token must work before it replaces the old
/// one. Then printerkeep sends the backups that are not in the repository.
pub fn change_token(paths: &Paths, token: AskToken) -> Result<Vec<String>, Error> {
    let settings = settings::load(&paths.settings())?;
    let url = settings.repository.clone().ok_or(Error::NoRepository)?;
    if repository::url_kind(&url)? != UrlKind::Https {
        return Err(Error::NoTokenForUrl(url));
    }
    let printer_name = printer_name(&settings)?;
    let _lock = lock(paths)?;
    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(&printer_name)?;
    }
    connect_with_token(&git, paths, &url, &printer_name, token, true)?;
    let mut lines = vec![format!(
        "The repository {url} accepted the new token. printerkeep uses it from now on."
    )];
    lines.extend(backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines());
    Ok(lines)
}

/// What `restore` did.
#[derive(Debug)]
pub struct RestoreReport {
    pub printer_name: String,
    pub written: usize,
    pub same: usize,
}

impl RestoreReport {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "Restored {} from the backup of {}.",
            count(self.written, "file"),
            self.printer_name
        )];
        if self.same > 0 {
            lines.push(format!(
                "{} already had the same content.",
                count(self.same, "file")
            ));
        }
        lines.push(
            "Restart Moonraker and Klipper to use the restored files: \
             sudo systemctl restart moonraker klipper"
                .to_string(),
        );
        lines
    }
}

fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

/// Restores the config folder from the repository. Never removes files. Replaces different files only
/// with `overwrite`.
pub fn restore(
    paths: &Paths,
    url: &str,
    printer: Option<&str>,
    overwrite: bool,
    token: AskToken,
) -> Result<RestoreReport, Error> {
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;
    let key_name = printer.map_or_else(host_name, str::to_string);

    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(&key_name)?;
    }
    let remote = connect(&git, paths, url, kind, &key_name, token)?.remote;

    let available: Vec<String> = git
        .remote_branches(&remote)
        .map_err(|e| repository::classify(e, url))?
        .into_iter()
        .filter(|b| b != SHARED_BRANCH)
        .collect();
    let printer_name = match printer {
        Some(name) if available.iter().any(|b| b == name) => name.to_string(),
        Some(name) => {
            return Err(Error::NoSuchPrinter {
                name: name.to_string(),
                available,
            });
        }
        None => match available.as_slice() {
            [] => return Err(Error::NothingToRestore),
            [one] => one.clone(),
            _ => return Err(Error::SelectPrinter(available)),
        },
    };

    let fetched = repository::fetched_ref(&printer_name);
    git.fetch(&remote, &printer_name, &fetched)
        .map_err(|e| repository::classify(e, url))?;
    let commit = git.resolve(&fetched)?.unwrap_or_default();
    let tree = git.read_tree(&commit)?;

    std::fs::create_dir_all(&paths.config).map_err(|source| Error::Io {
        context: format!("Cannot make the config folder {}", paths.config.display()),
        source,
    })?;
    let plan = repository::plan_restore(&git, &tree, &paths.config)?;
    if !plan.different.is_empty() && !overwrite {
        return Err(Error::RestoreWouldOverwrite(plan.different));
    }
    repository::write_files(&git, &tree, &paths.config, &plan.write)?;
    repository::continue_history(&git, &printer_name, &commit)?;
    settings::set_options(
        &paths.settings(),
        &[("printer_name", &printer_name), ("repository", url)],
    )?;

    Ok(RestoreReport {
        printer_name,
        written: plan.write.len(),
        same: plan.same,
    })
}

/// The path of a file in the config folder, from the command line: relative, or absolute in the config folder.
fn config_path(paths: &Paths, file: &str) -> Result<String, Error> {
    let path = Path::new(file);
    let relative = match path.strip_prefix(&paths.config) {
        Ok(relative) => relative.to_string_lossy().into_owned(),
        Err(_) => file.strip_prefix("./").unwrap_or(file).to_string(),
    };
    shared::check_path(&relative).map_err(|reason| Error::CannotUseFile {
        file: file.to_string(),
        reason,
    })?;
    Ok(relative)
}

/// The settings, the printer name and the repository for a command on shared files.
fn shared_setup(paths: &Paths) -> Result<(Settings, String, git::Remote), Error> {
    let settings = settings::load(&paths.settings())?;
    let printer_name = printer_name(&settings)?;
    let url = settings
        .repository
        .clone()
        .ok_or(Error::SharedNeedsRepository)?;
    let remote = repository::remote(&url, paths)?;
    Ok((settings, printer_name, remote))
}

/// Starts to follow a shared file. If the file is on the printer, its differences become local changes.
/// If not, printerkeep writes the shared version.
pub fn follow(paths: &Paths, file: &str) -> Result<Vec<String>, Error> {
    let (settings, printer_name, remote) = shared_setup(paths)?;
    let path = config_path(paths, file)?;
    let _lock = lock(paths)?;
    if settings.follow.contains(&path) {
        return Ok(vec![format!("printerkeep already follows {path}.")]);
    }
    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(&printer_name)?;
    }
    let in_shared = match shared::fetch(&git, &remote)? {
        Some(tip) => git.read_tree(&tip)?.contains_key(path.as_bytes()),
        None => false,
    };
    if !in_shared {
        return Err(Error::NotInShared(path));
    }
    settings::add_to_list(&paths.settings(), "follow", &path)?;
    let settings = settings::load(&paths.settings())?;
    Ok(backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines())
}

/// Stops following a shared file. The file stays on the printer.
pub fn stop_following(paths: &Paths, file: &str) -> Result<Vec<String>, Error> {
    let settings = settings::load(&paths.settings())?;
    let printer_name = printer_name(&settings)?;
    let path = config_path(paths, file)?;
    let _lock = lock(paths)?;
    if !settings::remove_from_list(&paths.settings(), "follow", &path)? {
        return Err(Error::NotFollowed(path));
    }
    let settings = settings::load(&paths.settings())?;
    let mut lines = backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines();
    if std::fs::read(paths.config.join(&path)).is_ok_and(|c| crate::merge::has_block(&c)) {
        lines.push(format!(
            "{path} still has #### lines from a needs decision. Keep the lines that you want, and delete \
             the #### lines."
        ));
    }
    Ok(lines)
}

/// Makes the shared version of a file equal to this printer's version, and follows the file.
pub fn copy_to_shared(paths: &Paths, file: &str) -> Result<Vec<String>, Error> {
    let (settings, printer_name, remote) = shared_setup(paths)?;
    let path = config_path(paths, file)?;
    let disk = paths.config.join(&path);
    if !disk.is_file() {
        return Err(Error::NoSuchFile(path));
    }
    let _lock = lock(paths)?;

    // First the normal run: a backup, and the changes from shared that this printer does not have yet.
    let mut lines = backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            no_send: true,
            ..COMMAND
        },
    )?
    .lines();
    let state_file = paths.config.join(STATE_FILE);
    let mut state = state::load(&state_file)?;
    let io = |source| Error::Io {
        context: format!("Cannot read {}", disk.display()),
        source,
    };
    let content = std::fs::read(&disk).map_err(io)?;
    if state
        .files
        .get(&path)
        .is_some_and(|f| f.needs_decision.is_some())
        || crate::merge::has_block(&content)
    {
        return Err(Error::FileNeedsDecision(path));
    }
    let executable = std::fs::metadata(&disk).map_err(io)?.permissions().mode() & 0o111 != 0;

    let git = Git::new(&paths.git_dir);
    let message = format!("Copied {path} from {printer_name}");
    let mut attempts = 0;
    let mut not_copied;
    let (commit, copied) = loop {
        attempts += 1;
        let tip = shared::fetch(&git, &remote)?;
        // A followed file must have all changes from shared, else the copy removes changes of other printers.
        if let (Some(entry), Some(tip)) = (state.files.get(&path), &tip) {
            let base = shared::content_at(&git, &entry.base, &path)?;
            let now = shared::content_at(&git, tip, &path)?;
            if base.map(|b| crate::merge::to_lf(&b)) != now.map(|n| crate::merge::to_lf(&n)) {
                return Err(Error::SharedChanged(path));
            }
        }
        // Override lines stay on this printer (C6).
        let in_shared = match &tip {
            Some(tip) => shared::content_at(&git, tip, &path)?,
            None => None,
        };
        let for_shared = overrides::for_shared(&content, in_shared.as_deref());
        let entry = git::TreeEntry {
            mode: if executable { "100755" } else { "100644" }.to_string(),
            oid: git.write_blob(&for_shared.content)?,
        };
        not_copied = for_shared;
        let new = shared::copy_commit(
            &git,
            paths,
            tip.as_deref(),
            &path,
            entry,
            &message,
            &identity(&printer_name),
        )?;
        let Some(new) = new else {
            break (tip.unwrap_or_default(), false);
        };
        match git.push(&remote, &new, SHARED_BRANCH) {
            Ok(()) => {
                git.set_ref(SHARED_REF, &new, "printerkeep: copied to shared")?;
                break (new, true);
            }
            Err(error) => match repository::classify(error, &remote.url) {
                // Another printer changed shared at the same time. Try again on its version.
                RepositoryError::RepositoryAhead if attempts < 3 => continue,
                error => return Err(error.into()),
            },
        }
    };

    settings::add_to_list(&paths.settings(), "follow", &path)?;
    state.files.insert(
        path.clone(),
        FileState {
            base: commit,
            needs_decision: None,
        },
    );
    state::save(&state_file, &state)?;
    let settings = settings::load(&paths.settings())?;
    let summary = format!("Copied {path} to shared");
    let report = backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            message: Some(&summary),
            ..COMMAND
        },
    )?;
    lines.retain(|line| line != "No files changed. No backup made.");
    if copied {
        lines.push(format!(
            "Copied {path} to shared. Other printers can now follow it."
        ));
    } else {
        lines.push(format!(
            "{path} in shared is already the same as on this printer."
        ));
    }
    for key in &not_copied.kept {
        lines.push(format!(
            "Not copied: {key} (override). Shared keeps its own line."
        ));
    }
    for key in &not_copied.placeholders {
        lines.push(format!(
            "Not copied: {key} (override). Shared has the value {} there. Each printer that follows the \
             file sets its own value.",
            overrides::PLACEHOLDER
        ));
    }
    lines.extend(
        report.lines().into_iter().filter(|line| {
            !line.starts_with("Backup made:") && !line.starts_with("No files changed")
        }),
    );
    Ok(lines)
}

/// Finishes a needs-decision file without editing it: keeps this printer's lines, or uses the shared lines.
pub fn decide(paths: &Paths, file: &str, use_shared: bool) -> Result<Vec<String>, Error> {
    let settings = settings::load(&paths.settings())?;
    let printer_name = printer_name(&settings)?;
    let path = config_path(paths, file)?;
    let _lock = lock(paths)?;
    let disk = paths.config.join(&path);
    let content = std::fs::read(&disk).map_err(|source| Error::Io {
        context: format!("Cannot read {}", disk.display()),
        source,
    })?;
    if !crate::merge::has_block(&content) {
        return Err(Error::NoDecision(path));
    }
    let decided = crate::merge::decide(&content, use_shared)
        .ok_or_else(|| Error::BlockChanged(path.clone()))?;
    let temporary = disk.with_file_name(".printerkeep-write.tmp");
    std::fs::write(&temporary, &decided)
        .and_then(|()| {
            std::fs::set_permissions(&temporary, std::fs::metadata(&disk)?.permissions())
        })
        .and_then(|()| std::fs::rename(&temporary, &disk))
        .map_err(|source| Error::Io {
            context: format!("Cannot write {}", disk.display()),
            source,
        })?;
    let which = if use_shared {
        "used the shared lines"
    } else {
        "kept the lines of this printer"
    };
    let mut lines = vec![format!("{path}: {which}.")];
    lines.extend(backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines());
    Ok(lines)
}

/// The state of the printer, for `printerkeep status`.
#[derive(Debug)]
pub struct Status {
    pub printer_name: String,
    pub config: PathBuf,
    pub last_backup: Option<LastBackup>,
    /// Changes in the config folder since the last backup.
    pub changes: Changes,
    pub skipped: Vec<String>,
    pub repository: Option<String>,
    /// Backups that are not in the repository yet. `None` without a repository or backups.
    pub not_sent: Option<u64>,
    /// Why the last try to send did not work.
    pub send_problem: Option<String>,
    /// The followed files and their status.
    pub followed: Vec<(String, shared::FileStatus)>,
}

#[derive(Debug)]
pub struct LastBackup {
    pub time: String,
    pub summary: String,
    pub count: u64,
    pub files: usize,
}

/// Reads the state of the printer. Changes nothing and uses no network.
pub fn status(paths: &Paths) -> Result<Status, Error> {
    let settings = settings::load(&paths.settings())?;
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let printer_name = printer_name(&settings)?;
    let readme = settings
        .repository
        .as_deref()
        .map(|url| readme::content(&printer_name, url, &settings.follow));
    let scan = backup_scan(
        paths,
        Files {
            rules: &rules,
            readme: readme.as_deref(),
        },
    )?;

    let git = Git::new(&paths.git_dir);
    let reference = branch(&printer_name);
    let mut not_sent = None;
    let (last_backup, changes) = if git.exists() && git.resolve(&reference)?.is_some() {
        let prepared = backup::prepare(&git, &scan, &reference, false)?;
        let commit = prepared.parent.as_deref().unwrap_or_default();
        let (time, summary) = git.commit_info(commit)?;
        let last = LastBackup {
            time: crate::log::timestamp(UNIX_EPOCH + Duration::from_secs(time)),
            summary,
            count: git.count_commits(commit)?,
            files: {
                let tree = git.read_tree(commit)?;
                tree.len() - usize::from(readme::in_tree(&git, &tree)?)
            },
        };
        if settings.repository.is_some() {
            not_sent = Some(match git.resolve(&repository::sent_ref(&printer_name))? {
                Some(sent) if git.is_ancestor(&sent, commit)? => {
                    git.count_between(&sent, commit)?
                }
                _ => last.count,
            });
        }
        (Some(last), prepared.changes)
    } else {
        let changes = Changes {
            added: scan
                .files
                .iter()
                .map(|f| String::from_utf8_lossy(&f.path).into_owned())
                .collect(),
            ..Changes::default()
        };
        (None, changes)
    };

    let mut followed = Vec::new();
    if !settings.follow.is_empty() {
        let state = state::load(&paths.config.join(STATE_FILE))?;
        let tip = if git.exists() {
            git.resolve(SHARED_REF)?
        } else {
            None
        };
        for path in &settings.follow {
            let status = if shared::check_path(path).is_err() {
                shared::FileStatus::NotInShared
            } else {
                shared::file_status(&git, paths, path, &state, tip.as_deref())?
            };
            followed.push((path.clone(), status));
        }
    }

    Ok(Status {
        followed,
        printer_name,
        config: paths.config.clone(),
        last_backup,
        changes,
        skipped: scan.skipped,
        repository: settings.repository,
        not_sent,
        send_problem: std::fs::read_to_string(paths.send_problem())
            .ok()
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty()),
    })
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Printer name: {}", self.printer_name)?;
        writeln!(f, "Config folder: {}", self.config.display())?;
        match &self.last_backup {
            Some(last) => {
                writeln!(f, "Last backup: {} ({})", last.time, last.summary)?;
                writeln!(f, "Backups: {}", last.count)?;
                writeln!(f, "Files in the last backup: {}", last.files)?;
                if self.changes.is_empty() {
                    writeln!(f, "Changes since the last backup: none")?;
                } else {
                    writeln!(
                        f,
                        "Changes since the last backup: {}. The next backup includes them.",
                        self.changes.summary()
                    )?;
                }
            }
            None => {
                writeln!(
                    f,
                    "Last backup: none. Run 'printerkeep backup' to make the first backup."
                )?;
                writeln!(f, "Files to back up: {}", self.changes.added.len())?;
            }
        }
        if !self.skipped.is_empty() {
            writeln!(f, "Not backed up (symlinks): {}", self.skipped.join(", "))?;
        }
        if !self.followed.is_empty() {
            writeln!(f, "Followed files (from shared):")?;
            for (path, status) in &self.followed {
                writeln!(f, "  {path}: {}", status.text())?;
            }
        }
        match &self.repository {
            Some(url) => {
                writeln!(f, "Repository: {url}")?;
                match self.not_sent {
                    Some(0) => writeln!(f, "All backups are in the repository."),
                    Some(n) => match &self.send_problem {
                        Some(problem) => writeln!(
                            f,
                            "Backups not in the repository: {n}. The last try to send them did not \
                             work:\n{problem}"
                        ),
                        None => writeln!(
                            f,
                            "Backups not in the repository: {n}. The next backup sends them."
                        ),
                    },
                    None => Ok(()),
                }
            }
            None => writeln!(
                f,
                "Repository: none. The history is on this printer only. \
                 To add one: printerkeep add-repository <url>"
            ),
        }
    }
}

/// The printer name: `printer_name` in the settings file, or the host name.
fn printer_name(settings: &Settings) -> Result<String, Error> {
    let name = match &settings.printer_name {
        Some(name) => name.clone(),
        None => host_name(),
    };
    if name == SHARED_BRANCH {
        return Err(Error::ReservedPrinterName);
    }
    if !git::is_valid_branch_name(&name)? {
        return Err(Error::InvalidPrinterName(name));
    }
    Ok(name)
}

fn host_name() -> String {
    ["/proc/sys/kernel/hostname", "/etc/hostname"]
        .iter()
        .filter_map(|file| std::fs::read_to_string(file).ok())
        .map(|name| name.trim().to_string())
        .find(|name| !name.is_empty())
        .unwrap_or_else(|| "printer".to_string())
}

fn branch(printer_name: &str) -> String {
    format!("refs/heads/{printer_name}")
}

/// What goes into a backup: the files of the config folder that the rules do not skip, and printerkeep's
/// README.md (only with a repository).
#[derive(Clone, Copy)]
struct Files<'a> {
    rules: &'a IgnoreRules,
    readme: Option<&'a [u8]>,
}

/// The files of a backup.
fn backup_scan(paths: &Paths, files: Files) -> Result<Scan, Error> {
    let mut scan = scan(paths, files.rules)?;
    scan.generated = files
        .readme
        .map(|content| (readme::README_FILE.as_bytes().to_vec(), content.to_vec()));
    Ok(scan)
}

fn scan(paths: &Paths, rules: &IgnoreRules) -> Result<Scan, Error> {
    if !paths.config.is_dir() {
        return Err(Error::NoConfigFolder(paths.config.clone()));
    }
    snapshot::scan(&paths.config, rules).map_err(|source| Error::Io {
        context: format!("Cannot read the config folder {}", paths.config.display()),
        source,
    })
}

/// Stops two runs at the same time (for example, the timer and a manual backup).
/// Also makes the data folder, readable only by this user (it can contain a token).
fn lock(paths: &Paths) -> Result<File, Error> {
    let io = |source| Error::Io {
        context: format!("Cannot use the lock file {}", paths.lock().display()),
        source,
    };
    std::fs::create_dir_all(&paths.data).map_err(io)?;
    // Change the mode only when it is wrong: each change is a write to the SD card.
    if std::fs::metadata(&paths.data)
        .map_err(io)?
        .permissions()
        .mode()
        & 0o777
        != 0o700
    {
        std::fs::set_permissions(&paths.data, std::fs::Permissions::from_mode(0o700))
            .map_err(io)?;
    }
    // Open without emptying the file: no write at each run.
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(paths.lock())
        .map_err(io)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(TryLockError::WouldBlock) => Err(Error::Locked),
        Err(TryLockError::Error(source)) => Err(io(source)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_lock_fails_while_the_first_is_held() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path());
        let first = lock(&paths).unwrap();
        assert!(matches!(lock(&paths), Err(Error::Locked)));
        drop(first);
        assert!(lock(&paths).is_ok());
    }
}
