//! The commands: settings, lock, printer name, backup, status, add-repository, restore, and the commands
//! for shared files.

use crate::backup::{self, Changes};
use crate::git::{self, Git, GitError, Identity};
use crate::ignore::{IgnoreRules, RuleError};
use crate::overrides;
use crate::paths::Paths;
use crate::repository::{self, History, RepositoryError, Sent, UrlKind};
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
    /// What happened with the repository. `None` if there is no repository.
    pub sent: Option<Result<Sent, RepositoryError>>,
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

/// Makes a backup if a file changed, and sends new backups to the repository.
pub fn backup(paths: &Paths) -> Result<BackupReport, Error> {
    let settings = settings::load(&paths.settings())?;
    let printer_name = printer_name(&settings)?;
    let _lock = lock(paths)?;
    backup_and_send(paths, &settings, &printer_name, None, true)
}

/// The backup run: a backup of the config folder, then changes from shared, then send. The caller holds
/// the lock. `message` replaces the message of the first backup. Without `send`, the backups stay on the
/// printer.
fn backup_and_send(
    paths: &Paths,
    settings: &Settings,
    printer_name: &str,
    message: Option<&str>,
    send: bool,
) -> Result<BackupReport, Error> {
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(printer_name)?;
    }
    // The files on the printer go into the history before printerkeep writes a file.
    let (outcome, skipped) = make_backup(paths, &rules, &git, printer_name, message)?;
    let shared = follow_shared(paths, settings, &rules, &git, printer_name)?;

    let sent = settings.repository.as_ref().filter(|_| send).map(|url| {
        repository::remote(url, paths)
            .and_then(|remote| repository::send(&git, &remote, printer_name))
    });
    // status has no network. It shows the problem from the last try.
    match &sent {
        Some(Err(error)) => {
            let _ = std::fs::write(paths.send_problem(), format!("{error}\n"));
        }
        Some(Ok(_)) => {
            let _ = std::fs::remove_file(paths.send_problem());
        }
        None => {}
    }
    Ok(BackupReport {
        printer_name: printer_name.to_string(),
        outcome,
        skipped,
        shared,
        sent,
    })
}

/// Makes a backup if a file changed. Returns what happened and the files that were not backed up.
fn make_backup(
    paths: &Paths,
    rules: &IgnoreRules,
    git: &Git,
    printer_name: &str,
    message: Option<&str>,
) -> Result<(Outcome, Vec<String>), Error> {
    let scan = scan(paths, rules)?;
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
    rules: &IgnoreRules,
    git: &Git,
    printer_name: &str,
) -> Result<Vec<String>, Error> {
    let state_file = paths.config.join(STATE_FILE);
    let mut state = state::load(&state_file)?;
    if settings.follow.is_empty() && state.files.is_empty() {
        return Ok(Vec::new());
    }
    let mut lines = Vec::new();
    let tip = match &settings.repository {
        None => {
            lines.push(
                "printerkeep.conf has files to follow, but this printer has no repository. \
                 To add one: printerkeep add-repository <url>"
                    .to_string(),
            );
            None
        }
        Some(url) => {
            match repository::remote(url, paths).and_then(|remote| shared::fetch(git, &remote)) {
                Ok(tip) => tip,
                Err(error) => {
                    lines.push(format!(
                        "Cannot get changes from shared: {error}. printerkeep tries again at the next \
                         backup."
                    ));
                    // The last copy is still a correct shared version.
                    git.resolve(SHARED_REF)?
                }
            }
        }
    };
    if settings.repository.is_none() && tip.is_none() {
        return Ok(lines);
    }
    let events = shared::apply(git, paths, &settings.follow, tip.as_deref(), &mut state)?;
    state::save(&state_file, &state)?;
    lines.extend(events.iter().map(FileEvent::line));
    if let Some(message) = shared::message(&events) {
        make_backup(paths, rules, git, printer_name, Some(&message))?;
    }
    Ok(lines)
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
    let backup = backup_and_send(paths, &settings, &printer_name, None, true)?;
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
    lines.extend(backup_and_send(paths, &settings, &printer_name, None, true)?.lines());
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
    Ok(backup_and_send(paths, &settings, &printer_name, None, true)?.lines())
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
    let mut lines = backup_and_send(paths, &settings, &printer_name, None, true)?.lines();
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
    let mut lines = backup_and_send(paths, &settings, &printer_name, None, false)?.lines();
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
    let report = backup_and_send(paths, &settings, &printer_name, Some(&summary), true)?;
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
    let scan = scan(paths, &rules)?;

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
            files: git.read_tree(commit)?.len(),
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

    Ok(Status {
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
    std::fs::set_permissions(&paths.data, std::fs::Permissions::from_mode(0o700)).map_err(io)?;
    let file = File::create(paths.lock()).map_err(io)?;
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
