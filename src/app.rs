//! The commands: settings, lock, printer name, and the backup and status runs.

use crate::backup::{self, Changes};
use crate::git::{self, Git, GitError, Identity};
use crate::ignore::{IgnoreRules, RuleError};
use crate::paths::Paths;
use crate::settings::{self, Settings, SettingsError};
use crate::snapshot::{self, Scan};
use std::fmt;
use std::fs::{File, TryLockError};
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
        match &self.outcome {
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
        }
    }
}

/// Makes a backup if a file changed.
pub fn backup(paths: &Paths) -> Result<BackupReport, Error> {
    let settings = settings::load(&paths.settings())?;
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let printer_name = printer_name(&settings)?;
    let _lock = lock(paths)?;

    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(&printer_name)?;
    }
    let scan = scan(paths, &rules)?;
    let reference = branch(&printer_name);
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
    Ok(BackupReport {
        printer_name,
        outcome,
        skipped: scan.skipped,
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
}

#[derive(Debug)]
pub struct LastBackup {
    pub time: String,
    pub summary: String,
    pub count: u64,
    pub files: usize,
}

/// Reads the state of the printer. Changes nothing.
pub fn status(paths: &Paths) -> Result<Status, Error> {
    let settings = settings::load(&paths.settings())?;
    let rules = IgnoreRules::new(settings.ignore.iter().map(String::as_str))?;
    let printer_name = printer_name(&settings)?;
    let scan = scan(paths, &rules)?;

    let git = Git::new(&paths.git_dir);
    let reference = branch(&printer_name);
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
            Some(url) => writeln!(f, "Repository: {url}"),
            None => writeln!(f, "Repository: none. The history is on this printer only."),
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
fn lock(paths: &Paths) -> Result<File, Error> {
    let io = |source| Error::Io {
        context: format!("Cannot use the lock file {}", paths.lock().display()),
        source,
    };
    std::fs::create_dir_all(&paths.data).map_err(io)?;
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
