//! The files and folders that printerkeep uses on the printer.

use std::path::PathBuf;

/// The name of the settings file in the config folder.
pub const SETTINGS_FILE: &str = "printerkeep.conf";

/// All paths, derived from one `printer_data` folder.
#[derive(Debug, Clone)]
pub struct Paths {
    /// The `printer_data` folder (usually `~/printer_data`).
    pub printer_data: PathBuf,
    /// The config folder: everything in it is backed up.
    pub config: PathBuf,
    /// printerkeep's own data: git data, lock, keys. Outside the config folder.
    pub data: PathBuf,
    /// The bare git repository with the history.
    pub git_dir: PathBuf,
    /// The log file. Fluidd and Mainsail show the logs folder.
    pub log: PathBuf,
}

impl Paths {
    pub fn new(printer_data: impl Into<PathBuf>) -> Self {
        let printer_data = printer_data.into();
        let data = printer_data.join("printerkeep");
        Self {
            config: printer_data.join("config"),
            git_dir: data.join("repo.git"),
            log: printer_data.join("logs").join("printerkeep.log"),
            data,
            printer_data,
        }
    }

    /// `printerkeep.conf` in the config folder.
    pub fn settings(&self) -> PathBuf {
        self.config.join(SETTINGS_FILE)
    }

    /// The lock file that stops two runs at the same time.
    pub fn lock(&self) -> PathBuf {
        self.data.join("lock")
    }

    /// Why the last try to send backups did not work. Removed when a send works.
    pub fn send_problem(&self) -> PathBuf {
        self.data.join("send-problem")
    }

    /// The symlinks that the last backup skipped. The log gets the list only when it changes.
    pub fn skipped(&self) -> PathBuf {
        self.data.join("skipped")
    }

    /// A temporary git index, used to build trees. Never in the config folder.
    pub fn index(&self) -> PathBuf {
        self.data.join("index")
    }
}
