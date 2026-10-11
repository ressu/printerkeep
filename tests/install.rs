//! M3: install and uninstall. Fake systemctl and loginctl commands write what they get to a log.

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

const MOONRAKER_CONF: &str = "[server]\nhost: 0.0.0.0\n\n[update_manager fluidd]\ntype: web\n";

/// A home folder with printer_data, and fake system commands.
struct Host {
    dir: TempDir,
}

impl Host {
    fn new() -> Self {
        let host = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        let config = host.printer_data().join("config");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("printer.cfg"), "[printer]\nkinematics: none\n").unwrap();
        fs::write(config.join("moonraker.conf"), MOONRAKER_CONF).unwrap();
        fs::create_dir_all(host.bin()).unwrap();
        // The log of the fake commands is outside the home folder.
        host.fake(
            "systemctl",
            "echo \"systemctl $*\" >> \"$FAKE_LOG\"; [ -z \"$FAKE_SYSTEMCTL_FAILS\" ] || { echo 'Failed to connect to bus' >&2; exit 1; }",
        );
        host.fake(
            "loginctl",
            "echo \"loginctl $*\" >> \"$FAKE_LOG\"
             case \"$1\" in
               show-user) echo \"${FAKE_LINGER:-no}\" ;;
               enable-linger) [ -z \"$FAKE_LINGER_REFUSED\" ] || exit 1 ;;
             esac",
        );
        host
    }

    fn home(&self) -> PathBuf {
        self.dir.path().join("home")
    }

    fn printer_data(&self) -> PathBuf {
        self.home().join("printer_data")
    }

    fn bin(&self) -> PathBuf {
        self.dir.path().join("bin")
    }

    fn log(&self) -> String {
        fs::read_to_string(self.dir.path().join("log")).unwrap_or_default()
    }

    fn fake(&self, name: &str, script: &str) {
        let file = self.bin().join(name);
        fs::write(&file, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn program_dir(&self) -> PathBuf {
        self.home().join("printerkeep")
    }

    fn program(&self) -> PathBuf {
        self.program_dir()
            .join(printerkeep::install::asset().unwrap())
    }

    fn units(&self) -> PathBuf {
        self.home().join(".config/systemd/user")
    }

    fn moonraker_conf(&self) -> String {
        fs::read_to_string(self.printer_data().join("config/moonraker.conf")).unwrap()
    }

    /// printerkeep from the build, or another program file.
    fn run_program(&self, program: &Path, args: &[&str]) -> Command {
        let mut command = Command::new(program);
        let path = format!(
            "{}:{}",
            self.bin().display(),
            std::env::var("PATH").unwrap_or_default()
        );
        command
            .env("HOME", self.home())
            .env("PATH", path)
            .env("USER", "printer")
            .env("FAKE_LOG", self.dir.path().join("log"))
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("PRINTERKEEP_PRINTER_DATA")
            .args(args);
        command
    }

    fn run(&self, args: &[&str]) -> Command {
        self.run_program(&assert_cmd::cargo::cargo_bin("printerkeep"), args)
    }
}

#[test]
fn install_puts_everything_in_the_home_folder() {
    let host = Host::new();
    host.run(&["install"])
        .assert()
        .success()
        .stdout(contains("printerkeep is installed."))
        .stdout(contains("First backup: 2 files"))
        .stdout(contains(
            "Lingering: turned on for user printer, so backups run also when nobody is logged in.",
        ))
        .stdout(contains(
            "added [update_manager printerkeep] to moonraker.conf",
        ));

    // The program, for update_manager.
    let program = host.program();
    let mode = fs::metadata(&program).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o755);
    let release_info = fs::read_to_string(host.program_dir().join("release_info.json")).unwrap();
    assert!(release_info.contains("\"project_owner\": \"ressu\""));
    assert!(release_info.contains("\"project_name\": \"printerkeep\""));
    assert!(release_info.contains(&format!("\"version\": \"v{}\"", printerkeep::VERSION)));
    assert!(release_info.contains(&format!(
        "\"asset_name\": \"{}\"",
        printerkeep::install::asset().unwrap()
    )));

    // The command.
    assert_eq!(
        fs::read_link(host.home().join(".local/bin/printerkeep")).unwrap(),
        program
    );

    // The units: user units, the program in the home folder, no layerlock.
    let service = fs::read_to_string(host.units().join("printerkeep.service")).unwrap();
    assert!(service.contains(&format!(
        "ExecStart=\"{}\" --printer-data \"{}\" backup",
        program.display(),
        host.printer_data().display()
    )));
    assert!(!service.contains("ExecCondition"));
    assert!(host.units().join("printerkeep.timer").is_file());
    let log = host.log();
    assert!(log.contains("systemctl --user daemon-reload"));
    assert!(log.contains("systemctl --user enable --now printerkeep.timer"));
    assert!(log.contains("loginctl enable-linger"));
    assert!(!log.contains("sudo"));

    // update_manager.
    let conf = host.moonraker_conf();
    assert!(conf.starts_with(MOONRAKER_CONF));
    assert!(conf.contains("[update_manager printerkeep]\ntype: executable\n"));
    assert!(conf.contains("repo: ressu/printerkeep\npath: ~/printerkeep\n"));
    assert!(conf.contains("is_system_service: False\n"));
}

#[test]
fn install_again_changes_nothing_twice() {
    let host = Host::new();
    host.run(&["install"]).assert().success();
    // The installed program installs again from its own place.
    host.run_program(&host.program(), &["install"])
        .env("FAKE_LINGER", "yes")
        .assert()
        .success()
        .stdout(contains(
            "moonraker.conf already has [update_manager printerkeep]",
        ))
        .stdout(contains("Lingering: on for user printer."))
        .stdout(contains("No files changed"));
    assert_eq!(
        host.moonraker_conf()
            .matches("[update_manager printerkeep]")
            .count(),
        1
    );
    assert!(host.program().is_file());
}

#[test]
fn install_uses_layerlock_when_it_is_installed() {
    let host = Host::new();
    host.fake("layerlock", "exit 0");
    host.run(&["install"])
        .assert()
        .success()
        .stdout(contains("layerlock: no backup starts during a print."));
    let service = fs::read_to_string(host.units().join("printerkeep.service")).unwrap();
    assert!(service.contains(&format!(
        "ExecCondition=\"{}\" --fail-open\n",
        host.bin().join("layerlock").display()
    )));
    host.run(&["status"])
        .assert()
        .success()
        .stdout(contains("layerlock").not());

    // layerlock removed: systemd would skip every backup, so status tells it.
    fs::remove_file(host.bin().join("layerlock")).unwrap();
    host.run(&["status"]).assert().success().stdout(contains(
        "but that file does not exist. systemd skips every backup.",
    ));
}

#[test]
fn status_tells_when_layerlock_was_installed_later() {
    let host = Host::new();
    host.run(&["install"]).assert().success();
    host.fake("layerlock", "exit 0");
    host.run(&["status"])
        .assert()
        .success()
        .stdout(contains("but the backup service does not use it"));
    host.run(&["install"]).assert().success();
    host.run(&["status"])
        .assert()
        .success()
        .stdout(contains("layerlock").not());
}

#[test]
fn install_tells_how_to_turn_on_lingering_when_it_is_not_permitted() {
    let host = Host::new();
    host.run(&["install"])
        .env("FAKE_LINGER_REFUSED", "1")
        .assert()
        .success()
        .stdout(contains(
            "Backups run only while user printer is logged in. To run them always, an administrator \
             can run: sudo loginctl enable-linger printer",
        ));
}

#[test]
fn install_without_the_systemd_user_manager_fails_clearly() {
    let host = Host::new();
    host.run(&["install"])
        .env("FAKE_SYSTEMCTL_FAILS", "1")
        .assert()
        .failure()
        .stderr(contains("printerkeep uses the systemd user manager"));
}

#[test]
fn install_without_moonraker_conf_shows_the_section() {
    let host = Host::new();
    fs::remove_file(host.printer_data().join("config/moonraker.conf")).unwrap();
    host.run(&["install"])
        .assert()
        .success()
        .stdout(contains("no moonraker.conf in the config folder"))
        .stdout(contains("    [update_manager printerkeep]"));
}

#[test]
fn install_never_replaces_a_command_that_is_not_a_link() {
    let host = Host::new();
    let command = host.home().join(".local/bin/printerkeep");
    fs::create_dir_all(command.parent().unwrap()).unwrap();
    fs::write(&command, "my own script").unwrap();
    host.run(&["install"])
        .assert()
        .success()
        .stdout(contains("not changed, because"));
    assert_eq!(fs::read_to_string(&command).unwrap(), "my own script");
}

#[test]
fn uninstall_removes_the_installation_and_keeps_the_history() {
    let host = Host::new();
    host.run(&["install"]).assert().success();
    host.run(&["uninstall"])
        .assert()
        .success()
        .stdout(contains(
            "Removed printerkeep.service and printerkeep.timer.",
        ))
        .stdout(contains("Restart Moonraker to remove printerkeep"))
        .stdout(contains(
            "printerkeep install turned on lingering for user printer. If nothing else needs it, \
             turn it off with: loginctl disable-linger",
        ))
        .stdout(contains(format!(
            "Kept: {}",
            host.printer_data().join("printerkeep").display()
        )));

    assert!(!host.program_dir().exists());
    assert!(!host.units().join("printerkeep.service").exists());
    assert!(!host.units().join("printerkeep.timer").exists());
    assert!(fs::symlink_metadata(host.home().join(".local/bin/printerkeep")).is_err());
    assert_eq!(host.moonraker_conf(), MOONRAKER_CONF);
    assert!(host.printer_data().join("printerkeep/repo.git").is_dir());
    assert!(
        host.log()
            .contains("systemctl --user disable --now printerkeep.timer")
    );

    // A second uninstall has nothing to do.
    host.run(&["uninstall"])
        .assert()
        .success()
        .stdout(contains("printerkeep is removed."))
        .stdout(contains("Removed").not());
}
