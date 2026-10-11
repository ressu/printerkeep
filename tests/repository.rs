//! M2: repository, sending and restore. A local bare repository stands in for GitHub.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A fake printer: a printer_data folder with a config folder.
struct Printer {
    dir: TempDir,
}

impl Printer {
    /// A printer with `printer_name` set in printerkeep.conf.
    fn new(name: &str) -> Self {
        let printer = Self::empty();
        printer.write(
            "printerkeep.conf",
            &format!("[printerkeep]\nprinter_name: {name}\n"),
        );
        printer
    }

    /// A printer with an empty config folder (like a new install, before a restore).
    fn empty() -> Self {
        let printer = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(printer.config()).unwrap();
        printer
    }

    fn config(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    fn write(&self, path: &str, content: &str) {
        let file = self.config().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    fn read(&self, path: &str) -> String {
        fs::read_to_string(self.config().join(path)).unwrap()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::cargo_bin("printerkeep").unwrap();
        command
            .arg("--printer-data")
            .arg(self.dir.path())
            .args(args);
        command
    }

    /// Runs printerkeep, expects success, returns stdout.
    fn ok(&self, args: &[&str]) -> String {
        let output = self
            .command(args)
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        String::from_utf8(output).unwrap()
    }

    fn tip(&self, name: &str) -> String {
        git(
            &self.dir.path().join("printerkeep/repo.git"),
            &["rev-parse", &format!("refs/heads/{name}")],
        )
    }
}

/// An empty bare repository.
fn repository() -> (TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("printers.git");
    std::process::Command::new("git")
        .args(["init", "--bare", "--quiet"])
        .arg(&path)
        .status()
        .unwrap();
    let url = path.to_str().unwrap().to_string();
    (dir, url)
}

fn git(git_dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .env("GIT_DIR", git_dir)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn remote_tip(url: &str, name: &str) -> String {
    git(
        Path::new(url),
        &["rev-parse", &format!("refs/heads/{name}")],
    )
}

#[test]
fn add_repository_sends_all_backups_and_stores_only_the_url() {
    let (_dir, url) = repository();
    let printer = Printer::new("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["backup"]);

    let output = printer.ok(&["add-repository", &url]);
    assert!(output.contains(&format!("Connected to the repository {url}.")));
    assert!(output.contains("The repository has no backups of printer-1."));
    // add-repository writes the URL into printerkeep.conf, so a backup follows.
    assert!(output.contains("Backup made: Changed printerkeep.conf\n"));
    assert!(output.contains("Sent 2 backups to the repository.\n"));
    assert_eq!(remote_tip(&url, "printer-1"), printer.tip("printer-1"));
    assert_eq!(
        printer.read("printerkeep.conf"),
        format!("[printerkeep]\nprinter_name: printer-1\nrepository: {url}\n")
    );
}

#[test]
fn backups_are_sent_and_no_change_sends_nothing() {
    let (_dir, url) = repository();
    let printer = Printer::new("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);

    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    assert_eq!(
        printer.ok(&["backup"]),
        "Backup made: Changed printer.cfg\nSent 1 backup to the repository.\n"
    );
    assert_eq!(remote_tip(&url, "printer-1"), printer.tip("printer-1"));

    // No change: no backup, nothing sent.
    assert_eq!(
        printer.ok(&["backup"]),
        "No files changed. No backup made.\n"
    );
    assert!(
        printer
            .ok(&["status"])
            .contains("All backups are in the repository.\n")
    );
}

#[test]
fn existing_history_in_the_repository_continues() {
    let (_dir, url) = repository();
    let first = Printer::new("printer-1");
    first.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    first.ok(&["add-repository", &url]);
    let history = remote_tip(&url, "printer-1");

    // A second install with the same printer name and its own local backups (the migration case).
    let second = Printer::new("printer-1");
    second.write("printer.cfg", "[printer]\nkinematics: cartesian\n");
    second.ok(&["backup"]);
    let local_only = second.tip("printer-1");

    let output = second.ok(&["add-repository", &url]);
    assert!(
        output.contains(
            "The repository has backups of printer-1. printerkeep continues that history."
        )
    );
    assert!(output.contains("They stay on this printer as refs/printerkeep/kept/printer-1-"));
    assert!(
        output.contains("Backup made: Changed printer.cfg"),
        "{output}"
    );

    // The repository's history is kept, and the new backup is on top of it.
    let tip = remote_tip(&url, "printer-1");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &history, &tip],
    );
    // The local-only backup is kept on the printer, not sent.
    let kept = git(
        &second.dir.path().join("printerkeep/repo.git"),
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/printerkeep/kept/",
        ],
    );
    assert_eq!(kept, local_only);
}

#[test]
fn repository_with_newer_changes_is_never_overwritten() {
    let (_dir, url) = repository();
    let first = Printer::new("printer-1");
    first.write("printer.cfg", "[printer]\n");
    first.ok(&["add-repository", &url]);

    // Another install of printer-1 sends a newer backup.
    let other = Printer::new("printer-1");
    other.write("printer.cfg", "[printer]\nmax_velocity: 300\n");
    other.ok(&["add-repository", &url]);
    let newer = remote_tip(&url, "printer-1");

    first.write("printer.cfg", "[printer]\nmax_velocity: 200\n");
    let output = first.ok(&["backup"]);
    assert!(output.starts_with("Backup made: Changed printer.cfg\n"));
    assert!(output.contains("The repository has changes that are not on this printer."));
    assert_eq!(remote_tip(&url, "printer-1"), newer);
    assert!(
        first
            .ok(&["status"])
            .contains("Backups not in the repository: 1.")
    );
}

#[test]
fn unreachable_repository_keeps_backups_for_later() {
    let (dir, url) = repository();
    let printer = Printer::new("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);

    let moved = dir.path().join("moved.git");
    fs::rename(&url, &moved).unwrap();
    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    let output = printer.ok(&["backup"]);
    assert!(output.contains("Cannot send the backups: "), "{output}");
    assert!(output.contains("printerkeep tries again at the next backup."));

    fs::rename(&moved, &url).unwrap();
    assert!(
        printer
            .ok(&["backup"])
            .contains("Sent 1 backup to the repository.")
    );
}

#[test]
fn restore_into_an_empty_config_folder() {
    let (_dir, url) = repository();
    let original = Printer::new("printer-1");
    original.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    original.write("macros/macros.cfg", "[gcode_macro START]\n");
    original.ok(&["add-repository", &url]);

    let new = Printer::empty();
    let output = new.ok(&["restore", &url]);
    assert!(output.contains("Restored 3 files from the backup of printer-1."));
    assert!(output.contains("sudo systemctl restart moonraker klipper"));
    assert_eq!(new.read("printer.cfg"), "[printer]\nkinematics: corexy\n");
    assert_eq!(new.read("macros/macros.cfg"), "[gcode_macro START]\n");

    // The restored printer continues the same history.
    assert_eq!(new.ok(&["backup"]), "No files changed. No backup made.\n");
    new.write("printer.cfg", "[printer]\nkinematics: cartesian\n");
    assert!(
        new.ok(&["backup"])
            .contains("Sent 1 backup to the repository.")
    );
    git(
        Path::new(&url),
        &[
            "merge-base",
            "--is-ancestor",
            &original.tip("printer-1"),
            &remote_tip(&url, "printer-1"),
        ],
    );
}

#[test]
fn restore_replaces_different_files_only_with_overwrite() {
    let (_dir, url) = repository();
    let original = Printer::new("printer-1");
    original.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    original.ok(&["add-repository", &url]);

    // A new install has its own default printer.cfg and an extra file.
    let new = Printer::empty();
    new.write("printer.cfg", "# default\n");
    new.write("extra.cfg", "# keep me\n");

    new.command(&["restore", &url])
        .assert()
        .failure()
        .stderr(contains(
            "These files in the config folder are different from the backup: printer.cfg.",
        ))
        .stderr(contains("add --overwrite"));
    assert_eq!(new.read("printer.cfg"), "# default\n");

    new.ok(&["restore", &url, "--overwrite"]);
    assert_eq!(new.read("printer.cfg"), "[printer]\nkinematics: corexy\n");
    // Restore never removes files.
    assert_eq!(new.read("extra.cfg"), "# keep me\n");
}

#[test]
fn restore_asks_which_printer_when_there_are_more() {
    let (_dir, url) = repository();
    for name in ["printer-1", "printer-2"] {
        let printer = Printer::new(name);
        printer.write("printer.cfg", &format!("# {name}\n"));
        printer.ok(&["add-repository", &url]);
    }

    let new = Printer::empty();
    new.command(&["restore", &url])
        .assert()
        .failure()
        .stderr(contains(
            "backups of more printers: printer-1, printer-2. Select one",
        ));
    new.command(&["restore", &url, "printer-3"])
        .assert()
        .failure()
        .stderr(contains("no printer named 'printer-3'"));
    new.ok(&["restore", &url, "printer-2"]);
    assert_eq!(new.read("printer.cfg"), "# printer-2\n");
    assert!(
        new.read("printerkeep.conf")
            .contains("printer_name: printer-2\n")
    );
}

#[test]
fn token_in_the_url_is_refused() {
    let printer = Printer::new("printer-1");
    printer
        .command(&[
            "add-repository",
            "https://me:github_pat_secret@github.com/me/printers.git",
        ])
        .assert()
        .failure()
        .stderr(contains("Do not put the token in the URL"));
    assert_eq!(
        printer.read("printerkeep.conf"),
        "[printerkeep]\nprinter_name: printer-1\n"
    );
}

#[test]
fn token_from_standard_input_is_stored_privately() {
    let printer = Printer::new("printer-1");
    // Nothing listens on port 9, so the connection fails after the token is stored.
    printer
        .command(&["add-repository", "https://127.0.0.1:9/me/printers.git"])
        .write_stdin("github_pat_test\n")
        .assert()
        .failure()
        .stderr(contains("Cannot connect to the repository"));

    let credentials = printer.dir.path().join("printerkeep/credentials");
    assert_eq!(
        fs::read_to_string(&credentials).unwrap(),
        "https://printerkeep:github_pat_test@127.0.0.1:9\n"
    );
    assert_eq!(
        fs::metadata(&credentials).unwrap().permissions().mode() & 0o777,
        0o600
    );
    // Without a connection, printerkeep.conf does not change.
    assert_eq!(
        printer.read("printerkeep.conf"),
        "[printerkeep]\nprinter_name: printer-1\n"
    );
}

#[test]
fn ssh_url_makes_a_key() {
    let printer = Printer::new("printer-1");
    printer
        .command(&["add-repository", "ssh://git@127.0.0.1:9/me/printers.git"])
        .assert()
        .failure();
    let key = printer.dir.path().join("printerkeep/id_ed25519.pub");
    assert!(fs::read_to_string(key).unwrap().starts_with("ssh-ed25519 "));
}
