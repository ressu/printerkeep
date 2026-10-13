//! The repository commands: add-repository, change-token, restore and rename.

use super::*;

/// What `add-repository` did.
#[derive(Debug)]
pub struct AddReport {
    pub url: String,
    pub token: TokenUse,
    pub printer_name: String,
    pub history: History,
    pub backup: BackupReport,
}

impl AddReport {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!("Connected to the repository {}.", self.url)];
        match self.token {
            TokenUse::Stored => lines.push("Used the stored token.".to_string()),
            TokenUse::New => lines.push("Stored the token.".to_string()),
            TokenUse::None => {}
        }
        match &self.history {
            History::New => lines.push(format!(
                "The repository has no backups of {}. printerkeep sends the backups of this printer.",
                self.printer_name
            )),
            History::Same => {}
            History::KlipperBackup => {
                lines.push(format!(
                    "The branch {} has backups from klipper-backup. printerkeep continues that history: \
                     the files of the config folder are now at the top of the branch, and the older backups \
                     stay in the history.",
                    self.printer_name
                ));
                lines.push(
                    "Important: stop klipper-backup on this printer (its timer, services and update_manager \
                     section). If it still runs, it sends its backups to the same branch."
                        .to_string(),
                );
            }
            History::Joined => lines.push(format!(
                "The repository has no configuration on the branch {}. printerkeep joined its history.",
                self.printer_name
            )),
            History::Continued { kept } => {
                lines.push(format!(
                    "The repository has backups of {}. printerkeep continues that history.",
                    self.printer_name
                ));
                if let Some(kept) = kept {
                    lines.push(format!(
                        "The backups that were only on this printer are not sent. They stay on this \
                         printer as {kept}."
                    ));
                }
            }
        }
        lines.extend(self.backup.lines());
        lines
    }
}

/// Why printerkeep asks for a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRequest {
    /// No token is stored for the repository.
    New,
    /// The repository did not accept the stored token (for example, it expired).
    StoredRefused,
    /// The user replaces the token (`change-token`).
    Replace,
}

/// Asks the user for a token for the repository URL.
pub type AskToken<'a> = &'a dyn Fn(TokenRequest, &str) -> std::io::Result<String>;

/// Which token a connection used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenUse {
    /// Not an https:// repository.
    None,
    /// The token that was stored before.
    Stored,
    /// A new token from the user. It is stored now.
    New,
}

/// Connects the printer to a repository. `token` is called only for an https:// URL without a stored token
/// that works.
pub fn add_repository(paths: &Paths, url: &str, token: AskToken) -> Result<AddReport, Error> {
    let (_, printer_name) = load(paths)?;
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;

    let git = open_git(paths, &printer_name)?;
    let connection = connect(&git, paths, url, kind, &printer_name, token)?;
    let remote = connection.remote;

    let history = match connection.commit {
        None => History::New,
        Some(_) => {
            let fetched = repository::fetched_ref(&printer_name);
            git.fetch(&remote, &printer_name, &fetched)
                .map_err(|e| repository::classify(e, url))?;
            let commit = git.resolve(&fetched)?.unwrap_or_default();
            let local = git.resolve(&branch(&printer_name))?;
            let related = match &local {
                Some(local) => git.merge_base(local, &commit)?.is_some(),
                None => false,
            };
            if related {
                repository::continue_history(&git, &printer_name, &commit)?
            } else if is_klipper_backup(&git, &commit)? {
                // A klipper-backup branch: continue its history. The next backup is a child of its last
                // commit, so git sees printer_data/config/* moved to the top (a rename) and follows the
                // files. The printer's own backups stay on the printer as a kept reference.
                repository::continue_history(&git, &printer_name, &commit)?;
                History::KlipperBackup
            } else if has_configuration(&git, &commit)? {
                // Another printer with the same name, or a new install of this printer. printerkeep never
                // takes over that history by itself.
                return Err(Error::NameInUse {
                    name: printer_name,
                    url: url.to_string(),
                });
            } else {
                // A new repository with only a README or a license (for example made on GitHub with
                // "Add a README file"): join it.
                join_history(&git, paths, &printer_name, &commit)?
            }
        }
    };

    // The URL only: never a token. The printer name, so that it stays the same.
    settings::set_options(
        &paths.settings(),
        &[("printer_name", &printer_name), ("repository", url)],
    )?;
    let settings = settings::load(&paths.settings())?;
    let message =
        (history == History::KlipperBackup).then_some("Continued the klipper-backup history");
    let backup = backup_and_send(paths, &settings, &printer_name, Run { message, ..COMMAND })?;
    Ok(AddReport {
        url: url.to_string(),
        token: connection.token,
        printer_name,
        history,
        backup,
    })
}

/// `true` if the commit is from klipper-backup: its README.md starts with klipper-backup's title.
fn is_klipper_backup(git: &Git, commit: &str) -> Result<bool, Error> {
    Ok(shared::content_at(git, commit, readme::README_FILE)?
        .is_some_and(|content| content.starts_with(b"# klipper-backup")))
}

/// `true` if the commit has configuration files (`.cfg`), anywhere in the tree.
fn has_configuration(git: &Git, commit: &str) -> Result<bool, Error> {
    Ok(git
        .read_tree(commit)?
        .keys()
        .any(|path| path.ends_with(b".cfg")))
}

/// Joins the history of a branch without configuration (a new repository's README or license) with the
/// printer's history: a merge commit with the printer's files. The other files are not in the config folder,
/// so they are not in the backups.
fn join_history(
    git: &Git,
    paths: &Paths,
    printer_name: &str,
    remote: &str,
) -> Result<History, Error> {
    let reference = branch(printer_name);
    match git.resolve(&reference)? {
        Some(local) => {
            let tree = git.read_tree(&local)?;
            let tree = git.write_tree(&tree, &paths.index())?;
            let commit = git.commit(
                &tree,
                &[&local, remote],
                "Joined the history of the repository\n",
                &identity(printer_name),
            )?;
            git.update_ref(&reference, &commit, Some(&local), "printerkeep: join")?;
        }
        None => {
            git.update_ref(&reference, remote, None, "printerkeep: join")?;
        }
    }
    git.set_ref(
        &repository::sent_ref(printer_name),
        remote,
        "printerkeep: in the repository",
    )?;
    Ok(History::Joined)
}

/// A working connection to the repository.
struct Connection {
    remote: git::Remote,
    /// The repository's commit for the printer, if there is one.
    commit: Option<String>,
    token: TokenUse,
}

/// Connects to the repository and tests the login. For https://, printerkeep uses the stored token, and
/// asks for a new one only if there is no stored token or the repository does not accept it. For SSH,
/// printerkeep makes its key, and shows it if the repository does not accept it.
fn connect(
    git: &Git,
    paths: &Paths,
    url: &str,
    kind: UrlKind,
    printer_name: &str,
    token: AskToken,
) -> Result<Connection, Error> {
    match kind {
        UrlKind::Https => connect_with_token(git, paths, url, printer_name, token, false),
        UrlKind::Ssh => {
            repository::ensure_ssh_key(paths, printer_name)?;
            let remote = repository::remote(url, paths)?;
            match git.remote_branch(&remote, printer_name) {
                Ok(commit) => Ok(Connection {
                    remote,
                    commit,
                    token: TokenUse::None,
                }),
                Err(error) => match repository::classify(error, url) {
                    RepositoryError::LoginFailed { .. } => Err(Error::DeployKeyNeeded {
                        public_key: repository::ensure_ssh_key(paths, printer_name)?,
                    }),
                    error => Err(error.into()),
                },
            }
        }
        UrlKind::Folder => {
            let remote = repository::remote(url, paths)?;
            let commit = git
                .remote_branch(&remote, printer_name)
                .map_err(|e| repository::classify(e, url))?;
            Ok(Connection {
                remote,
                commit,
                token: TokenUse::None,
            })
        }
    }
}

/// `replace`: do not use the stored token, ask for a new one.
fn connect_with_token(
    git: &Git,
    paths: &Paths,
    url: &str,
    printer_name: &str,
    token: AskToken,
    replace: bool,
) -> Result<Connection, Error> {
    let mut request = if replace {
        TokenRequest::Replace
    } else {
        TokenRequest::New
    };
    if !replace && repository::has_token(paths, url) {
        let remote = repository::remote(url, paths)?;
        match git.remote_branch(&remote, printer_name) {
            Ok(commit) => {
                return Ok(Connection {
                    remote,
                    commit,
                    token: TokenUse::Stored,
                });
            }
            Err(error) => match repository::classify(error, url) {
                RepositoryError::LoginFailed { .. } => request = TokenRequest::StoredRefused,
                error => return Err(error.into()),
            },
        }
    }

    let new_token = token(request, url)
        .map_err(|source| Error::Io {
            context: "Cannot read the token".into(),
            source,
        })?
        .trim()
        .to_string();
    if new_token.is_empty() {
        return Err(Error::NoToken);
    }
    // The new token replaces the stored one only when the repository accepts it.
    repository::store_new_token(paths, url, &new_token)?;
    match git.remote_branch(&repository::new_token_remote(url, paths), printer_name) {
        Ok(commit) => {
            repository::keep_new_token(paths)?;
            Ok(Connection {
                remote: repository::remote(url, paths)?,
                commit,
                token: TokenUse::New,
            })
        }
        Err(error) => {
            repository::discard_new_token(paths);
            Err(match repository::classify(error, url) {
                RepositoryError::LoginFailed { .. } => Error::TokenRefused {
                    url: url.to_string(),
                },
                error => error.into(),
            })
        }
    }
}

/// Replaces the token of the printer's repository. The new token must work before it replaces the old
/// one. Then printerkeep sends the backups that are not in the repository.
pub fn change_token(paths: &Paths, token: AskToken) -> Result<Vec<String>, Error> {
    let settings = settings::load(&paths.settings())?;
    let url = settings.repository.clone().ok_or(Error::NoRepository)?;
    if repository::url_kind(&url)? != UrlKind::Https {
        return Err(Error::NoTokenForUrl(url));
    }
    let printer_name = printer_name(&settings)?;
    let _lock = lock(paths)?;
    let git = open_git(paths, &printer_name)?;
    connect_with_token(&git, paths, &url, &printer_name, token, true)?;
    let mut lines = vec![format!(
        "The repository {url} accepted the new token. printerkeep uses it from now on."
    )];
    lines.extend(backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines());
    Ok(lines)
}

/// What `restore` did.
#[derive(Debug)]
pub struct RestoreReport {
    pub printer_name: String,
    pub written: usize,
    pub same: usize,
}

impl RestoreReport {
    pub fn lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "Restored {} from the backup of {}.",
            count(self.written, "file"),
            self.printer_name
        )];
        if self.same > 0 {
            lines.push(format!(
                "{} already had the same content.",
                count(self.same, "file")
            ));
        }
        lines.push(
            "Restart Moonraker and Klipper to use the restored files: \
             sudo systemctl restart moonraker klipper"
                .to_string(),
        );
        lines
    }
}

/// Restores the config folder from the repository. Never removes files. Replaces different files only
/// with `overwrite`.
pub fn restore(
    paths: &Paths,
    url: &str,
    printer: Option<&str>,
    overwrite: bool,
    token: AskToken,
) -> Result<RestoreReport, Error> {
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;
    let key_name = printer.unwrap_or(DEFAULT_NAME).to_string();

    let git = open_git(paths, &key_name)?;
    let remote = connect(&git, paths, url, kind, &key_name, token)?.remote;

    let available: Vec<String> = git
        .remote_branches(&remote)
        .map_err(|e| repository::classify(e, url))?
        .into_iter()
        .filter(|b| b != SHARED_BRANCH)
        .collect();
    let printer_name = match printer {
        Some(name) if available.iter().any(|b| b == name) => name.to_string(),
        Some(name) => {
            return Err(Error::NoSuchPrinter {
                name: name.to_string(),
                available,
            });
        }
        None => match available.as_slice() {
            [] => return Err(Error::NothingToRestore),
            [one] => one.clone(),
            _ => return Err(Error::SelectPrinter(available)),
        },
    };

    let fetched = repository::fetched_ref(&printer_name);
    git.fetch(&remote, &printer_name, &fetched)
        .map_err(|e| repository::classify(e, url))?;
    let commit = git.resolve(&fetched)?.unwrap_or_default();
    let tree = git.read_tree(&commit)?;

    std::fs::create_dir_all(&paths.config).map_err(|source| Error::Io {
        context: format!("Cannot make the config folder {}", paths.config.display()),
        source,
    })?;
    let plan = repository::plan_restore(&git, &tree, &paths.config)?;
    if !plan.different.is_empty() && !overwrite {
        return Err(Error::RestoreWouldOverwrite(plan.different));
    }
    repository::write_files(&git, &tree, &paths.config, &plan.write)?;
    repository::continue_history(&git, &printer_name, &commit)?;
    settings::set_options(
        &paths.settings(),
        &[("printer_name", &printer_name), ("repository", url)],
    )?;

    Ok(RestoreReport {
        printer_name,
        written: plan.write.len(),
        same: plan.same,
    })
}

/// Gives the printer a new name. Its history moves to the branch with the new name. In the repository, the
/// old branch stays.
pub fn rename(paths: &Paths, new: &str) -> Result<Vec<String>, Error> {
    let settings = settings::load(&paths.settings())?;
    let old = printer_name(&settings)?;
    check_name(new)?;
    if new == old {
        return Ok(vec![format!("The printer name is already {new}.")]);
    }
    let _lock = lock(paths)?;
    let git = Git::new(&paths.git_dir);
    if git.exists() {
        if let Some(url) = &settings.repository {
            let remote = repository::remote(url, paths)?;
            if git
                .remote_branch(&remote, new)
                .map_err(|e| repository::classify(e, url))?
                .is_some()
            {
                return Err(Error::NameTaken(new.to_string()));
            }
        }
        for (from, to) in [
            (branch(&old), branch(new)),
            (repository::sent_ref(&old), repository::sent_ref(new)),
            (repository::fetched_ref(&old), repository::fetched_ref(new)),
        ] {
            if let Some(commit) = git.resolve(&from)? {
                git.set_ref(&to, &commit, "printerkeep: rename")?;
                git.delete_ref(&from)?;
            }
        }
        git.set_head(new)?;
    }
    settings::set_options(&paths.settings(), &[("printer_name", new)])?;
    let mut lines = vec![format!("Renamed the printer from {old} to {new}.")];
    if settings.repository.is_some() {
        lines.push(format!(
            "The repository keeps the branch {old}. If you do not need it, remove it in the web pages of the \
             repository."
        ));
        // The repository does not have the new branch yet: send all backups.
        let _ = git.delete_ref(&repository::sent_ref(new));
    }
    let settings = settings::load(&paths.settings())?;
    lines.extend(backup_and_send(paths, &settings, new, COMMAND)?.lines());
    Ok(lines)
}
