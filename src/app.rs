//! The commands: settings, lock, printer name, backup, status, add-repository and restore.

use crate::backup::{self, Changes};
use crate::git::{self, Git, GitError, Identity};
use crate::ignore::{IgnoreRules, RuleError};
use crate::paths::Paths;
use crate::repository::{self, History, RepositoryError, Sent, UrlKind};
use crate::settings::{self, Settings, SettingsError};
use crate::snapshot::{self, Scan};
use std::fmt;
use std::fs::{File, TryLockError};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
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

/// The name of the branch for shared files. No printer can have this name.
pub const SHARED_BRANCH: &str = "shared";

/// What a backup run did.
#[derive(Debug)]
pub struct BackupReport {
    pub printer_name: String,
    pub outcome: Outcome,
    /// Symlinks and files that printerkeep does not back up.
    pub skipped: Vec<String>,
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
    backup_and_send(paths, &settings, &printer_name)
}

/// The backup run. The caller holds the lock.
fn backup_and_send(
    paths: &Paths,
    settings: &Settings,
    printer_name: &str,
) -> Result<BackupReport, Error> {
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(printer_name)?;
    }
    let scan = scan(paths, &rules)?;
    let reference = branch(printer_name);
    let prepared = backup::prepare(&git, &scan, &reference, true)?;

    let outcome = if prepared.parent.is_none() && prepared.tree.is_empty() {
        Outcome::NoFiles
    } else {
        let identity = Identity {
            name: "printerkeep".to_string(),
            email: format!("printerkeep@{printer_name}"),
        };
        match backup::commit(&git, &prepared, &reference, &paths.index(), &identity)? {
            Some((commit, message)) => Outcome::Made {
                commit,
                summary: message.lines().next().unwrap_or_default().to_string(),
            },
            None => Outcome::NoChange,
        }
    };

    let sent = settings.repository.as_ref().map(|url| {
        repository::remote(url, paths)
            .and_then(|remote| repository::send(&git, &remote, printer_name))
    });
    Ok(BackupReport {
        printer_name: printer_name.to_string(),
        outcome,
        skipped: scan.skipped,
        sent,
    })
}

/// What `add-repository` did.
#[derive(Debug)]
pub struct AddReport {
    pub url: String,
    pub printer_name: String,
    pub history: History,
    pub backup: BackupReport,
}

impl AddReport {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Connected to the repository {}.", self.url)];
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

/// Connects the printer to a repository. `token` is called only for an https:// URL.
pub fn add_repository(
    paths: &Paths,
    url: &str,
    token: &dyn Fn() -> std::io::Result<String>,
) -> Result<AddReport, Error> {
    let settings = settings::load(&paths.settings())?;
    let printer_name = printer_name(&settings)?;
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;

    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(&printer_name)?;
    }
    let remote = login(paths, url, kind, &printer_name, token)?;
    let remote_commit = connect(&git, &remote, kind, paths, &printer_name)?;

    let history = match remote_commit {
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
    let backup = backup_and_send(paths, &settings, &printer_name)?;
    Ok(AddReport {
        url: url.to_string(),
        printer_name,
        history,
        backup,
    })
}

/// Stores the login for `url`. Asks for the token, or makes the SSH key.
fn login(
    paths: &Paths,
    url: &str,
    kind: UrlKind,
    printer_name: &str,
    token: &dyn Fn() -> std::io::Result<String>,
) -> Result<git::Remote, Error> {
    match kind {
        UrlKind::Https => {
            let token = token()
                .map_err(|source| Error::Io {
                    context: "Cannot read the token".into(),
                    source,
                })?
                .trim()
                .to_string();
            if token.is_empty() {
                return Err(Error::NoToken);
            }
            repository::store_token(paths, url, &token)?;
        }
        UrlKind::Ssh => {
            repository::ensure_ssh_key(paths, printer_name)?;
        }
        UrlKind::Folder => {}
    }
    Ok(repository::remote(url, paths)?)
}

/// Tests the connection and returns the repository's commit for this printer, if there is one.
fn connect(
    git: &Git,
    remote: &git::Remote,
    kind: UrlKind,
    paths: &Paths,
    printer_name: &str,
) -> Result<Option<String>, Error> {
    match git.remote_branch(remote, printer_name) {
        Ok(commit) => Ok(commit),
        Err(error) => match repository::classify(error, &remote.url) {
            RepositoryError::LoginFailed { .. } if kind == UrlKind::Ssh => {
                Err(Error::DeployKeyNeeded {
                    public_key: repository::ensure_ssh_key(paths, printer_name)?,
                })
            }
            error => Err(error.into()),
        },
    }
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
    token: &dyn Fn() -> std::io::Result<String>,
) -> Result<RestoreReport, Error> {
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;
    let key_name = printer.map_or_else(host_name, str::to_string);

    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(&key_name)?;
    }
    let remote = login(paths, url, kind, &key_name, token)?;
    // Tests the connection, as in add-repository.
    connect(&git, &remote, kind, paths, &key_name)?;

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
                    Some(n) => writeln!(
                        f,
                        "Backups not in the repository: {n}. The next backup sends them."
                    ),
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
