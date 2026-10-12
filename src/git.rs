//! A thin wrapper around the system `git` command.
//!
//! printerkeep uses only old, stable plumbing commands. It never runs `git merge`, `git pull` or
//! `git checkout`, and it never uses the config folder as a work tree for writing.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

#[derive(Debug, thiserror::Error)]
pub enum GitError {
    #[error("git is not installed. Install it, for example with: sudo apt install git")]
    NotInstalled,
    #[error("Cannot run git: {0}")]
    Io(#[from] std::io::Error),
    #[error("git {command} failed: {stderr}")]
    Failed { command: String, stderr: String },
    #[error("git {command} gave output that printerkeep cannot read")]
    Unexpected { command: String },
}

/// Variables that could point git at a different repository or index.
const CLEARED_ENV: &[&str] = &[
    "GIT_DIR",
    "GIT_WORK_TREE",
    "GIT_INDEX_FILE",
    "GIT_OBJECT_DIRECTORY",
    "GIT_ALTERNATE_OBJECT_DIRECTORIES",
    "GIT_COMMON_DIR",
    "GIT_NAMESPACE",
    "GIT_CEILING_DIRECTORIES",
    "GIT_PREFIX",
];

/// One entry of a tree: a file with its mode and blob id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub mode: String,
    pub oid: String,
}

/// All files of a tree, by path (relative, `/`-separated bytes).
pub type Tree = BTreeMap<Vec<u8>, TreeEntry>;

/// The author and committer of printerkeep's commits.
#[derive(Debug, Clone)]
pub struct Identity {
    pub name: String,
    pub email: String,
}

/// A bare git repository.
#[derive(Debug, Clone)]
pub struct Git {
    git_dir: PathBuf,
}

impl Git {
    pub fn new(git_dir: impl Into<PathBuf>) -> Self {
        Self {
            git_dir: git_dir.into(),
        }
    }

    pub fn exists(&self) -> bool {
        self.git_dir.join("HEAD").is_file()
    }

    fn command(&self) -> Command {
        let mut command = bare_command();
        command.env("GIT_DIR", &self.git_dir);
        command
    }

    /// Makes a new bare repository whose HEAD points to `branch`.
    pub fn init(&self, branch: &str) -> Result<(), GitError> {
        std::fs::create_dir_all(&self.git_dir)?;
        run(self.command().args(["init", "--bare", "--quiet"]), None)?;
        // `init --initial-branch` needs git 2.28. symbolic-ref works with all versions.
        run(
            self.command()
                .args(["symbolic-ref", "HEAD"])
                .arg(format!("refs/heads/{branch}")),
            None,
        )?;
        Ok(())
    }

    /// The commit that `reference` points to, or `None` if it does not exist.
    pub fn resolve(&self, reference: &str) -> Result<Option<String>, GitError> {
        let output = output(
            self.command()
                .args(["rev-parse", "--verify", "--quiet"])
                .arg(format!("{reference}^{{commit}}")),
            None,
        )?;
        if output.status.success() {
            Ok(Some(line(&output.stdout)))
        } else if output.status.code() == Some(1) {
            Ok(None)
        } else {
            Err(failed("rev-parse", &output))
        }
    }

    /// All files in the tree of `commit`.
    pub fn read_tree(&self, commit: &str) -> Result<Tree, GitError> {
        let stdout = run(
            self.command()
                .args(["ls-tree", "-r", "-z", "--full-tree"])
                .arg(commit),
            None,
        )?;
        let mut tree = Tree::new();
        for record in stdout.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            // "<mode> SP <type> SP <oid> TAB <path>"
            let unexpected = || GitError::Unexpected {
                command: "ls-tree".into(),
            };
            let tab = record
                .iter()
                .position(|&b| b == b'\t')
                .ok_or_else(unexpected)?;
            let header = std::str::from_utf8(&record[..tab]).map_err(|_| unexpected())?;
            let mut parts = header.split(' ');
            let (Some(mode), Some(_kind), Some(oid)) = (parts.next(), parts.next(), parts.next())
            else {
                return Err(unexpected());
            };
            tree.insert(
                record[tab + 1..].to_vec(),
                TreeEntry {
                    mode: mode.to_string(),
                    oid: oid.to_string(),
                },
            );
        }
        Ok(tree)
    }

    /// Hashes files from disk, exactly as they are (no filters). With `write`, stores them as blobs.
    pub fn hash_files(&self, files: &[PathBuf], write: bool) -> Result<Vec<String>, GitError> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let mut input = Vec::new();
        for file in files {
            input.extend_from_slice(file.as_os_str().as_bytes());
            input.push(b'\n');
        }
        let mut command = self.command();
        command.args(["hash-object", "--no-filters", "--stdin-paths"]);
        if write {
            command.arg("-w");
        }
        let stdout = run(&mut command, Some(&input))?;
        let oids: Vec<String> = String::from_utf8_lossy(&stdout)
            .lines()
            .map(str::to_string)
            .collect();
        if oids.len() != files.len() {
            return Err(GitError::Unexpected {
                command: "hash-object".into(),
            });
        }
        Ok(oids)
    }

    /// The content of a blob.
    pub fn blob(&self, oid: &str) -> Result<Vec<u8>, GitError> {
        run(self.command().args(["cat-file", "blob", oid]), None)
    }

    /// Stores `content` as a blob and returns its id.
    pub fn write_blob(&self, content: &[u8]) -> Result<String, GitError> {
        let stdout = run(
            self.command()
                .args(["hash-object", "-w", "--no-filters", "--stdin"]),
            Some(content),
        )?;
        Ok(line(&stdout))
    }

    /// A three-way merge of three files with `git merge-file --diff3`. Returns the result and the number of
    /// places where the changes touch the same lines (each marked with `marker_size` `<`, `|`, `=` and `>`).
    pub fn merge_file(
        &self,
        mine: &Path,
        base: &Path,
        theirs: &Path,
        marker_size: usize,
    ) -> Result<(Vec<u8>, u32), GitError> {
        let output = output(
            self.command()
                .args(["merge-file", "-p", "--diff3"])
                .arg(format!("--marker-size={marker_size}"))
                .args(["-L", "mine", "-L", "base", "-L", "shared"])
                .arg(mine)
                .arg(base)
                .arg(theirs),
            None,
        )?;
        // The exit code is the number of conflicts (at most 127), or negative on an error.
        match output.status.code() {
            Some(code @ 0..=127) => Ok((output.stdout, code as u32)),
            _ => Err(failed("merge-file", &output)),
        }
    }

    /// Writes a tree with exactly these files. `index` is a temporary index file.
    pub fn write_tree(&self, tree: &Tree, index: &Path) -> Result<String, GitError> {
        match std::fs::remove_file(index) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        let mut input = Vec::new();
        for (path, entry) in tree {
            input.extend_from_slice(format!("{} {}\t", entry.mode, entry.oid).as_bytes());
            input.extend_from_slice(path);
            input.push(0);
        }
        run(
            self.command().env("GIT_INDEX_FILE", index).args([
                "update-index",
                "--add",
                "-z",
                "--index-info",
            ]),
            Some(&input),
        )?;
        let stdout = run(
            self.command()
                .env("GIT_INDEX_FILE", index)
                .arg("write-tree"),
            None,
        )?;
        let _ = std::fs::remove_file(index);
        Ok(line(&stdout))
    }

    /// Makes a commit and returns its id.
    pub fn commit(
        &self,
        tree: &str,
        parent: Option<&str>,
        message: &str,
        identity: &Identity,
    ) -> Result<String, GitError> {
        let mut command = self.command();
        command
            .env("GIT_AUTHOR_NAME", &identity.name)
            .env("GIT_AUTHOR_EMAIL", &identity.email)
            .env("GIT_COMMITTER_NAME", &identity.name)
            .env("GIT_COMMITTER_EMAIL", &identity.email)
            .args(["commit-tree", "--no-gpg-sign", tree]);
        if let Some(parent) = parent {
            command.args(["-p", parent]);
        }
        let stdout = run(&mut command, Some(message.as_bytes()))?;
        Ok(line(&stdout))
    }

    /// A commit with more parents (a merge). printerkeep uses it to join the repository's history.
    pub fn commit_with_parents(
        &self,
        tree: &str,
        parents: &[&str],
        message: &str,
        identity: &Identity,
    ) -> Result<String, GitError> {
        let mut command = self.command();
        command
            .env("GIT_AUTHOR_NAME", &identity.name)
            .env("GIT_AUTHOR_EMAIL", &identity.email)
            .env("GIT_COMMITTER_NAME", &identity.name)
            .env("GIT_COMMITTER_EMAIL", &identity.email)
            .args(["commit-tree", "--no-gpg-sign", tree]);
        for parent in parents {
            command.args(["-p", parent]);
        }
        let stdout = run(&mut command, Some(message.as_bytes()))?;
        Ok(line(&stdout))
    }

    /// The best common ancestor of two commits, or `None` if they have no common history.
    pub fn merge_base(&self, a: &str, b: &str) -> Result<Option<String>, GitError> {
        let output = output(self.command().args(["merge-base", a, b]), None)?;
        match output.status.code() {
            Some(0) => Ok(Some(line(&output.stdout))),
            Some(1) => Ok(None),
            _ => Err(failed("merge-base", &output)),
        }
    }

    /// Moves `reference` to `new`, but only if it still points to `old` (`None`: it must not exist).
    pub fn update_ref(
        &self,
        reference: &str,
        new: &str,
        old: Option<&str>,
        reason: &str,
    ) -> Result<(), GitError> {
        run(
            self.command().args([
                "update-ref",
                "-m",
                reason,
                reference,
                new,
                old.unwrap_or(""),
            ]),
            None,
        )?;
        Ok(())
    }

    /// Moves `reference` to `new`, whatever it points to now. For printerkeep's own references only.
    pub fn set_ref(&self, reference: &str, new: &str, reason: &str) -> Result<(), GitError> {
        run(
            self.command()
                .args(["update-ref", "-m", reason, reference, new]),
            None,
        )?;
        Ok(())
    }

    /// The commit time (seconds since 1970) and subject of `commit`.
    pub fn commit_info(&self, commit: &str) -> Result<(u64, String), GitError> {
        let stdout = run(
            self.command()
                .args(["show", "-s", "--format=%ct%x00%s"])
                .arg(commit),
            None,
        )?;
        let text = String::from_utf8_lossy(&stdout);
        let text = text.trim_end_matches('\n');
        text.split_once('\0')
            .and_then(|(time, subject)| Some((time.parse().ok()?, subject.to_string())))
            .ok_or_else(|| GitError::Unexpected {
                command: "show".into(),
            })
    }

    /// The number of commits that `commit` has in its history, itself included.
    pub fn count_commits(&self, commit: &str) -> Result<u64, GitError> {
        let stdout = run(self.command().args(["rev-list", "--count", commit]), None)?;
        line(&stdout).parse().map_err(|_| GitError::Unexpected {
            command: "rev-list".into(),
        })
    }

    /// The number of commits in `to` that are not in `from`.
    pub fn count_between(&self, from: &str, to: &str) -> Result<u64, GitError> {
        let stdout = run(
            self.command()
                .args(["rev-list", "--count"])
                .arg(format!("{from}..{to}")),
            None,
        )?;
        line(&stdout).parse().map_err(|_| GitError::Unexpected {
            command: "rev-list".into(),
        })
    }

    /// `true` if `ancestor` is in the history of `commit` (or is `commit`).
    pub fn is_ancestor(&self, ancestor: &str, commit: &str) -> Result<bool, GitError> {
        let output = output(
            self.command()
                .args(["merge-base", "--is-ancestor", ancestor, commit]),
            None,
        )?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(failed("merge-base", &output)),
        }
    }

    /// The commit of `refs/heads/<branch>` in the repository, or `None` if the branch does not exist.
    /// Also tests the connection and the authentication.
    pub fn remote_branch(&self, remote: &Remote, branch: &str) -> Result<Option<String>, GitError> {
        let stdout = run(
            remote
                .command(self)
                .args(["ls-remote", "--heads", &remote.url])
                .arg(format!("refs/heads/{branch}")),
            None,
        )?;
        Ok(String::from_utf8_lossy(&stdout)
            .split_whitespace()
            .next()
            .map(str::to_string))
    }

    /// The names of all branches in the repository.
    pub fn remote_branches(&self, remote: &Remote) -> Result<Vec<String>, GitError> {
        let stdout = run(
            remote
                .command(self)
                .args(["ls-remote", "--heads", &remote.url]),
            None,
        )?;
        Ok(String::from_utf8_lossy(&stdout)
            .lines()
            .filter_map(|l| l.split('\t').nth(1))
            .filter_map(|r| r.strip_prefix("refs/heads/"))
            .map(str::to_string)
            .collect())
    }

    /// Copies `refs/heads/<branch>` from the repository to `local_ref`.
    pub fn fetch(&self, remote: &Remote, branch: &str, local_ref: &str) -> Result<(), GitError> {
        run(
            remote
                .command(self)
                .args(["fetch", "--quiet", "--no-tags", &remote.url])
                .arg(format!("+refs/heads/{branch}:{local_ref}")),
            None,
        )?;
        Ok(())
    }

    /// Like `fetch`, but returns `false` if the repository has no such branch.
    pub fn fetch_if_exists(
        &self,
        remote: &Remote,
        branch: &str,
        local_ref: &str,
    ) -> Result<bool, GitError> {
        match self.fetch(remote, branch, local_ref) {
            Ok(()) => Ok(true),
            Err(GitError::Failed { stderr, .. })
                if stderr.to_lowercase().contains("couldn't find remote ref") =>
            {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }

    /// Sends `local_ref` to `refs/heads/<branch>` in the repository. Never forces.
    pub fn push(&self, remote: &Remote, local_ref: &str, branch: &str) -> Result<(), GitError> {
        run(
            remote
                .command(self)
                .args(["push", "--quiet", &remote.url])
                .arg(format!("{local_ref}:refs/heads/{branch}")),
            None,
        )?;
        Ok(())
    }
}

/// How printerkeep logs in to a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    /// A folder on this computer or a network share: no login.
    None,
    /// HTTPS with a personal access token, kept by git's `store` credential helper in this file.
    Token { credentials: PathBuf },
    /// SSH with printerkeep's own key and its own known_hosts file.
    Ssh { key: PathBuf, known_hosts: PathBuf },
}

/// A repository URL and its login.
#[derive(Debug, Clone)]
pub struct Remote {
    pub url: String,
    pub auth: Auth,
}

impl Remote {
    /// A git command for this remote. The `-c` options must come before the subcommand.
    fn command(&self, git: &Git) -> Command {
        let mut command = git.command();
        // Stop slow or stalled HTTPS transfers instead of hanging the timer.
        command.args([
            "-c",
            "http.lowSpeedLimit=1000",
            "-c",
            "http.lowSpeedTime=60",
        ]);
        match &self.auth {
            Auth::None => {}
            Auth::Token { credentials } => {
                // The empty value removes the user's own credential helpers, so only printerkeep's file is used.
                // The helper answers "get" only. When the repository refuses a token, git tells the helpers
                // to erase it, and the "store" helper would remove it from the file. printerkeep alone
                // changes the file: a short failure on the server must not remove a token that works.
                command.args(["-c", "credential.helper="]);
                command.arg("-c").arg(format!(
                    "credential.helper=!f() {{ test \"$1\" = get && git credential-store --file={} get; }}; f",
                    shell_quote(credentials)
                ));
            }
            Auth::Ssh { key, known_hosts } => {
                command.env(
                    "GIT_SSH_COMMAND",
                    format!(
                        "ssh -i {} -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=20 \
                         -o StrictHostKeyChecking=accept-new -o UserKnownHostsFile={}",
                        shell_quote(key),
                        shell_quote(known_hosts)
                    ),
                );
            }
        }
        command
    }
}

/// Quotes a path for the shell that git uses to run helpers and `ssh`.
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.display().to_string().replace('\'', r"'\''"))
}

/// `true` if `name` is a valid branch name.
pub fn is_valid_branch_name(name: &str) -> Result<bool, GitError> {
    let output = output(
        bare_command()
            .args(["check-ref-format", "--branch"])
            .arg(name),
        None,
    )?;
    Ok(output.status.success())
}

fn bare_command() -> Command {
    let mut command = Command::new("git");
    for variable in CLEARED_ENV {
        command.env_remove(variable);
    }
    // Never wait for a password on a terminal; a timer has no terminal.
    command.env("GIT_TERMINAL_PROMPT", "0");
    command.env("LC_ALL", "C");
    command
}

fn output(command: &mut Command, stdin: Option<&[u8]>) -> Result<Output, GitError> {
    command
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            GitError::NotInstalled
        } else {
            GitError::Io(e)
        }
    })?;
    // Write stdin in a thread, so a full stdout pipe cannot block the write.
    let output = std::thread::scope(|scope| {
        if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
            scope.spawn(move || {
                let _ = pipe.write_all(input);
            });
        }
        child.wait_with_output()
    })?;
    Ok(output)
}

fn run(command: &mut Command, stdin: Option<&[u8]>) -> Result<Vec<u8>, GitError> {
    let name = subcommand(command);
    let output = output(command, stdin)?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(failed(&name, &output))
    }
}

fn subcommand(command: &Command) -> String {
    command
        .get_args()
        .next()
        .map(OsStr::to_string_lossy)
        .unwrap_or_default()
        .into_owned()
}

fn failed(command: &str, output: &Output) -> GitError {
    GitError::Failed {
        command: command.to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
    }
}

fn line(stdout: &[u8]) -> String {
    String::from_utf8_lossy(stdout).trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// git must find the token through the exact options that printerkeep passes, also when the path has
    /// a space and the user has other credential helpers.
    #[test]
    fn token_auth_gives_git_the_stored_token() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("printer data");
        std::fs::create_dir_all(&data).unwrap();
        let credentials = data.join("credentials");
        std::fs::write(
            &credentials,
            "https://printerkeep:github_pat_test@github.com\n",
        )
        .unwrap();
        let git = Git::new(dir.path().join("repo.git"));
        git.init("printer-1").unwrap();
        let remote = Remote {
            url: "https://github.com/me/printers.git".into(),
            auth: Auth::Token {
                credentials: credentials.clone(),
            },
        };

        let mut command = remote.command(&git);
        command
            .env("GIT_CONFIG_COUNT", "1")
            .env("GIT_CONFIG_KEY_0", "credential.helper")
            .env("GIT_CONFIG_VALUE_0", "!echo password=wrong-helper; :")
            .args(["credential", "fill"]);
        let stdout = run(
            &mut command,
            Some(b"protocol=https\nhost=github.com\npath=me/printers.git\n\n"),
        )
        .unwrap();
        let text = String::from_utf8(stdout).unwrap();
        assert!(text.contains("username=printerkeep\n"), "{text}");
        assert!(text.contains("password=github_pat_test\n"), "{text}");

        // When the repository refuses the token, git tells the helper to erase it. The file stays.
        let mut reject = remote.command(&git);
        reject.args(["credential", "reject"]);
        run(
            &mut reject,
            Some(
                b"protocol=https\nhost=github.com\nusername=printerkeep\npassword=github_pat_test\n\n",
            ),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&credentials).unwrap(),
            "https://printerkeep:github_pat_test@github.com\n"
        );
    }
}
