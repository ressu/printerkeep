//! The commands. This module has the errors and what all commands use (settings, printer name, lock, the
//! files of a backup). The commands are in:
//!
//! - `run`: the backup run (`backup`; the other commands run it too),
//! - `connection`: add-repository, change-token, restore, rename,
//! - `shared_files`: follow, stop-following, copy-to-shared, decide,
//! - `status`.
//!
//! The command modules `use super::*`: the imports below are also theirs.

use crate::backup::{self, Changes};
use crate::branch_readme;
use crate::git::{self, Git, GitError, Identity};
use crate::ignore::{IgnoreRules, RuleError};
use crate::overrides;
use crate::paths::Paths;
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

mod connection;
mod run;
mod shared_files;
mod status;

pub use connection::{
    AddReport, Ask, AskToken, Connect, RestoreReport, TokenRequest, TokenUse, add_repository,
    change_token, rename, restore,
};
pub use run::{BackupReport, Outcome, backup};
use run::{COMMAND, Run, backup_and_send};
pub use shared_files::{Confirm, copy_to_shared, decide, follow, stop_following};
pub use status::{LastBackup, Status, status};

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
        "The repository did not accept the token, or the URL is wrong ({url}). The stored login did not \
         change. Check that the token has access to this repository, with the permission \"Contents: Read and \
         write\", and that it did not expire. Then try again."
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
    #[error(
        "The branch {name} in the repository has backups with no common history with this printer's \
         backups. printerkeep did not continue it, and changed nothing. To continue that branch on this \
         printer: printerkeep add-repository {url} --printer-name {name} --continue. For a different branch: \
         printerkeep add-repository {url} --printer-name <another name>"
    )]
    NameInUse { name: String, url: String },
    #[error(
        "The repository has printers ({}), and this printer has the default name main. printerkeep does not \
         guess which printer this is. If it is one of them: printerkeep add-repository {url} --printer-name \
         <its name>. For a new printer: printerkeep add-repository {url} --printer-name <new name>",
        .printers.join(", ")
    )]
    WhichPrinter { url: String, printers: Vec<String> },
    #[error(
        "The repository already has a branch named '{name}' (from another printer, or from klipper-backup). \
         rename never takes over a branch in the repository. Select a different name. Or, to make this \
         printer continue that branch: printerkeep add-repository {url} --printer-name {name} --continue"
    )]
    NameTaken { name: String, url: String },
    #[error(
        "The repository has no backups of a printer. Check the URL. To send backups to it, run \
         printerkeep add-repository <url> on the printer that has them."
    )]
    NothingToRestore,
    #[error(
        "The repository has backups of several printers: {}. Select one: printerkeep restore <url> --printer-name <printer name>",
        .0.join(", ")
    )]
    SelectPrinter(Vec<String>),
    #[error(
        "The repository has no printer named '{name}'. Printers in the repository: {}. Use one of them: \
         printerkeep restore <url> --printer-name <printer name>",
        .available.join(", ")
    )]
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

fn identity(printer_name: &str) -> Identity {
    Identity {
        name: "printerkeep".to_string(),
        email: format!("printerkeep@{printer_name}"),
    }
}

fn count(n: usize, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
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

/// The printer name when `printer_name` is not set: the usual name of the first branch of a repository. It
/// never comes from the host name, which is often an image default (`mainsail`, `raspberrypi`) or set by DHCP.
pub const DEFAULT_NAME: &str = "main";

/// The printer name: `printer_name` in the settings file, or `main`.
fn printer_name(settings: &Settings) -> Result<String, Error> {
    let name = settings
        .printer_name
        .clone()
        .unwrap_or_else(|| DEFAULT_NAME.to_string());
    check_name(&name)?;
    Ok(name)
}

/// The settings and the printer name.
fn load(paths: &Paths) -> Result<(Settings, String), Error> {
    let settings = settings::load(&paths.settings())?;
    let name = printer_name(&settings)?;
    Ok((settings, name))
}

/// printerkeep's git data. Makes it, with HEAD on the printer's branch, if it does not exist.
fn open_git(paths: &Paths, printer_name: &str) -> Result<Git, Error> {
    let git = Git::new(&paths.git_dir);
    if !git.exists() {
        git.init(printer_name)?;
    }
    Ok(git)
}

fn check_name(name: &str) -> Result<(), Error> {
    if name == SHARED_BRANCH {
        return Err(Error::ReservedPrinterName);
    }
    if !git::is_valid_branch_name(name)? {
        return Err(Error::InvalidPrinterName(name.to_string()));
    }
    Ok(())
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
    scan.generated = files.readme.map(|content| {
        (
            branch_readme::README_FILE.as_bytes().to_vec(),
            content.to_vec(),
        )
    });
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
