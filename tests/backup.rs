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

    /// Runs `printerkeep <command>`, expects success, returns stdout.
    fn run(&self, command: &str) -> String {
        let output = self
            .printerkeep(command)
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
    // A run with no change is not in the log: it writes nothing to the SD card.
    assert!(!log.contains("No files changed"));
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

#[test]
fn skipped_symlinks_are_shown_only_when_the_list_changes() {
    let printer = Printer::new();
    printer.write("printer.cfg", "[printer]\n");
    std::os::unix::fs::symlink("/home/pi/KAMP", printer.config().join("KAMP")).unwrap();
    printer.backup();

    // The same links at the next backup: neither the terminal nor the log names them again.
    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    assert_eq!(printer.backup(), "Backup made: Changed printer.cfg\n");
    assert_eq!(printer.log().matches("Not backed up (symlinks)").count(), 1);

    // A new link: the terminal and the log get the new list.
    std::os::unix::fs::symlink("/home/pi/ShakeTune", printer.config().join("K-ShakeTune")).unwrap();
    printer.write("printer.cfg", "[printer]\nkinematics: cartesian\n");
    assert!(
        printer
            .backup()
            .contains("Not backed up (symlinks): K-ShakeTune, KAMP\n")
    );
    assert!(
        printer
            .log()
            .contains("Not backed up (symlinks): K-ShakeTune, KAMP\n")
    );
    assert_eq!(printer.log().matches("Not backed up (symlinks)").count(), 2);
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
        .stderr(predicates::str::contains("'shared' is reserved"));
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
            "doesn't exist. To use another printer_data folder, add --printer-data",
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
        before.contains("Last backup: none yet. To make the first one, run: printerkeep backup\n")
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

/// printer.cfg with a probe offset and a bed mesh in the SAVE_CONFIG block.
fn printer_cfg(z_offset: &str, mesh: &str) -> String {
    format!(
        "[printer]\nkinematics: corexy\n\n{SAVE_CONFIG}#*# [probe]\n#*# z_offset = {z_offset}\n#*#\n\
         #*# [bed_mesh default]\n#*# version = 1\n#*# points =\n#*# \t{mesh}, 0.050\n#*# x_count = 2\n"
    )
}

const WAITING: &str = "No backup made: printer.cfg has only bundled changes. \
                       They go into the next backup with other changes.\n";

#[test]
fn bed_mesh_change_alone_waits_for_the_next_backup() {
    let printer = Printer::new();
    printer.write("printer.cfg", &printer_cfg("1.000", "0.100"));
    printer.write("macros.cfg", "[gcode_macro START]\ngcode:\n  G28\n");
    printer.backup();

    printer.write("printer.cfg", &printer_cfg("1.000", "0.125"));
    assert_eq!(printer.run("sync"), WAITING);
    assert_eq!(printer.commits(), 1);
    // A run without a backup writes nothing to the log.
    assert!(!printer.log().contains("bundled"));
    assert!(printer.run("status").contains(
        "Changes since the last backup: printer.cfg has only bundled changes. \
             They go into the next backup with other changes.\n"
    ));

    printer.write("macros.cfg", "[gcode_macro START]\ngcode:\n  G28\n  G90\n");
    assert_eq!(printer.run("sync"), "Backup made: Changed 2 files\n");
    assert_eq!(printer.commits(), 2);
    assert!(
        printer
            .git(&["show", "refs/heads/printer-1:printer.cfg"])
            .contains("0.125, 0.050")
    );
}

#[test]
fn backup_by_hand_includes_bundled_changes() {
    let printer = Printer::new();
    printer.write("printer.cfg", &printer_cfg("1.000", "0.100"));
    printer.backup();

    printer.write("printer.cfg", &printer_cfg("1.000", "0.125"));
    assert_eq!(
        printer.backup(),
        "Backup made: SAVE_CONFIG changed printer.cfg\n"
    );
    assert_eq!(printer.commits(), 2);
}

#[test]
fn probe_offset_change_makes_a_backup() {
    let printer = Printer::new();
    printer.write("printer.cfg", &printer_cfg("1.000", "0.100"));
    printer.backup();

    printer.write("printer.cfg", &printer_cfg("1.125", "0.125"));
    assert_eq!(
        printer.run("sync"),
        "Backup made: SAVE_CONFIG changed printer.cfg\n"
    );
}

#[test]
fn empty_bundled_changes_backs_up_each_change() {
    let printer = Printer::new();
    printer.write(
        "printerkeep.conf",
        "[printerkeep]\nprinter_name: printer-1\nbundled_changes:\n",
    );
    printer.write("printer.cfg", &printer_cfg("1.000", "0.100"));
    printer.backup();

    printer.write("printer.cfg", &printer_cfg("1.000", "0.125"));
    assert_eq!(
        printer.run("sync"),
        "Backup made: SAVE_CONFIG changed printer.cfg\n"
    );
}

#[test]
fn bundled_files_from_the_settings_file() {
    let printer = Printer::new();
    printer.write(
        "printerkeep.conf",
        "[printerkeep]\nprinter_name: printer-1\nbundled_changes:\n  variables.cfg\n",
    );
    printer.write("printer.cfg", &printer_cfg("1.000", "0.100"));
    printer.backup();

    // A new file is never bundled.
    printer.write("variables.cfg", "[Variables]\nlast_tool = 0\n");
    assert_eq!(printer.run("sync"), "Backup made: Added variables.cfg\n");

    printer.write("variables.cfg", "[Variables]\nlast_tool = 1\n");
    assert_eq!(
        printer.run("sync"),
        "No backup made: variables.cfg has only bundled changes. \
         They go into the next backup with other changes.\n"
    );

    // The list replaces the default: the bed mesh isn't bundled now.
    printer.write("printer.cfg", &printer_cfg("1.000", "0.125"));
    assert_eq!(printer.run("sync"), "Backup made: Changed 2 files\n");
}
