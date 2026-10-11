//! M1: local backups, tested with the real binary and real git in temporary folders.

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A fake printer: a printer_data folder with a config folder.
struct Printer {
    dir: TempDir,
}

impl Printer {
    fn new() -> Self {
        let printer = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(printer.config()).unwrap();
        printer.write(
            "printerkeep.conf",
            "[printerkeep]\nprinter_name: printer-1\n",
        );
        printer
    }

    fn printer_data(&self) -> &Path {
        self.dir.path()
    }

    fn config(&self) -> PathBuf {
        self.printer_data().join("config")
    }

    fn write(&self, path: &str, content: &str) {
        let file = self.config().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    fn printerkeep(&self, command: &str) -> assert_cmd::assert::Assert {
        Command::cargo_bin("printerkeep")
            .unwrap()
            .arg("--printer-data")
            .arg(self.printer_data())
            .arg(command)
            .assert()
    }

    /// Runs `printerkeep backup`, expects success, returns stdout.
    fn backup(&self) -> String {
        let output = self
            .printerkeep("backup")
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(output).unwrap()
    }

    fn git(&self, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .env("GIT_DIR", self.printer_data().join("printerkeep/repo.git"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn commits(&self) -> usize {
        self.git(&["rev-list", "--count", "refs/heads/printer-1"])
            .trim()
            .parse()
            .unwrap()
    }

    fn files(&self) -> Vec<String> {
        self.git(&["ls-tree", "-r", "--name-only", "refs/heads/printer-1"])
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn subject(&self) -> String {
        self.git(&["show", "-s", "--format=%s", "refs/heads/printer-1"])
            .trim()
            .to_string()
    }

    fn log(&self) -> String {
        fs::read_to_string(self.printer_data().join("logs/printerkeep.log")).unwrap()
    }
}

#[test]
fn first_backup_then_no_backup_without_a_change() {
    let printer = Printer::new();
    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    printer.write("macros/macros.cfg", "[gcode_macro START]\ngcode:\n  G28\n");

    assert_eq!(printer.backup(), "Backup made: First backup: 3 files\n");
    assert_eq!(printer.commits(), 1);
    assert_eq!(
        printer.files(),
        ["macros/macros.cfg", "printer.cfg", "printerkeep.conf"]
    );

    assert_eq!(printer.backup(), "No files changed. No backup made.\n");
    assert_eq!(printer.commits(), 1);

    let log = printer.log();
    assert!(log.contains(" UTC Backup made: First backup: 3 files\n"));
    assert!(log.contains(" UTC No files changed. No backup made.\n"));
}

#[test]
fn line_ending_change_alone_makes_no_backup() {
    let printer = Printer::new();
    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    printer.backup();

    printer.write("printer.cfg", "[printer]\r\nkinematics: corexy\r\n");
    assert_eq!(printer.backup(), "No files changed. No backup made.\n");
    assert_eq!(printer.commits(), 1);

    printer.write("printer.cfg", "[printer]\r\nkinematics: cartesian\r\n");
    assert_eq!(printer.backup(), "Backup made: Changed printer.cfg\n");
    assert_eq!(printer.commits(), 2);
    // The new backup stores the file exactly as it is on disk.
    assert_eq!(
        printer.git(&["show", "refs/heads/printer-1:printer.cfg"]),
        "[printer]\r\nkinematics: cartesian\r\n"
    );
}

#[test]
fn ignored_files_and_symlinks_are_not_backed_up() {
    let printer = Printer::new();
    printer.write(
        "printerkeep.conf",
        "[printerkeep]\nprinter_name: printer-1\nignore:\n  timelapse/\n",
    );
    printer.write("printer.cfg", "[printer]\n");
    printer.write("printer-20261011_120000.cfg", "[printer]\n");
    printer.write(".moonraker.conf.bkp", "[server]\n");
    printer.write("moonraker.secrets", "[api]\ntoken: x\n");
    printer.write("timelapse/settings.cfg", "x\n");
    printer.write("Klipper-Macros/.git/HEAD", "ref: refs/heads/main\n");
    printer.write("Klipper-Macros/macros.cfg", "[gcode_macro A]\n");
    std::os::unix::fs::symlink("/home/pi/KAMP", printer.config().join("KAMP")).unwrap();

    assert_eq!(
        printer.backup(),
        "Backup made: First backup: 3 files\nNot backed up (symlinks): KAMP\n"
    );
    assert_eq!(
        printer.files(),
        [
            "Klipper-Macros/macros.cfg",
            "printer.cfg",
            "printerkeep.conf"
        ]
    );
}

const SAVE_CONFIG: &str = "#*# <---------------------- SAVE_CONFIG ---------------------->\n\
                           #*# DO NOT EDIT THIS BLOCK OR BELOW. The contents are auto-generated.\n\
                           #*#\n";

#[test]
fn save_config_change_has_its_own_message() {
    let printer = Printer::new();
    printer.write(
        "printer.cfg",
        &format!("[printer]\n\n{SAVE_CONFIG}#*# [probe]\n#*# z_offset = 1.000\n"),
    );
    printer.backup();

    printer.write(
        "printer.cfg",
        &format!("[printer]\n\n{SAVE_CONFIG}#*# [probe]\n#*# z_offset = 1.125\n"),
    );
    assert_eq!(
        printer.backup(),
        "Backup made: SAVE_CONFIG changed printer.cfg\n"
    );
    let message = printer.git(&["show", "-s", "--format=%B", "refs/heads/printer-1"]);
    assert!(message.contains("Changed:\n  printer.cfg (SAVE_CONFIG)\n"));
}

#[test]
fn added_and_removed_files() {
    let printer = Printer::new();
    printer.write("old.cfg", "x\n");
    printer.backup();

    fs::remove_file(printer.config().join("old.cfg")).unwrap();
    printer.write("new.cfg", "y\n");
    assert_eq!(
        printer.backup(),
        "Backup made: Added new.cfg, removed old.cfg\n"
    );
    assert_eq!(printer.subject(), "Added new.cfg, removed old.cfg");
}

#[test]
fn settings_error_shows_the_line_and_goes_to_the_log() {
    let printer = Printer::new();
    printer.write("printerkeep.conf", "[printerkeep]\nfolow: a.cfg\n");
    printer
        .printerkeep("backup")
        .failure()
        .stderr(predicates::str::contains(
            "printerkeep.conf, line 2: unknown option 'folow'.",
        ));
    assert!(printer.log().contains("Error: "));
}

#[test]
fn printer_name_shared_is_not_allowed() {
    let printer = Printer::new();
    printer.write("printerkeep.conf", "[printerkeep]\nprinter_name: shared\n");
    printer
        .printerkeep("backup")
        .failure()
        .stderr(predicates::str::contains("'shared' is not available"));
}

#[test]
fn missing_config_folder_is_a_clear_error() {
    let dir = tempfile::tempdir().unwrap();
    Command::cargo_bin("printerkeep")
        .unwrap()
        .arg("--printer-data")
        .arg(dir.path())
        .arg("backup")
        .assert()
        .failure()
        .stderr(predicates::str::contains(
            "does not exist. Use --printer-data",
        ));
}

#[test]
fn status_before_and_after_backups() {
    let printer = Printer::new();
    printer.write("printer.cfg", "[printer]\n");

    let before = printer
        .printerkeep("status")
        .success()
        .get_output()
        .stdout
        .clone();
    let before = String::from_utf8(before).unwrap();
    assert!(before.contains("Printer name: printer-1\n"));
    assert!(
        before.contains("Last backup: none. Run 'printerkeep backup' to make the first backup.\n")
    );
    assert!(before.contains("Files to back up: 2\n"));
    assert!(before.contains("Repository: none."));

    printer.backup();
    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    let after = printer
        .printerkeep("status")
        .success()
        .get_output()
        .stdout
        .clone();
    let after = String::from_utf8(after).unwrap();
    assert!(after.contains(" UTC (First backup: 2 files)\n"), "{after}");
    assert!(after.contains("Backups: 1\n"));
    assert!(after.contains("Changes since the last backup: Changed printer.cfg."));
    // status changes nothing.
    assert_eq!(printer.commits(), 1);
}
