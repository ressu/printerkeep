//! The repository: login, sending backups, continuing an existing history, and restoring files.

use crate::git::{Auth, Git, GitError, Remote, Tree};
use crate::paths::Paths;
use std::ffi::OsStr;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error(
        "Do not put the token in the URL: printerkeep.conf would contain it, and the repository would get \
         it. Use the URL without the token. printerkeep asks for the token."
    )]
    TokenInUrl,
    #[error(
        "printerkeep cannot use the URL '{0}'. Use an https:// URL (token), an SSH URL (key), or a folder."
    )]
    UnsupportedUrl(String),
    #[error("{}", login_failed(url))]
    LoginFailed { url: String },
    #[error(
        "Cannot connect to the repository {url}: {message}. Check the network connection of the printer, \
         and the repository URL in printerkeep.conf"
    )]
    Connection { url: String, message: String },
    #[error("The repository has backups on this printer's branch that are not on this printer")]
    RepositoryAhead,
    #[error(
        "ssh-keygen is not installed. Install it, for example with: sudo apt install openssh-client"
    )]
    NoSshKeygen,
    #[error("ssh-keygen failed: {0}")]
    SshKeygen(String),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Git(GitError),
}

/// The kind of repository URL. It selects the login.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlKind {
    /// `https://…`: personal access token.
    Https,
    /// `ssh://…` or `user@host:path`: SSH key.
    Ssh,
    /// A folder (`/srv/printers.git` or `file://…`): no login.
    Folder,
}

pub fn url_kind(url: &str) -> Result<UrlKind, RepositoryError> {
    if let Some(rest) = url.strip_prefix("https://") {
        let authority = rest.split('/').next().unwrap_or_default();
        if authority
            .rsplit_once('@')
            .is_some_and(|(user, _)| user.contains(':'))
        {
            return Err(RepositoryError::TokenInUrl);
        }
        return Ok(UrlKind::Https);
    }
    if url.starts_with("ssh://") {
        return Ok(UrlKind::Ssh);
    }
    if url.starts_with("file://") || url.starts_with('/') {
        return Ok(UrlKind::Folder);
    }
    if url.contains("://") {
        return Err(RepositoryError::UnsupportedUrl(url.to_string()));
    }
    // The scp form: [user@]host:path
    match url.split_once(':') {
        Some((host, path)) if !host.is_empty() && !host.contains('/') && !path.is_empty() => {
            Ok(UrlKind::Ssh)
        }
        _ => Err(RepositoryError::UnsupportedUrl(url.to_string())),
    }
}

/// The login for `url`, with printerkeep's files in its data folder.
pub fn remote(url: &str, paths: &Paths) -> Result<Remote, RepositoryError> {
    let auth = match url_kind(url)? {
        UrlKind::Https => Auth::Token {
            credentials: credentials_file(paths),
        },
        UrlKind::Ssh => Auth::Ssh {
            key: key_file(paths),
            known_hosts: paths.data.join("known_hosts"),
        },
        UrlKind::Folder => Auth::None,
    };
    Ok(Remote {
        url: url.to_string(),
        auth,
    })
}

fn credentials_file(paths: &Paths) -> PathBuf {
    paths.data.join("credentials")
}

fn new_credentials_file(paths: &Paths) -> PathBuf {
    paths.data.join("credentials.new")
}

fn key_file(paths: &Paths) -> PathBuf {
    paths.data.join("id_ed25519")
}

/// The user name and the host of an https:// URL.
fn https_login(url: &str) -> (&str, &str) {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    let authority = rest.split('/').next().unwrap_or_default();
    match authority.rsplit_once('@') {
        Some((user, host)) => (user, host),
        // Most git hosts (GitHub, GitLab, Gitea) accept any user name with a token.
        None => ("printerkeep", authority),
    }
}

/// True if a token for the host of `url` is stored.
pub fn has_token(paths: &Paths, url: &str) -> bool {
    let (_, host) = https_login(url);
    std::fs::read_to_string(credentials_file(paths)).is_ok_and(|text| {
        text.lines()
            .any(|line| line.rsplit_once('@').is_some_and(|(_, h)| h == host))
    })
}

/// Stores a new token for `url` next to the current one, in git's credential file format. Only this user
/// can read the file. Test it with `new_token_remote`, then call `keep_new_token` or `discard_new_token`.
/// Thus, a wrong token never replaces a token that works.
pub fn store_new_token(paths: &Paths, url: &str, token: &str) -> Result<(), RepositoryError> {
    let (user, host) = https_login(url);
    let line = format!(
        "https://{}:{}@{host}\n",
        percent_encode(user),
        percent_encode(token)
    );

    let io = |context: &str| {
        let context = context.to_string();
        move |source| RepositoryError::Io { context, source }
    };
    create_private_folder(&paths.data).map_err(io("Cannot make the data folder"))?;
    let file = new_credentials_file(paths);
    let _ = std::fs::remove_file(&file);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&file)
        .and_then(|mut f| f.write_all(line.as_bytes()))
        .map_err(io("Cannot store the token"))
}

/// The login for `url` with the new token (see `store_new_token`).
pub fn new_token_remote(url: &str, paths: &Paths) -> Remote {
    Remote {
        url: url.to_string(),
        auth: Auth::Token {
            credentials: new_credentials_file(paths),
        },
    }
}

/// The new token replaces the current one.
pub fn keep_new_token(paths: &Paths) -> Result<(), RepositoryError> {
    std::fs::rename(new_credentials_file(paths), credentials_file(paths)).map_err(|source| {
        RepositoryError::Io {
            context: "Cannot store the token".into(),
            source,
        }
    })
}

/// Removes the new token. The current token does not change.
pub fn discard_new_token(paths: &Paths) {
    let _ = std::fs::remove_file(new_credentials_file(paths));
}

fn login_failed(url: &str) -> String {
    let what = if url.starts_with("https://") {
        "If the token expired or was removed, run: printerkeep change-token".to_string()
    } else {
        format!("If the key was removed, run: printerkeep add-repository {url}")
    };
    format!("The repository did not accept the login, or the URL is wrong ({url}). {what}")
}

/// Makes printerkeep's SSH key if it does not exist. Returns the public key.
pub fn ensure_ssh_key(paths: &Paths, printer_name: &str) -> Result<String, RepositoryError> {
    let key = key_file(paths);
    let public = key.with_extension("pub");
    if !key.exists() {
        create_private_folder(&paths.data).map_err(|source| RepositoryError::Io {
            context: "Cannot make the data folder".into(),
            source,
        })?;
        let output = Command::new("ssh-keygen")
            .args(["-q", "-t", "ed25519", "-N", "", "-C"])
            .arg(format!("printerkeep@{printer_name}"))
            .arg("-f")
            .arg(&key)
            .output()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    RepositoryError::NoSshKeygen
                } else {
                    RepositoryError::SshKeygen(e.to_string())
                }
            })?;
        if !output.status.success() {
            return Err(RepositoryError::SshKeygen(
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
            ));
        }
    }
    std::fs::read_to_string(&public)
        .map(|k| k.trim().to_string())
        .map_err(|source| RepositoryError::Io {
            context: format!("Cannot read {}", public.display()),
            source,
        })
}

fn create_private_folder(folder: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    std::fs::set_permissions(folder, std::fs::Permissions::from_mode(0o700))
}

fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Makes a repository error from a git error of a network command.
pub fn classify(error: GitError, url: &str) -> RepositoryError {
    let GitError::Failed { stderr, .. } = &error else {
        return RepositoryError::Git(error);
    };
    let text = stderr.to_lowercase();
    let any = |words: &[&str]| words.iter().any(|w| text.contains(w));
    if any(&["non-fast-forward", "fetch first", "[rejected]"]) {
        RepositoryError::RepositoryAhead
    } else if any(&[
        "authentication failed",
        "permission denied",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "invalid username or password",
        "repository not found",
        "error: 401",
        "error: 403",
        "returned error: 401",
        "returned error: 403",
    ]) {
        RepositoryError::LoginFailed {
            url: url.to_string(),
        }
    } else {
        RepositoryError::Connection {
            url: url.to_string(),
            message: main_message(stderr),
        }
    }
}

/// The line of git's error output that says what went wrong: the first "fatal:" or "error:" line (git adds
/// general advice after it), without that word and without the last period.
fn main_message(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let line = lines
        .iter()
        .find(|l| l.starts_with("fatal:") || l.starts_with("error:"))
        .or(lines.last())
        .copied()
        .unwrap_or("unknown error");
    line.trim_start_matches("fatal:")
        .trim_start_matches("error:")
        .trim()
        .trim_end_matches('.')
        .to_string()
}

/// The reference with the last backup that the repository has (from this printer's view).
pub fn sent_ref(printer_name: &str) -> String {
    format!("refs/printerkeep/sent/{printer_name}")
}

/// The reference with the last copy of the printer's branch from the repository.
pub fn fetched_ref(printer_name: &str) -> String {
    format!("refs/printerkeep/repository/{printer_name}")
}

/// What `send` did.
#[derive(Debug, PartialEq, Eq)]
pub enum Sent {
    /// The repository already has all backups. No network connection was made.
    Nothing,
    Sent(u64),
}

/// Sends the backups that the repository does not have. Never overwrites the repository.
pub fn send(git: &Git, remote: &Remote, printer_name: &str) -> Result<Sent, RepositoryError> {
    let branch = format!("refs/heads/{printer_name}");
    let Some(local) = git.resolve(&branch).map_err(RepositoryError::Git)? else {
        return Ok(Sent::Nothing);
    };
    let sent_ref = sent_ref(printer_name);
    let sent = git.resolve(&sent_ref).map_err(RepositoryError::Git)?;
    if sent.as_deref() == Some(local.as_str()) {
        return Ok(Sent::Nothing);
    }
    let count = match &sent {
        Some(sent)
            if git
                .is_ancestor(sent, &local)
                .map_err(RepositoryError::Git)? =>
        {
            git.count_between(sent, &local)
        }
        _ => git.count_commits(&local),
    }
    .map_err(RepositoryError::Git)?;

    if let Err(error) = git.push(remote, &branch, printer_name) {
        let error = classify(error, &remote.url);
        if matches!(error, RepositoryError::RepositoryAhead) {
            // Keep a copy for later (release 0.2 merges it). Errors here do not matter.
            let _ = git.fetch(remote, printer_name, &fetched_ref(printer_name));
        }
        return Err(error);
    }
    git.set_ref(&sent_ref, &local, "printerkeep: sent")
        .map_err(RepositoryError::Git)?;
    Ok(Sent::Sent(count))
}

/// How the printer's branch relates to the repository's branch after `continue_history`.
#[derive(Debug, PartialEq, Eq)]
pub enum History {
    /// The repository has no branch for this printer.
    New,
    /// The repository's branch has this printer's backups (or more). Nothing changed.
    Same,
    /// printerkeep continues the repository's history. Backups that were only on this printer are
    /// kept in `kept` (a local reference), if there were any.
    Continued { kept: Option<String> },
    /// The repository's branch had no configuration (only a README or a license). Both histories are joined.
    Joined,
    /// The repository's branch was from klipper-backup. Both histories are joined.
    KlipperBackup,
}

/// Keeps `commit` (backups that were only on this printer) as a reference that is never sent, and returns it.
fn keep_aside(git: &Git, printer_name: &str, commit: &str) -> Result<String, GitError> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let kept = format!("refs/printerkeep/kept/{printer_name}-{seconds}");
    git.set_ref(
        &kept,
        commit,
        "printerkeep: keep backups that were only on this printer",
    )?;
    Ok(kept)
}

/// Sets the printer's backups aside: they are kept as a reference that is never sent, and the branch is
/// removed, so that the next backup starts a new history. Returns the reference, if there were backups.
pub fn set_aside(git: &Git, printer_name: &str) -> Result<Option<String>, GitError> {
    let branch = format!("refs/heads/{printer_name}");
    let Some(local) = git.resolve(&branch)? else {
        return Ok(None);
    };
    let kept = keep_aside(git, printer_name, &local)?;
    git.delete_ref(&branch)?;
    let _ = git.delete_ref(&sent_ref(printer_name));
    Ok(Some(kept))
}

/// Moves the printer's local history (its branch, and the sent and fetched references) to a new name, and
/// points HEAD to it. Changes nothing in the repository.
pub fn move_local(git: &Git, old: &str, new: &str) -> Result<(), GitError> {
    for (from, to) in [
        (format!("refs/heads/{old}"), format!("refs/heads/{new}")),
        (sent_ref(old), sent_ref(new)),
        (fetched_ref(old), fetched_ref(new)),
    ] {
        if let Some(commit) = git.resolve(&from)? {
            git.set_ref(&to, &commit, "printerkeep: rename")?;
            git.delete_ref(&from)?;
        }
    }
    git.set_head(new)
}

/// Makes the printer's branch continue the repository's branch `remote_commit` (already fetched).
pub fn continue_history(
    git: &Git,
    printer_name: &str,
    remote_commit: &str,
) -> Result<History, GitError> {
    let branch = format!("refs/heads/{printer_name}");
    let history = match git.resolve(&branch)? {
        None => {
            git.update_ref(
                &branch,
                remote_commit,
                None,
                "printerkeep: continue the repository's history",
            )?;
            History::Continued { kept: None }
        }
        Some(local) if git.is_ancestor(remote_commit, &local)? => History::Same,
        Some(local) => {
            let kept = if git.is_ancestor(&local, remote_commit)? {
                None
            } else {
                Some(keep_aside(git, printer_name, &local)?)
            };
            git.update_ref(
                &branch,
                remote_commit,
                Some(&local),
                "printerkeep: continue the repository's history",
            )?;
            History::Continued { kept }
        }
    };
    git.set_ref(
        &sent_ref(printer_name),
        remote_commit,
        "printerkeep: in the repository",
    )?;
    Ok(history)
}

/// What a restore writes, compared with the config folder.
#[derive(Debug, PartialEq, Eq)]
pub struct RestorePlan {
    /// Files to write (new or different).
    pub write: Vec<Vec<u8>>,
    /// Files that exist and are different. Restore writes them only with --overwrite.
    pub different: Vec<String>,
    /// Files that are already the same.
    pub same: usize,
}

/// Compares the config folder with the files to restore.
pub fn plan_restore(git: &Git, tree: &Tree, config: &Path) -> Result<RestorePlan, RepositoryError> {
    let mut plan = RestorePlan {
        write: Vec::new(),
        different: Vec::new(),
        same: 0,
    };
    for (path, entry) in tree {
        // printerkeep's README.md is not in the config folder.
        if path == crate::branch_readme::README_FILE.as_bytes()
            && crate::branch_readme::is_generated(
                &git.blob(&entry.oid).map_err(RepositoryError::Git)?,
            )
        {
            continue;
        }
        let disk = config.join(OsStr::from_bytes(path));
        let is_regular_file =
            std::fs::symlink_metadata(&disk).is_ok_and(|m| m.file_type().is_file());
        let exists = std::fs::symlink_metadata(&disk).is_ok();
        if is_regular_file {
            let content = std::fs::read(&disk).map_err(|source| RepositoryError::Io {
                context: format!("Cannot read {}", disk.display()),
                source,
            })?;
            let blob = git.blob(&entry.oid).map_err(RepositoryError::Git)?;
            if content == blob {
                plan.same += 1;
                continue;
            }
        }
        if exists {
            plan.different
                .push(String::from_utf8_lossy(path).into_owned());
        }
        plan.write.push(path.clone());
    }
    Ok(plan)
}

/// Writes the planned files into the config folder. Each file is replaced in one step.
pub fn write_files(
    git: &Git,
    tree: &Tree,
    config: &Path,
    files: &[Vec<u8>],
) -> Result<(), RepositoryError> {
    for path in files {
        let entry = &tree[path];
        let disk = config.join(OsStr::from_bytes(path));
        let io = |source| RepositoryError::Io {
            context: format!("Cannot write {}", disk.display()),
            source,
        };
        let content = git.blob(&entry.oid).map_err(RepositoryError::Git)?;
        let mode = if entry.mode == "100755" { 0o755 } else { 0o644 };
        crate::files::replace(&disk, &content, Some(mode)).map_err(io)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_main_line_of_a_git_error() {
        let stderr = "fatal: '/srv/printers.git' does not appear to be a git repository\n\
                      fatal: Could not read from remote repository.\n\n\
                      Please make sure you have the correct access rights\nand the repository exists.\n";
        assert_eq!(
            main_message(stderr),
            "'/srv/printers.git' does not appear to be a git repository"
        );
        assert_eq!(
            main_message("ssh: connect to host github.com port 22: Network is unreachable\n"),
            "ssh: connect to host github.com port 22: Network is unreachable"
        );
    }

    #[test]
    fn url_kinds() {
        assert_eq!(
            url_kind("https://github.com/me/printers.git").unwrap(),
            UrlKind::Https
        );
        assert_eq!(
            url_kind("https://me@github.com/me/printers.git").unwrap(),
            UrlKind::Https
        );
        assert_eq!(
            url_kind("git@github.com:me/printers.git").unwrap(),
            UrlKind::Ssh
        );
        assert_eq!(
            url_kind("ssh://git@host:2222/printers.git").unwrap(),
            UrlKind::Ssh
        );
        assert_eq!(url_kind("/srv/git/printers.git").unwrap(), UrlKind::Folder);
        assert_eq!(
            url_kind("file:///srv/git/printers.git").unwrap(),
            UrlKind::Folder
        );
        assert!(matches!(
            url_kind("https://me:ghp_secret@github.com/me/printers.git"),
            Err(RepositoryError::TokenInUrl)
        ));
        assert!(matches!(
            url_kind("http://host/printers.git"),
            Err(RepositoryError::UnsupportedUrl(_))
        ));
        assert!(matches!(
            url_kind("printers"),
            Err(RepositoryError::UnsupportedUrl(_))
        ));
    }

    #[test]
    fn token_file_is_private_and_in_git_format() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path());
        let url = "https://github.com/me/printers.git";
        assert!(!has_token(&paths, url));
        store_new_token(&paths, url, "github_pat_A1b2").unwrap();
        // Not used until it is kept.
        assert!(!has_token(&paths, url));
        keep_new_token(&paths).unwrap();
        assert!(has_token(&paths, url));
        assert!(!has_token(&paths, "https://gitlab.com/me/printers.git"));

        let file = dir.path().join("printerkeep/credentials");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "https://printerkeep:github_pat_A1b2@github.com\n"
        );
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            std::fs::metadata(dir.path().join("printerkeep"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );

        // A discarded token does not change the stored one.
        store_new_token(&paths, url, "wrong").unwrap();
        discard_new_token(&paths);
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "https://printerkeep:github_pat_A1b2@github.com\n"
        );

        // A user name in the URL is used, and special characters are encoded.
        store_new_token(
            &paths,
            "https://me@git.example:3000/me/printers.git",
            "a/b c",
        )
        .unwrap();
        keep_new_token(&paths).unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "https://me:a%2Fb%20c@git.example:3000\n"
        );
    }

    #[test]
    fn classify_errors() {
        let failed = |stderr: &str| GitError::Failed {
            command: "push".into(),
            stderr: stderr.into(),
        };
        assert!(matches!(
            classify(failed(" ! [rejected] x -> x (fetch first)"), "u"),
            RepositoryError::RepositoryAhead
        ));
        assert!(matches!(
            classify(
                failed(
                    "remote: Invalid username or password.\nfatal: Authentication failed for 'https://github.com/x'"
                ),
                "u"
            ),
            RepositoryError::LoginFailed { .. }
        ));
        assert!(matches!(
            classify(
                failed("git@github.com: Permission denied (publickey)."),
                "u"
            ),
            RepositoryError::LoginFailed { .. }
        ));
        match classify(
            failed("fatal: unable to access 'https://x/': Could not resolve host: x"),
            "u",
        ) {
            RepositoryError::Connection { message, .. } => {
                assert_eq!(
                    message,
                    "unable to access 'https://x/': Could not resolve host: x"
                )
            }
            other => panic!("{other:?}"),
        }
    }
}
