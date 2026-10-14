//! Helpers for the integration tests: printers in temporary folders, and local bare repositories that stand
//! in for GitHub.

// Each test file uses a part of these helpers.
#![allow(dead_code)]

use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// A printer: a printer_data folder with a config folder.
pub struct Printer {
    pub dir: TempDir,
}

impl Printer {
    /// A printer with an empty config folder and no settings (the name is `main`).
    pub fn empty() -> Self {
        let printer = Self {
            dir: tempfile::tempdir().unwrap(),
        };
        fs::create_dir_all(printer.config()).unwrap();
        printer
    }

    /// A printer with `printer_name` set in printerkeep.conf.
    pub fn named(name: &str) -> Self {
        let printer = Self::empty();
        printer.write(
            "printerkeep.conf",
            &format!("[printerkeep]\nprinter_name: {name}\n"),
        );
        printer
    }

    /// A named printer with a printer.cfg, connected to the repository `url`.
    pub fn connected(name: &str, url: &str) -> Self {
        let printer = Self::named(name);
        printer.write("printer.cfg", &format!("[printer]\n# {name}\n"));
        printer.ok(&["connect", url]);
        printer
    }

    pub fn config(&self) -> PathBuf {
        self.dir.path().join("config")
    }

    pub fn write(&self, path: &str, content: &str) {
        let file = self.config().join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, content).unwrap();
    }

    pub fn read(&self, path: &str) -> String {
        fs::read_to_string(self.config().join(path)).unwrap()
    }

    /// printerkeep for this printer. Without XDG_RUNTIME_DIR, each run checks the repository.
    pub fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::cargo_bin("printerkeep").unwrap();
        command
            .env_remove("XDG_RUNTIME_DIR")
            .arg("--printer-data")
            .arg(self.dir.path())
            .args(args);
        command
    }

    /// Runs printerkeep, expects success, returns stdout.
    pub fn ok(&self, args: &[&str]) -> String {
        let output = self.command(args).assert().success().get_output().clone();
        String::from_utf8(output.stdout).unwrap()
    }

    /// Runs printerkeep, expects failure, returns stderr.
    pub fn fails(&self, args: &[&str]) -> String {
        let output = self.command(args).assert().failure().get_output().clone();
        String::from_utf8(output.stderr).unwrap()
    }

    /// git in the printer's own git data.
    pub fn git(&self, args: &[&str]) -> String {
        git(&self.dir.path().join("printerkeep/repo.git"), args)
    }

    /// The last commit of a branch in the printer's git data.
    pub fn tip(&self, branch: &str) -> String {
        self.git(&["rev-parse", &format!("refs/heads/{branch}")])
    }

    /// The subject of the last backup on a branch.
    pub fn last_backup(&self, branch: &str) -> String {
        self.git(&["log", "-1", "--format=%s", branch])
    }

    /// The status of a followed file, from `printerkeep status`.
    pub fn status_of(&self, path: &str) -> String {
        let status = self.ok(&["status"]);
        status
            .lines()
            .find_map(|l| l.strip_prefix(&format!("  {path}: ")))
            .unwrap_or_else(|| panic!("{path} not in the status:\n{status}"))
            .to_string()
    }
}

/// An empty bare repository. Returns its folder (keep it) and its URL.
pub fn repository() -> (TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("printers.git");
    git(
        Path::new("."),
        &["init", "--bare", "--quiet", path.to_str().unwrap()],
    );
    let url = path.to_str().unwrap().to_string();
    (dir, url)
}

/// Runs git with this git folder, expects success, returns stdout without the last line end.
pub fn git(git_dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .env("GIT_DIR", git_dir)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

/// The last commit of a branch in the repository.
pub fn remote_tip(url: &str, branch: &str) -> String {
    git(
        Path::new(url),
        &["rev-parse", &format!("refs/heads/{branch}")],
    )
}

fn remote_tip_exists(url: &str, branch: &str) -> bool {
    std::process::Command::new("git")
        .env("GIT_DIR", url)
        .args([
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .unwrap()
        .status
        .success()
}

/// A file on a branch of the repository (`git show branch:path`), with its last line end.
pub fn show(url: &str, branch: &str, path: &str) -> String {
    git(Path::new(url), &["show", &format!("{branch}:{path}")]) + "\n"
}

/// A commit on `branch` of the repository, as an edit in the web pages of the git host: `change` gets the
/// folder of a clone. The branch is made if the repository does not have it.
pub fn web_edit(url: &str, branch: &str, change: impl FnOnce(&Path)) {
    web_commit(url, branch, "web edit", change);
}

/// Like `web_edit`, with a commit message.
pub fn web_commit(url: &str, branch: &str, message: &str, change: impl FnOnce(&Path)) {
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path().join("work");
    let run = |args: &[&str], at: &Path| {
        let output = std::process::Command::new("git")
            .current_dir(at)
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {output:?}");
    };
    run(
        &["clone", "--quiet", url, work.to_str().unwrap()],
        dir.path(),
    );
    if remote_tip_exists(url, branch) {
        run(
            &[
                "checkout",
                "--quiet",
                "-B",
                branch,
                &format!("origin/{branch}"),
            ],
            &work,
        );
    } else {
        run(&["checkout", "--quiet", "--orphan", branch], &work);
    }
    change(&work);
    run(&["add", "-A"], &work);
    run(
        &[
            "-c",
            "user.name=web",
            "-c",
            "user.email=web@example.com",
            "commit",
            "--quiet",
            "-m",
            message,
        ],
        &work,
    );
    run(
        &[
            "push",
            "--quiet",
            "origin",
            &format!("HEAD:refs/heads/{branch}"),
        ],
        &work,
    );
}
