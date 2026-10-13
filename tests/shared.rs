//! M5: shared files on more printers. A local bare repository stands in for GitHub.

use assert_cmd::Command;
use predicates::str::contains;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const MACROS: &str = "\
[gcode_macro PARK]
gcode:
    G90
    G1 X10 Y10 F6000

[gcode_macro LOAD]
variable_temp: 230
gcode:
    M109 S{printer['gcode_macro LOAD'].temp}
    G1 E50 F300
";

struct Printer {
    dir: TempDir,
}

impl Printer {
    /// A printer connected to the repository `url`, with a first backup.
    fn new(name: &str, url: &str) -> Self {
        let printer = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(printer.config()).unwrap();
        printer.write(
            "printerkeep.conf",
            &format!("[printerkeep]\nprinter_name: {name}\n"),
        );
        printer.write("printer.cfg", &format!("[printer]\n# {name}\n"));
        printer.ok(&["add-repository", url]);
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
            .env_remove("XDG_RUNTIME_DIR")
            .arg("--printer-data")
            .arg(self.dir.path())
            .args(args);
        command
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.command(args).assert().success().get_output().clone();
        String::from_utf8(output.stdout).unwrap()
    }

    fn fails(&self, args: &[&str]) -> String {
        let output = self.command(args).assert().failure().get_output().clone();
        String::from_utf8(output.stderr).unwrap()
    }

    /// The subject of the last backup.
    fn last_backup(&self, name: &str) -> String {
        git(
            &self.dir.path().join("printerkeep/repo.git"),
            &["log", "-1", "--format=%s", name],
        )
    }
}

fn repository() -> (TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("printers.git");
    git(
        Path::new("."),
        &["init", "--bare", "--quiet", path.to_str().unwrap()],
    );
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

/// The content of a file on the shared branch.
fn shared(url: &str, path: &str) -> String {
    git(Path::new(url), &["show", &format!("shared:{path}")]) + "\n"
}

#[test]
fn copy_to_shared_and_follow_keep_local_changes() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    let printer3 = Printer::new("printer-3", &url);

    // M8: printer 1's version becomes the shared version.
    printer1.write("macros/macros.cfg", MACROS);
    let output = printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    assert!(
        output.contains("Copied macros/macros.cfg to shared. Other printers can now follow it."),
        "{output}"
    );
    assert_eq!(shared(&url, "macros/macros.cfg"), MACROS);
    assert!(
        printer1
            .read("printerkeep.conf")
            .contains("follow:\n  macros/macros.cfg\n")
    );
    assert!(
        printer1
            .read(".printerkeep-state.json")
            .contains("\"macros/macros.cfg\"")
    );
    assert_eq!(
        printer1.last_backup("printer-1"),
        "Copied macros/macros.cfg to shared"
    );

    // Printer 2 has a different version: it stays, as local changes.
    let printer2_macros = MACROS.replace("variable_temp: 230", "variable_temp: 250");
    printer2.write("macros/macros.cfg", &printer2_macros);
    let output = printer2.ok(&["follow", "macros/macros.cfg"]);
    assert!(
        output.contains("Follows macros/macros.cfg. The differences"),
        "{output}"
    );
    assert_eq!(printer2.read("macros/macros.cfg"), printer2_macros);

    // Printer 3 does not have the file: it gets the shared version.
    let output = printer3.ok(&["follow", "macros/macros.cfg"]);
    assert!(output.contains("Wrote the shared version."), "{output}");
    assert_eq!(printer3.read("macros/macros.cfg"), MACROS);

    // M1, M4: printer 1 improves PARK. Printer 2 gets it and keeps its temperature.
    let improved = MACROS.replace("G1 X10 Y10 F6000", "G1 X10 Y10 F9000");
    printer1.write("macros/macros.cfg", &improved);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    let output = printer2.ok(&["backup"]);
    assert!(
        output
            .contains("macros/macros.cfg: wrote the changes from shared. The local changes stay."),
        "{output}"
    );
    assert_eq!(
        printer2.read("macros/macros.cfg"),
        improved.replace("variable_temp: 230", "variable_temp: 250")
    );
    assert_eq!(
        printer2.last_backup("printer-2"),
        "Changes from shared: macros/macros.cfg"
    );
    // Printer 3 has no local changes: it gets the shared version.
    let output = printer3.ok(&["backup"]);
    assert!(
        output.contains("macros/macros.cfg: wrote the changes from shared."),
        "{output}"
    );
    assert_eq!(printer3.read("macros/macros.cfg"), improved);

    // Nothing new: no backup, no change.
    let output = printer2.ok(&["backup"]);
    assert!(
        output.contains("No files changed. No backup made."),
        "{output}"
    );
    assert!(!output.contains("macros/macros.cfg"), "{output}");
}

#[test]
fn copy_to_shared_first_takes_the_changes_of_other_printers() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["follow", "macros/macros.cfg"]);

    // Both change different lines. Printer 2 copies after printer 1: printer 1's change must stay.
    printer1.write(
        "macros/macros.cfg",
        &MACROS.replace("G1 E50 F300", "G1 E60 F300"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.write("macros/macros.cfg", &MACROS.replace("G90", "G90\n    G28"));
    printer2.ok(&["copy-to-shared", "macros/macros.cfg"]);
    let both = MACROS
        .replace("G1 E50 F300", "G1 E60 F300")
        .replace("G90", "G90\n    G28");
    assert_eq!(shared(&url, "macros/macros.cfg"), both);
    assert_eq!(printer2.read("macros/macros.cfg"), both);
}

#[test]
fn needs_decision_waits_for_the_user() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["follow", "macros/macros.cfg"]);

    // M9: both change the same line.
    printer2.write(
        "macros/macros.cfg",
        &MACROS.replace("variable_temp: 230", "variable_temp: 250"),
    );
    printer1.write(
        "macros/macros.cfg",
        &MACROS.replace("variable_temp: 230", "variable_temp: 240"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    let output = printer2.ok(&["backup"]);
    assert!(
        output.contains(
            "macros/macros.cfg: needs decision. Open the file and look for the #### lines."
        ),
        "{output}"
    );
    let with_block = printer2.read("macros/macros.cfg");
    assert!(
        with_block.contains(
            "#### Active now (this printer):\nvariable_temp: 250\n\
             #### Shared version. To use it, remove the\n\
             #### # from the next line and delete the\n\
             #### line above:\n#variable_temp: 240\n"
        ),
        "{with_block}"
    );
    assert_eq!(
        printer2.last_backup("printer-2"),
        "Needs decision: macros/macros.cfg"
    );
    // The backup before it has printer 2's own version.
    assert!(
        git(
            &printer2.dir.path().join("printerkeep/repo.git"),
            &["show", "printer-2~1:macros/macros.cfg"]
        )
        .contains("variable_temp: 250")
    );

    // printerkeep does not copy a file with a block, and does not change the file again.
    let error = printer2.fails(&["copy-to-shared", "macros/macros.cfg"]);
    assert!(error.contains("needs decision"), "{error}");
    printer1.write(
        "macros/macros.cfg",
        &MACROS
            .replace("variable_temp: 230", "variable_temp: 240")
            .replace("G1 E50 F300", "G1 E60 F300"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    let output = printer2.ok(&["backup"]);
    assert!(output.contains("needs decision"), "{output}");
    assert_eq!(printer2.read("macros/macros.cfg"), with_block);

    // The user uses the shared line and deletes the #### lines. The waiting change then comes in.
    let decided: String = with_block
        .lines()
        .filter(|l| !l.starts_with("####") && *l != "variable_temp: 250")
        .map(|l| l.strip_prefix('#').unwrap_or(l))
        .map(|l| format!("{l}\n"))
        .collect();
    assert_eq!(
        decided,
        MACROS.replace("variable_temp: 230", "variable_temp: 240")
    );
    printer2.write("macros/macros.cfg", &decided);
    let output = printer2.ok(&["backup"]);
    assert!(
        output.contains("macros/macros.cfg: decision recorded."),
        "{output}"
    );
    assert!(
        output.contains("macros/macros.cfg: wrote the changes from shared."),
        "{output}"
    );
    assert_eq!(
        printer2.read("macros/macros.cfg"),
        MACROS
            .replace("variable_temp: 230", "variable_temp: 240")
            .replace("G1 E50 F300", "G1 E60 F300")
    );
}

#[test]
fn line_endings_of_the_printer_stay() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    let crlf = MACROS.replace('\n', "\r\n");
    printer2.write("macros/macros.cfg", &crlf);
    printer2.ok(&["follow", "macros/macros.cfg"]);

    printer1.write(
        "macros/macros.cfg",
        &MACROS.replace("G1 E50 F300", "G1 E60 F300"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["backup"]);
    assert_eq!(
        printer2.read("macros/macros.cfg"),
        crlf.replace("G1 E50 F300", "G1 E60 F300")
    );
}

#[test]
fn stop_following_keeps_the_file() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["follow", "macros/macros.cfg"]);

    let output = printer2.ok(&["stop-following", "macros/macros.cfg"]);
    assert!(
        output.contains("Stopped following macros/macros.cfg."),
        "{output}"
    );
    assert!(
        !printer2
            .read("printerkeep.conf")
            .contains("macros/macros.cfg")
    );
    assert!(!printer2.config().join(".printerkeep-state.json").exists());
    assert_eq!(printer2.read("macros/macros.cfg"), MACROS);

    printer1.write(
        "macros/macros.cfg",
        &MACROS.replace("G1 E50 F300", "G1 E60 F300"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["backup"]);
    assert_eq!(printer2.read("macros/macros.cfg"), MACROS);

    let error = printer2.fails(&["stop-following", "macros/macros.cfg"]);
    assert!(
        error.contains("printerkeep does not follow macros/macros.cfg."),
        "{error}"
    );
}

#[test]
fn follow_in_the_settings_file_works_without_a_command() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);

    // In Fluidd: add the file to the follow list in printerkeep.conf.
    let settings = printer2.read("printerkeep.conf");
    printer2.write(
        "printerkeep.conf",
        &format!("{settings}follow:\n  macros/macros.cfg\n  missing.cfg\n"),
    );
    let output = printer2.ok(&["backup"]);
    assert!(
        output.contains("Follows macros/macros.cfg. Wrote the shared version."),
        "{output}"
    );
    assert!(
        output.contains("missing.cfg: not in shared. To add it to shared: printerkeep copy-to-shared missing.cfg"),
        "{output}"
    );
    assert_eq!(printer2.read("macros/macros.cfg"), MACROS);
}

#[test]
fn shared_commands_need_a_repository_and_a_shared_file() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let error = printer1.fails(&["follow", "macros/macros.cfg"]);
    assert!(
        error.contains(
            "macros/macros.cfg is not in shared. To add it to shared: printerkeep copy-to-shared"
        ),
        "{error}"
    );
    let error = printer1.fails(&["copy-to-shared", "printerkeep.conf"]);
    assert!(
        error.contains("this file belongs to this printer only"),
        "{error}"
    );
    let error = printer1.fails(&["copy-to-shared", "../moonraker.conf"]);
    assert!(error.contains("without '..'"), "{error}");
    let error = printer1.fails(&["copy-to-shared", "nothing.cfg"]);
    assert!(
        error.contains("nothing.cfg is not in the config folder."),
        "{error}"
    );

    let alone = tempfile::tempdir().unwrap();
    fs::create_dir_all(alone.path().join("config")).unwrap();
    Command::cargo_bin("printerkeep")
        .unwrap()
        .arg("--printer-data")
        .arg(alone.path())
        .args(["follow", "macros/macros.cfg"])
        .assert()
        .failure()
        .stderr(contains("Shared files need a repository."));
}

const SENSOR: &str = "\
[filament_switch_sensor runout]
switch_pin: ^PG12   # printerkeep: override
pause_on_runout: True
runout_gcode:
    PAUSE
";

#[test]
fn override_lines_never_travel() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    let printer3 = Printer::new("printer-3", &url);

    // C6: the pin of printer 1 does not go to shared.
    printer1.write("sensor.cfg", SENSOR);
    let output = printer1.ok(&["copy-to-shared", "sensor.cfg"]);
    assert!(
        output.contains(
            "Not copied: switch_pin in [filament_switch_sensor runout] (override). Shared has the value \
             CHANGE_ME there."
        ),
        "{output}"
    );
    let in_shared = SENSOR.replace("^PG12", "CHANGE_ME");
    assert_eq!(shared(&url, "sensor.cfg"), in_shared);
    assert_eq!(printer1.read("sensor.cfg"), SENSOR);

    // A printer without the file gets the placeholder, and a clear line.
    let output = printer3.ok(&["follow", "sensor.cfg"]);
    assert!(
        output.contains(
            "sensor.cfg: 1 override line has the value CHANGE_ME. Set the values for this printer."
        ),
        "{output}"
    );
    assert_eq!(printer3.read("sensor.cfg"), in_shared);
    printer3.write("sensor.cfg", &SENSOR.replace("^PG12", "^PA3"));
    printer3.ok(&["backup"]);

    let printer2_sensor = SENSOR.replace("^PG12", "^PA1");
    printer2.write("sensor.cfg", &printer2_sensor);
    printer2.ok(&["follow", "sensor.cfg"]);

    // Printer 2 improves the runout macro. Only the macro goes to shared, and to the other printers.
    let improved = printer2_sensor.replace("    PAUSE\n", "    PAUSE\n    M117 Runout\n");
    printer2.write("sensor.cfg", &improved);
    let output = printer2.ok(&["copy-to-shared", "sensor.cfg"]);
    assert!(
        output.contains(
            "Not copied: switch_pin in [filament_switch_sensor runout] (override). Shared keeps its own line."
        ),
        "{output}"
    );
    assert_eq!(
        shared(&url, "sensor.cfg"),
        in_shared.replace("    PAUSE\n", "    PAUSE\n    M117 Runout\n")
    );
    printer1.ok(&["backup"]);
    assert_eq!(
        printer1.read("sensor.cfg"),
        SENSOR.replace("    PAUSE\n", "    PAUSE\n    M117 Runout\n")
    );
    let output = printer3.ok(&["backup"]);
    assert!(
        output.contains("sensor.cfg: wrote the changes from shared."),
        "{output}"
    );
    assert_eq!(
        printer3.read("sensor.cfg"),
        SENSOR
            .replace("^PG12", "^PA3")
            .replace("    PAUSE\n", "    PAUSE\n    M117 Runout\n")
    );

    // Printer 2's own copy: nothing to write, no needs decision.
    let output = printer2.ok(&["backup"]);
    assert!(!output.contains("sensor.cfg"), "{output}");
    assert_eq!(printer2.read("sensor.cfg"), improved);
}

#[test]
fn decide_and_status_of_followed_files() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.write("other.cfg", "[gcode_macro OTHER]\ngcode:\n    M118 other\n");
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer1.ok(&["copy-to-shared", "other.cfg"]);
    printer2.ok(&["follow", "macros/macros.cfg"]);
    printer2.ok(&["follow", "other.cfg"]);

    let status = printer2.ok(&["status"]);
    assert!(
        status.contains(
            "Followed files (from shared):\n  macros/macros.cfg: up to date\n  other.cfg: up to date\n"
        ),
        "{status}"
    );

    printer2.write(
        "macros/macros.cfg",
        &MACROS.replace("variable_temp: 230", "variable_temp: 250"),
    );
    printer1.write(
        "macros/macros.cfg",
        &MACROS.replace("variable_temp: 230", "variable_temp: 240"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer1.write(
        "other.cfg",
        "[gcode_macro OTHER]\ngcode:\n    M118 changed\n",
    );
    printer1.ok(&["copy-to-shared", "other.cfg"]);
    // status uses printerkeep's last copy of shared: it does not know about the new changes yet.
    let status = printer2.ok(&["status"]);
    assert!(
        status.contains("  macros/macros.cfg: local changes\n"),
        "{status}"
    );
    printer2.ok(&["backup"]);
    let status = printer2.ok(&["status"]);
    assert!(
        status.contains(
            "  macros/macros.cfg: needs decision. Open the file and look for the #### lines\n  other.cfg: up to date\n"
        ),
        "{status}"
    );

    let error = printer2.fails(&["decide", "other.cfg", "--keep-mine"]);
    assert!(
        error.contains("other.cfg does not need decision."),
        "{error}"
    );
    let output = printer2.ok(&["decide", "macros/macros.cfg", "--keep-mine"]);
    assert!(
        output.contains("macros/macros.cfg: kept the lines of this printer."),
        "{output}"
    );
    assert!(
        output.contains("macros/macros.cfg: decision recorded."),
        "{output}"
    );
    assert_eq!(
        printer2.read("macros/macros.cfg"),
        MACROS.replace("variable_temp: 230", "variable_temp: 250")
    );
    let status = printer2.ok(&["status"]);
    assert!(
        status.contains("  macros/macros.cfg: local changes\n"),
        "{status}"
    );

    // Later changes from shared merge with the decision.
    printer1.write(
        "macros/macros.cfg",
        &MACROS
            .replace("variable_temp: 230", "variable_temp: 240")
            .replace("G90", "G91"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["backup"]);
    assert_eq!(
        printer2.read("macros/macros.cfg"),
        MACROS
            .replace("variable_temp: 230", "variable_temp: 250")
            .replace("G90", "G91")
    );
}

/// The modification times of all files in a folder.
fn file_times(folder: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let mut times = Vec::new();
    let mut folders = vec![folder.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in fs::read_dir(&folder).unwrap() {
            let entry = entry.unwrap();
            let metadata = entry.metadata().unwrap();
            if metadata.is_dir() {
                folders.push(entry.path());
            } else {
                times.push((entry.path(), metadata.modified().unwrap()));
            }
        }
    }
    times.sort();
    times
}

#[test]
fn a_run_with_no_change_writes_nothing() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["follow", "macros/macros.cfg"]);
    printer2.ok(&["backup"]);

    // The repository is checked (each run, without XDG_RUNTIME_DIR), but nothing is new.
    let before = file_times(printer2.dir.path());
    std::thread::sleep(std::time::Duration::from_millis(20));
    let output = printer2.ok(&["backup"]);
    assert_eq!(output, "No files changed. No backup made.\n");
    assert_eq!(file_times(printer2.dir.path()), before);
}

#[test]
fn check_repository_sets_how_often_printerkeep_checks() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["follow", "macros/macros.cfg"]);
    let settings = printer2.read("printerkeep.conf");
    printer2.write(
        "printerkeep.conf",
        &format!("{settings}check_repository: 1h\n"),
    );

    let runtime = tempfile::tempdir().unwrap();
    let timer_run = || {
        let output = printer2
            .command(&["backup"])
            .env("XDG_RUNTIME_DIR", runtime.path())
            .assert()
            .success()
            .get_output()
            .clone();
        String::from_utf8(output.stdout).unwrap()
    };
    timer_run();
    let checks: Vec<_> = fs::read_dir(runtime.path().join("printerkeep"))
        .unwrap()
        .collect();
    assert_eq!(checks.len(), 1);

    // A change in shared. The last check was less than an hour ago: the timer run does not see it.
    printer1.write(
        "macros/macros.cfg",
        &MACROS.replace("G1 E50 F300", "G1 E60 F300"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    let output = timer_run();
    assert!(!output.contains("macros/macros.cfg"), "{output}");
    assert_eq!(printer2.read("macros/macros.cfg"), MACROS);

    // After a reboot (an empty XDG_RUNTIME_DIR), or by hand (a command), printerkeep checks.
    fs::remove_dir_all(runtime.path().join("printerkeep")).unwrap();
    let output = timer_run();
    assert!(
        output.contains("macros/macros.cfg: wrote the changes from shared."),
        "{output}"
    );
}

#[test]
fn a_web_edit_gets_to_a_printer_without_changes() {
    let (_dir, url) = repository();
    let printer = Printer::new("printer-1", &url);
    let web = tempfile::tempdir().unwrap();
    let work = web.path().join("work");
    std::process::Command::new("git")
        .args(["clone", "--quiet", "--branch", "printer-1", &url])
        .arg(&work)
        .status()
        .unwrap();
    fs::write(work.join("printer.cfg"), "[printer]\nmax_velocity: 300\n").unwrap();
    for args in [
        &["add", "-A"][..],
        &[
            "-c",
            "user.name=web",
            "-c",
            "user.email=web@example.com",
            "commit",
            "--quiet",
            "-m",
            "web",
        ],
        &["push", "--quiet", "origin", "printer-1"],
    ] {
        assert!(
            std::process::Command::new("git")
                .current_dir(&work)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }
    let web_tip = git(Path::new(&url), &["rev-parse", "printer-1"]);

    let output = printer.ok(&["backup"]);
    assert!(
        output.contains("printer.cfg: wrote the changes from the repository."),
        "{output}"
    );
    assert_eq!(
        printer.read("printer.cfg"),
        "[printer]\nmax_velocity: 300\n"
    );
    // The printer continues the repository's history: no merge commit, nothing to send.
    assert_eq!(
        git(
            &printer.dir.path().join("printerkeep/repo.git"),
            &["rev-parse", "printer-1"]
        ),
        web_tip
    );
    assert_eq!(
        printer.ok(&["backup"]),
        "No files changed. No backup made.\n"
    );
}

#[test]
fn overview_on_shared_shows_each_printer() {
    let (_dir, url) = repository();
    let printer1 = Printer::new("printer-1", &url);
    let printer2 = Printer::new("printer-2", &url);
    printer1.write("macros/macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["follow", "macros/macros.cfg"]);

    let overview = shared(&url, "README.md");
    assert!(overview.contains("# Printers\n"), "{overview}");
    assert!(
        overview.contains(&format!("printerkeep restore {url} <printer name>")),
        "{overview}"
    );
    assert!(
        overview.contains("### printer-1\n\nprinter-1 follows 1 shared file. Branch: `printer-1`."),
        "{overview}"
    );
    assert!(overview.contains("### printer-2\n"), "{overview}");
    assert!(
        overview.contains("| `macros/macros.cfg` | up to date |"),
        "{overview}"
    );

    // The overview changes shared, but not the file: printer 1 makes no backup for it.
    let output = printer1.ok(&["backup"]);
    assert_eq!(output, "No files changed. No backup made.\n");

    // Needs decision shows in the overview.
    printer2.write(
        "macros/macros.cfg",
        &MACROS.replace("variable_temp: 230", "variable_temp: 250"),
    );
    printer1.write(
        "macros/macros.cfg",
        &MACROS.replace("variable_temp: 230", "variable_temp: 240"),
    );
    printer1.ok(&["copy-to-shared", "macros/macros.cfg"]);
    printer2.ok(&["backup"]);
    let overview = shared(&url, "README.md");
    assert!(
        overview.contains("printer-2 follows 1 shared file. 1 needs decision."),
        "{overview}"
    );
    assert!(
        overview.contains("| `macros/macros.cfg` | needs decision |"),
        "{overview}"
    );

    // README.md on shared is not a file to follow.
    let error = printer2.fails(&["follow", "README.md"]);
    assert!(error.contains("the overview of the printers"), "{error}");

    // A printer that follows nothing has no section.
    printer2.ok(&["stop-following", "macros/macros.cfg"]);
    let overview = shared(&url, "README.md");
    assert!(!overview.contains("printer-2"), "{overview}");
    assert!(overview.contains("### printer-1\n"), "{overview}");
}
