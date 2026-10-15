//! Personal access tokens over HTTPS, against a local git server that works like GitHub
//! (`tests/support/https_git_server.py`). Needs python3 and openssl.

use assert_cmd::Command;
use predicates::prelude::PredicateBooleanExt;
use predicates::str::contains;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as Process};
use std::time::{Duration, Instant};
use tempfile::TempDir;

/// The git server. Stops when dropped.
struct Server {
    dir: TempDir,
    process: Child,
    port: String,
}

impl Server {
    fn start(token: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let repositories = dir.path().join("repositories");
        fs::create_dir_all(&repositories).unwrap();
        git(&repositories, &["init", "-q", "--bare", "printers.git"]);
        fs::write(dir.path().join("token"), token).unwrap();
        let script =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/support/https_git_server.py");
        let process = Process::new("python3")
            .arg(script)
            .arg(dir.path())
            .spawn()
            .expect("python3 is necessary for this test");
        let port_file = dir.path().join("port");
        let start = Instant::now();
        while !port_file.exists() {
            assert!(
                start.elapsed() < Duration::from_secs(30),
                "the test server did not start"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        let port = fs::read_to_string(port_file).unwrap();
        Self { dir, process, port }
    }

    fn url(&self) -> String {
        format!("https://127.0.0.1:{}/printers.git", self.port)
    }

    /// The server accepts only this token from now on, as when a token expires or is removed.
    fn accept_only(&self, token: &str) {
        fs::write(self.dir.path().join("token"), token).unwrap();
    }

    fn cert(&self) -> PathBuf {
        self.dir.path().join("cert.pem")
    }

    /// The commit of a branch in the server's repository.
    fn branch(&self, name: &str) -> Option<String> {
        let output = Process::new("git")
            .arg("--git-dir")
            .arg(self.dir.path().join("repositories/printers.git"))
            .args(["rev-parse", "--verify", "--quiet", name])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).unwrap().trim().to_string())
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

fn git(dir: &Path, args: &[&str]) {
    let status = Process::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}

/// A printer that trusts the server's certificate.
struct Printer<'a> {
    dir: TempDir,
    server: &'a Server,
}

impl<'a> Printer<'a> {
    fn new(server: &'a Server) -> Self {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("config")).unwrap();
        fs::write(
            dir.path().join("config/printerkeep.conf"),
            "[printerkeep]\nprinter_name: printer-1\n",
        )
        .unwrap();
        fs::write(dir.path().join("config/printer.cfg"), "[printer]\n").unwrap();
        Self { dir, server }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::cargo_bin("printerkeep").unwrap();
        command
            .env_remove("XDG_RUNTIME_DIR")
            .env("GIT_SSL_CAINFO", self.server.cert())
            .arg("--printer-data")
            .arg(self.dir.path())
            .args(args);
        command
    }

    fn credentials(&self) -> String {
        fs::read_to_string(self.dir.path().join("printerkeep/credentials")).unwrap()
    }

    fn sent(&self) -> String {
        let output = Process::new("git")
            .arg("--git-dir")
            .arg(self.dir.path().join("printerkeep/repo.git"))
            .args(["rev-parse", "printer-1"])
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }
}

#[test]
fn stored_token_is_used_again() {
    let server = Server::start("token-1");
    let printer = Printer::new(&server);
    let url = server.url();

    printer
        .command(&["connect", &url])
        .write_stdin("token-1\n")
        .assert()
        .success()
        .stdout(contains("and stored the token."));
    assert_eq!(server.branch("printer-1"), Some(printer.sent()));
    assert!(printer.credentials().contains(":token-1@127.0.0.1:"));

    // No token on standard input: printerkeep must not ask.
    printer
        .command(&["connect", &url])
        .write_stdin("")
        .assert()
        .success()
        .stdout(contains("with the stored token."));
}

#[test]
fn wrong_token_is_refused_and_not_stored() {
    let server = Server::start("token-1");
    let printer = Printer::new(&server);
    printer
        .command(&["connect", &server.url()])
        .write_stdin("wrong\n")
        .assert()
        .failure()
        .stderr(contains("refused the token"));
    assert!(!printer.dir.path().join("printerkeep/credentials").exists());
    assert!(
        !printer
            .dir
            .path()
            .join("printerkeep/credentials.new")
            .exists()
    );
    assert_eq!(server.branch("printer-1"), None);
}

#[test]
fn change_token_after_the_token_expired() {
    let server = Server::start("token-1");
    let printer = Printer::new(&server);
    printer
        .command(&["connect", &server.url()])
        .write_stdin("token-1\n")
        .assert()
        .success();

    // The token expires. The next backup is made, but not sent.
    server.accept_only("token-2");
    fs::write(
        printer.dir.path().join("config/printer.cfg"),
        "[printer]\n# new\n",
    )
    .unwrap();
    printer
        .command(&["sync"])
        .assert()
        .success()
        .stdout(contains("printerkeep change-token"));
    assert_ne!(server.branch("printer-1"), Some(printer.sent()));
    // status has no network, but it tells the problem of the last try.
    printer
        .command(&["status"])
        .assert()
        .success()
        .stdout(contains(
            "Backups not in the repository: 1. The last try to send them didn't work:",
        ))
        .stdout(contains("printerkeep change-token"));
    // git did not remove the old token: only printerkeep changes the file.
    assert!(printer.credentials().contains(":token-1@"));

    // A wrong new token does not replace the old one.
    printer
        .command(&["change-token"])
        .write_stdin("wrong\n")
        .assert()
        .failure()
        .stderr(contains("The stored login didn't change."));
    assert!(printer.credentials().contains(":token-1@"));

    // The right token replaces it, and the backup is sent.
    printer
        .command(&["change-token"])
        .write_stdin("token-2\n")
        .assert()
        .success()
        .stdout(contains("accepted the new token"));
    assert!(printer.credentials().contains(":token-2@"));
    assert_eq!(server.branch("printer-1"), Some(printer.sent()));
    printer
        .command(&["status"])
        .assert()
        .success()
        .stdout(contains("All backups are in the repository."))
        .stdout(contains("didn't work").not());
}

#[test]
fn connect_asks_again_when_the_stored_token_is_refused() {
    let server = Server::start("token-1");
    let printer = Printer::new(&server);
    let url = server.url();
    printer
        .command(&["connect", &url])
        .write_stdin("token-1\n")
        .assert()
        .success();

    server.accept_only("token-2");
    printer
        .command(&["connect", &url])
        .write_stdin("token-2\n")
        .assert()
        .success()
        .stdout(contains("and stored the token."));
    assert!(printer.credentials().contains(":token-2@"));
}

#[test]
fn change_token_needs_an_https_repository() {
    let server = Server::start("token-1");
    let printer = Printer::new(&server);
    printer
        .command(&["change-token"])
        .assert()
        .failure()
        .stderr(contains(
            "This printer isn't connected to a repository yet.",
        ));

    fs::write(
        printer.dir.path().join("config/printerkeep.conf"),
        "[printerkeep]\nprinter_name: printer-1\nrepository: git@github.com:me/printers.git\n",
    )
    .unwrap();
    printer
        .command(&["change-token"])
        .assert()
        .failure()
        .stderr(contains("uses an SSH key, not a token"));
}
