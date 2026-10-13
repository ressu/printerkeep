//! The repository commands: add-repository, change-token, restore and rename.

use super::*;

/// What `add-repository` did.
#[derive(Debug)]
pub struct AddReport {
    pub url: String,
    pub token: TokenUse,
    pub printer_name: String,
    /// The printer's name before, if it took the name of a printer in the repository.
    pub renamed_from: Option<String>,
    /// The printer branches in the repository, before this printer was added.
    pub printers: Vec<String>,
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
        if let Some(old) = &self.renamed_from {
            lines.push(format!(
                "This printer's name is now {} (it was {old}).",
                self.printer_name
            ));
        }
        match &self.history {
            History::New => lines.push(format!(
                "The repository has no branch {} yet, so printerkeep starts it with this printer's backups.",
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
                "The branch {} in the repository has no configuration yet, so printerkeep continues from it.",
                self.printer_name
            )),
            History::Continued { kept } => {
                lines.push(format!(
                    "The repository has backups of {}, and printerkeep continues that history.",
                    self.printer_name
                ));
                if let Some(kept) = kept {
                    lines.push(format!(
                        "The backups that were only on this printer stay on it as {kept}, and aren't sent."
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

/// The questions that add-repository and restore ask on a terminal. Each returns `None` when there is no
/// terminal: then printerkeep does not guess, and the caller gets an error that names the options. A
/// question is short lines: the explanation first, the question itself on the last line.
pub trait Ask {
    /// A yes/no question. The default answer is no.
    fn confirm(&self, question: &str) -> Option<bool>;
    /// A question with a line of text as the answer.
    fn text(&self, question: &str) -> Option<String>;
}

/// What a script (or the command line) decided without questions.
#[derive(Debug, Default, Clone)]
pub struct Connect {
    /// `--printer-name`: this printer's name in the repository.
    pub printer_name: Option<String>,
    /// `--continue`: continue the repository's branch, also when its history is not this printer's.
    pub continue_history: bool,
    /// `--from-now`: for a new branch, send only a backup of the files now; set the local history aside.
    pub from_now: bool,
}

/// Names for a question, one on each line.
fn list(names: &[String]) -> String {
    names
        .iter()
        .map(|name| format!("  {name}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The printer branches in the repository (not `shared`).
fn printer_branches(git: &Git, remote: &git::Remote) -> Result<Vec<String>, Error> {
    Ok(git
        .remote_branches(remote)
        .map_err(|e| repository::classify(e, &remote.url))?
        .into_iter()
        .filter(|b| b != SHARED_BRANCH)
        .collect())
}

/// Connects the printer to a repository: finds which printer in the repository this is (asks, or uses
/// `choices`), continues that printer's branch or starts a new one, and sends the backups. `token` is called
/// only for an https:// URL without a stored token that works.
pub fn add_repository(
    paths: &Paths,
    url: &str,
    choices: &Connect,
    token: AskToken,
    ask: &dyn Ask,
) -> Result<AddReport, Error> {
    let (settings, current) = load(paths)?;
    // A name in printerkeep.conf or --printer-name is the user's choice; the default main is not.
    let chosen = choices.printer_name.is_some() || settings.printer_name.is_some();
    if let Some(name) = &choices.printer_name {
        check_name(name)?;
    }
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;

    let git = open_git(paths, &current)?;
    let connection = connect(&git, paths, url, kind, &current, token)?;
    let remote = connection.remote;
    let printers = printer_branches(&git, &remote)?;

    // Which printer in the repository is this?
    // printerkeep asks when the repository has other printers. The default name main is not a choice of the
    // user: then it always asks. A chosen name is asked about only when the repository has no branch with it.
    let others = printers.iter().any(|p| p != &current);
    let target = match &choices.printer_name {
        Some(name) => name.clone(),
        None if !others || (chosen && printers.contains(&current)) => current.clone(),
        None => {
            let question = format!(
                "The repository has these printers:\n{}\nThis printer's name is {current}{}.\n\
                 Which printer is this? Type its name (a new name makes a new branch), or press Enter to \
                 use {current}",
                list(&printers),
                if chosen { "" } else { " (the default)" }
            );
            match ask.text(&question) {
                Some(answer) if answer.trim().is_empty() => current.clone(),
                Some(answer) => {
                    let name = answer.trim().to_string();
                    check_name(&name)?;
                    name
                }
                None if chosen => current.clone(),
                None => {
                    return Err(Error::WhichPrinter {
                        url: url.to_string(),
                        printers,
                    });
                }
            }
        }
    };
    if target != current {
        repository::move_local(&git, &current, &target)?;
    }

    let history = if printers.contains(&target) {
        let fetched = repository::fetched_ref(&target);
        git.fetch(&remote, &target, &fetched)
            .map_err(|e| repository::classify(e, url))?;
        let commit = git.resolve(&fetched)?.unwrap_or_default();
        let related = match git.resolve(&branch(&target))? {
            Some(local) => git.merge_base(&local, &commit)?.is_some(),
            None => true,
        };
        if related {
            repository::continue_history(&git, &target, &commit)?
        } else if !has_configuration(&git, &commit)? {
            // A new repository with only a README or a license (for example made on GitHub with
            // "Add a README file"): join it.
            join_history(&git, paths, &target, &commit)?
        } else {
            // Another history with configuration: this printer's from before a new install, klipper-backup's,
            // or another printer's with the same name. Only the user knows which; the same question for all.
            let question = format!(
                "The branch {target} in the repository has backups that are not on this printer.\n\
                 If you continue that history, your files do not change, and the backups made on this \
                 printer are set aside.\n\
                 Continue that history on this printer?"
            );
            let yes = choices.continue_history || ask.confirm(&question).unwrap_or(false);
            if !yes {
                return Err(Error::NameInUse {
                    name: target,
                    url: url.to_string(),
                });
            }
            let continued = repository::continue_history(&git, &target, &commit)?;
            // klipper-backup still runs on the printer, if the user did not stop it: the report says so.
            if is_klipper_backup(&git, &commit)? {
                History::KlipperBackup
            } else {
                continued
            }
        }
    } else {
        // A new branch. If the printer has more than the install's first backup, the user can start the
        // history in the repository from now.
        let local_backups = match git.resolve(&branch(&target))? {
            Some(local) => git.count_commits(&local)?,
            None => 0,
        };
        let from_now = choices.from_now
            || (local_backups > 1
                && ask
                    .confirm(&format!(
                        "This printer has {local_backups} backups. You can send them all, or start the \
                         history in the repository from now: then they are set aside on this printer and not \
                         sent.\n\
                         Start the history in the repository from now?"
                    ))
                    .unwrap_or(false));
        if from_now {
            repository::set_aside(&git, &target)?;
        }
        History::New
    };

    // The URL only: never a token. The printer name, so that it stays the same.
    settings::set_options(
        &paths.settings(),
        &[("printer_name", &target), ("repository", url)],
    )?;
    let settings = settings::load(&paths.settings())?;
    let message = match history {
        History::KlipperBackup => Some("Continued the klipper-backup history"),
        _ => None,
    };
    let backup = backup_and_send(paths, &settings, &target, Run { message, ..COMMAND })?;
    Ok(AddReport {
        url: url.to_string(),
        token: connection.token,
        renamed_from: (target != current).then_some(current),
        printer_name: target,
        printers,
        history,
        backup,
    })
}

/// `true` if the commit is from klipper-backup: its README.md starts with klipper-backup's title.
fn is_klipper_backup(git: &Git, commit: &str) -> Result<bool, Error> {
    Ok(shared::content_at(git, commit, branch_readme::README_FILE)?
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
                Ok(_) => Ok(Connection {
                    remote,
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
            git.remote_branch(&remote, printer_name)
                .map_err(|e| repository::classify(e, url))?;
            Ok(Connection {
                remote,
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
            Ok(_) => {
                return Ok(Connection {
                    remote,
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
            context: "Can't read the token".into(),
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
        Ok(_) => {
            repository::keep_new_token(paths)?;
            Ok(Connection {
                remote: repository::remote(url, paths)?,
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

/// Restores the config folder from the repository: finds which printer in the repository this is (asks, or
/// uses `printer`), writes the files of its last backup, and continues its history. The backups that were
/// only on this printer (for example the install's first backup) are set aside. Never removes files.
/// Replaces different files only with `overwrite`.
pub fn restore(
    paths: &Paths,
    url: &str,
    printer: Option<&str>,
    overwrite: bool,
    token: AskToken,
    ask: &dyn Ask,
) -> Result<RestoreReport, Error> {
    let (_, current) = load(paths)?;
    let kind = repository::url_kind(url)?;
    let _lock = lock(paths)?;

    let git = open_git(paths, &current)?;
    let remote = connect(&git, paths, url, kind, &current, token)?.remote;
    let available = printer_branches(&git, &remote)?;
    let printer_name = match printer {
        Some(name) if available.iter().any(|b| b == name) => name.to_string(),
        Some(name) => {
            return Err(Error::NoSuchPrinter {
                name: name.to_string(),
                available,
            });
        }
        None if available.contains(&current) => current.clone(),
        None => match available.as_slice() {
            [] => return Err(Error::NothingToRestore),
            [one] => one.clone(),
            _ => {
                let question = format!(
                    "The repository has these printers:\n{}\nWhich printer is this? Type its name",
                    list(&available)
                );
                match ask.text(&question).map(|a| a.trim().to_string()) {
                    Some(name) if available.contains(&name) => name,
                    Some(name) => return Err(Error::NoSuchPrinter { name, available }),
                    None => return Err(Error::SelectPrinter(available)),
                }
            }
        },
    };

    let fetched = repository::fetched_ref(&printer_name);
    git.fetch(&remote, &printer_name, &fetched)
        .map_err(|e| repository::classify(e, url))?;
    let commit = git.resolve(&fetched)?.unwrap_or_default();
    let tree = git.read_tree(&commit)?;

    std::fs::create_dir_all(&paths.config).map_err(|source| Error::Io {
        context: format!("Can't make the config folder {}", paths.config.display()),
        source,
    })?;
    let plan = repository::plan_restore(&git, &tree, &paths.config)?;
    if !plan.different.is_empty() && !overwrite {
        return Err(Error::RestoreWouldOverwrite(plan.different));
    }
    repository::write_files(&git, &tree, &paths.config, &plan.write)?;
    // The local history (for example the install's first backup of the default configuration) goes to the
    // restored name, and is set aside when the history continues.
    if printer_name != current {
        repository::move_local(&git, &current, &printer_name)?;
    }
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
                return Err(Error::NameTaken {
                    name: new.to_string(),
                    url: url.clone(),
                });
            }
        }
        repository::move_local(&git, &old, new)?;
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
