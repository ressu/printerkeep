//! The commands for shared files: follow, stop-following, copy-to-shared and decide.

use super::*;

/// The settings, the printer name and the repository for a command on shared files.
fn shared_setup(paths: &Paths) -> Result<(Settings, String, git::Remote), Error> {
    let (settings, printer_name) = load(paths)?;
    let url = settings
        .repository
        .clone()
        .ok_or(Error::SharedNeedsRepository)?;
    let remote = repository::remote(&url, paths)?;
    Ok((settings, printer_name, remote))
}

/// Starts to follow a shared file. If the file is on the printer, its differences become local changes.
/// If not, printerkeep writes the shared version.
pub fn follow(paths: &Paths, file: &str) -> Result<Vec<String>, Error> {
    let (settings, printer_name, remote) = shared_setup(paths)?;
    let path = config_path(paths, file)?;
    let _lock = lock(paths)?;
    if settings.follow.contains(&path) {
        return Ok(vec![format!("printerkeep already follows {path}.")]);
    }
    let git = open_git(paths, &printer_name)?;
    let in_shared = match shared::fetch(&git, &remote)? {
        Some(tip) => git.read_tree(&tip)?.contains_key(path.as_bytes()),
        None => false,
    };
    if !in_shared {
        return Err(Error::NotInShared(path));
    }
    settings::add_to_list(&paths.settings(), "follow", &path)?;
    let settings = settings::load(&paths.settings())?;
    Ok(backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines())
}

/// Stops following a shared file. The file stays on the printer.
pub fn stop_following(paths: &Paths, file: &str) -> Result<Vec<String>, Error> {
    let (_, printer_name) = load(paths)?;
    let path = config_path(paths, file)?;
    let _lock = lock(paths)?;
    if !settings::remove_from_list(&paths.settings(), "follow", &path)? {
        return Err(Error::NotFollowed(path));
    }
    let settings = settings::load(&paths.settings())?;
    let mut lines = backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines();
    if std::fs::read(paths.config.join(&path)).is_ok_and(|c| crate::merge::has_block(&c)) {
        lines.push(format!(
            "{path} still has #### lines from a needs decision. Keep the lines that you want, and delete \
             the #### lines."
        ));
    }
    Ok(lines)
}

/// Shows the changes that a copy to shared sends (the text), and returns `true` if the user wants them sent.
pub type Confirm<'a> = &'a dyn Fn(&str) -> std::io::Result<bool>;

/// Makes the shared version of a file equal to this printer's version, and follows the file. `confirm` gets
/// the changes that go to the other printers first.
pub fn copy_to_shared(paths: &Paths, file: &str, confirm: Confirm) -> Result<Vec<String>, Error> {
    let (settings, printer_name, remote) = shared_setup(paths)?;
    let path = config_path(paths, file)?;
    let disk = paths.config.join(&path);
    if !disk.is_file() {
        return Err(Error::NoSuchFile(path));
    }
    let _lock = lock(paths)?;

    // First the normal run: a backup, and the changes from shared that this printer does not have yet.
    let mut lines = backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            no_send: true,
            ..COMMAND
        },
    )?
    .lines();
    let state_file = paths.config.join(STATE_FILE);
    let mut state = state::load(&state_file)?;
    let io = |source| Error::Io {
        context: format!("Can't read {}", disk.display()),
        source,
    };
    let content = std::fs::read(&disk).map_err(io)?;
    if state
        .files
        .get(&path)
        .is_some_and(|f| f.needs_decision.is_some())
        || crate::merge::has_block(&content)
    {
        return Err(Error::FileNeedsDecision(path));
    }
    let executable = std::fs::metadata(&disk).map_err(io)?.permissions().mode() & 0o111 != 0;

    let git = Git::new(&paths.git_dir);
    let message = format!("Copied {path} from {printer_name}");
    let mut attempts = 0;
    let mut not_copied;
    let (commit, copied) = loop {
        attempts += 1;
        let tip = shared::fetch(&git, &remote)?;
        // A followed file must have all changes from shared, else the copy removes changes of other printers.
        if let (Some(entry), Some(tip)) = (state.files.get(&path), &tip) {
            let base = shared::content_at(&git, &entry.base, &path)?;
            let now = shared::content_at(&git, tip, &path)?;
            if base.map(|b| crate::merge::to_lf(&b)) != now.map(|n| crate::merge::to_lf(&n)) {
                return Err(Error::SharedChanged(path));
            }
        }
        // Override lines stay on this printer (C6).
        let in_shared = match &tip {
            Some(tip) => shared::content_at(&git, tip, &path)?,
            None => None,
        };
        let for_shared = overrides::for_shared(&content, in_shared.as_deref());
        let entry = git::TreeEntry {
            mode: if executable { "100755" } else { "100644" }.to_string(),
            oid: git.hash_blob(&for_shared.content, true)?,
        };
        // The changes that go to the other printers: the user sees them before they are sent (C6).
        let old_oid = match &tip {
            Some(tip) => git
                .read_tree(tip)?
                .get(path.as_bytes())
                .map(|e| e.oid.clone()),
            None => None,
        };
        if attempts == 1 && old_oid.as_deref() != Some(entry.oid.as_str()) {
            let preview = copy_preview(&git, &path, old_oid.as_deref(), &entry.oid, &for_shared)?;
            let yes = confirm(&preview).map_err(|source| Error::Io {
                context: "Can't copy to shared".into(),
                source,
            })?;
            if !yes {
                return Ok(vec![format!(
                    "Nothing copied: {path} in shared stays as it was."
                )]);
            }
        }
        not_copied = for_shared;
        let mut files = vec![(path.as_str(), entry)];
        // The README of shared: when shared has none, or printerkeep's README is not up to date.
        let shared_readme = branch_readme::shared_content(remote.url.as_str());
        let in_shared = match &tip {
            Some(tip) => shared::content_at(&git, tip, branch_readme::README_FILE)?,
            None => None,
        };
        if branch_readme::shared_needs_write(in_shared.as_deref(), &shared_readme) {
            files.push((
                branch_readme::README_FILE,
                git::TreeEntry {
                    mode: "100644".to_string(),
                    oid: git.hash_blob(&shared_readme, true)?,
                },
            ));
        }
        let new = shared::copy_commit(
            &git,
            paths,
            tip.as_deref(),
            files,
            &message,
            &identity(&printer_name),
        )?;
        let Some(new) = new else {
            break (tip.unwrap_or_default(), false);
        };
        match git.push(&remote, &new, SHARED_BRANCH) {
            Ok(()) => {
                git.set_ref(SHARED_REF, &new, "printerkeep: copied to shared")?;
                break (new, true);
            }
            Err(error) => match repository::classify(error, &remote.url) {
                // Another printer changed shared at the same time. Try again on its version.
                RepositoryError::RepositoryAhead if attempts < 3 => continue,
                error => return Err(error.into()),
            },
        }
    };

    settings::add_to_list(&paths.settings(), "follow", &path)?;
    state.files.insert(
        path.clone(),
        FileState {
            base: commit,
            needs_decision: None,
        },
    );
    state::save(&state_file, &state)?;
    let settings = settings::load(&paths.settings())?;
    let summary = format!("Copied {path} to shared");
    let report = backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            message: Some(&summary),
            ..COMMAND
        },
    )?;
    lines.retain(|line| line != "No files changed. No backup made.");
    if copied {
        lines.push(format!(
            "Copied {path} to shared. Other printers can now follow it."
        ));
    } else {
        lines.push(format!(
            "{path} in shared is already the same as on this printer."
        ));
    }
    for key in &not_copied.kept {
        lines.push(format!(
            "Kept on this printer: {key} (override). Shared keeps its own line."
        ));
    }
    for key in &not_copied.placeholders {
        lines.push(format!(
            "Kept on this printer: {key} (override). Shared has the value {} there, and each printer that \
             follows the file sets its own value.",
            overrides::PLACEHOLDER
        ));
    }
    lines.extend(
        report.lines().into_iter().filter(|line| {
            !line.starts_with("Backup made:") && !line.starts_with("No files changed")
        }),
    );
    Ok(lines)
}

/// Finishes a needs-decision file without editing it: keeps this printer's lines, or uses the shared lines.
pub fn decide(paths: &Paths, file: &str, use_shared: bool) -> Result<Vec<String>, Error> {
    let (settings, printer_name) = load(paths)?;
    let path = config_path(paths, file)?;
    let _lock = lock(paths)?;
    let disk = paths.config.join(&path);
    let content = std::fs::read(&disk).map_err(|source| Error::Io {
        context: format!("Can't read {}", disk.display()),
        source,
    })?;
    if !crate::merge::has_block(&content) {
        return Err(Error::NoDecision(path));
    }
    let decided = crate::merge::decide(&content, use_shared)
        .ok_or_else(|| Error::BlockChanged(path.clone()))?;
    crate::files::replace(&disk, &decided, None).map_err(|source| Error::Io {
        context: format!("Can't write {}", disk.display()),
        source,
    })?;
    let which = if use_shared {
        "used the shared lines"
    } else {
        "kept the lines of this printer"
    };
    let mut lines = vec![format!("{path}: {which}.")];
    lines.extend(backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines());
    Ok(lines)
}

/// The text that `copy-to-shared` shows before it sends: the changes in shared, and the override lines that
/// stay on this printer.
fn copy_preview(
    git: &Git,
    path: &str,
    old: Option<&str>,
    new: &str,
    for_shared: &overrides::ForShared,
) -> Result<String, Error> {
    let mut text = match old {
        Some(old) => format!(
            "These changes go to shared. Each printer that follows {path} gets them at its next run:\n\n{}",
            git.diff_blobs(old, new)?
        ),
        None => {
            let lines = git.blob(new)?.iter().filter(|&&b| b == b'\n').count();
            format!("{path} isn't in shared yet. It goes to shared with {lines} lines.\n")
        }
    };
    for key in &for_shared.kept {
        text.push_str(&format!(
            "Kept on this printer: {key} (override). Shared keeps its own line.\n"
        ));
    }
    for key in &for_shared.placeholders {
        text.push_str(&format!(
            "Kept on this printer: {key} (override). Shared gets the value {}.\n",
            overrides::PLACEHOLDER
        ));
    }
    Ok(text)
}
