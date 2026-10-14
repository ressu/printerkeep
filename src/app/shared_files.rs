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

/// Checks each item with `check`. With several items, all problems come back together in one error, so that
/// the user sees them all; the caller changes nothing then.
fn check_all<I, T>(
    items: &[I],
    mut check: impl FnMut(&I) -> Result<T, Error>,
) -> Result<Vec<T>, Error> {
    let mut checked = Vec::new();
    let mut errors = Vec::new();
    for item in items {
        match check(item) {
            Ok(value) => checked.push(value),
            Err(error) => errors.push(error),
        }
    }
    match errors.len() {
        0 => Ok(checked),
        1 if items.len() == 1 => Err(errors.remove(0)),
        _ => Err(Error::Several(errors)),
    }
}

/// The paths in the config folder for the files of the command line, each once.
fn config_paths(
    paths: &Paths,
    files: &[String],
    mut check: impl FnMut(&str) -> Result<(), Error>,
) -> Result<Vec<String>, Error> {
    let mut found = check_all(files, |file| {
        let path = config_path(paths, file)?;
        check(&path)?;
        Ok(path)
    })?;
    let mut seen = std::collections::BTreeSet::new();
    found.retain(|path| seen.insert(path.clone()));
    Ok(found)
}

/// An error if the path is a symlink: printerkeep never backs up or writes through a symlink.
fn not_a_symlink(paths: &Paths, path: &str) -> Result<(), Error> {
    match std::fs::symlink_metadata(paths.config.join(path)) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::Symlink(path.to_string())),
        _ => Ok(()),
    }
}

/// Starts to follow shared files. If a file is on the printer, its differences become local changes. If
/// not, printerkeep writes the shared version.
pub fn follow(paths: &Paths, files: &[String]) -> Result<Vec<String>, Error> {
    let (settings, printer_name, remote) = shared_setup(paths)?;
    let _lock = lock(paths)?;
    let git = open_git(paths, &printer_name)?;
    let shared_tree = match shared::fetch(&git, &remote)? {
        Some(tip) => git.read_tree(&tip)?,
        None => git::Tree::new(),
    };
    let names = config_paths(paths, files, |path| {
        not_a_symlink(paths, path)?;
        if !settings.follow.iter().any(|f| f == path) && !shared_tree.contains_key(path.as_bytes())
        {
            return Err(Error::NotInShared(path.to_string()));
        }
        Ok(())
    })?;
    let mut lines = Vec::new();
    let mut added = false;
    for path in &names {
        if settings.follow.contains(path) {
            lines.push(format!("printerkeep already follows {path}."));
        } else {
            settings::add_to_list(&paths.settings(), "follow", path)?;
            added = true;
        }
    }
    if added {
        let settings = settings::load(&paths.settings())?;
        lines.extend(backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines());
    }
    Ok(lines)
}

/// Stops following shared files. The files stay on the printer.
pub fn stop_following(paths: &Paths, files: &[String]) -> Result<Vec<String>, Error> {
    let (settings, printer_name) = load(paths)?;
    let _lock = lock(paths)?;
    let names = config_paths(paths, files, |path| {
        if settings.follow.iter().any(|f| f == path) {
            Ok(())
        } else {
            Err(Error::NotFollowed(path.to_string()))
        }
    })?;
    for path in &names {
        settings::remove_from_list(&paths.settings(), "follow", path)?;
    }
    let settings = settings::load(&paths.settings())?;
    let mut lines = backup_and_send(paths, &settings, &printer_name, COMMAND)?.lines();
    for path in &names {
        if std::fs::read(paths.config.join(path)).is_ok_and(|c| crate::merge::has_block(&c)) {
            lines.push(format!(
                "{path} still has #### lines from a needs decision. Keep the lines that you want, and \
                 delete the #### lines."
            ));
        }
    }
    Ok(lines)
}

/// Shows the changes that a copy to shared sends (the text), and returns `true` if the user wants them sent.
pub type Confirm<'a> = &'a dyn Fn(&str) -> std::io::Result<bool>;

/// A file to copy to shared: its path, content on this printer, and mode.
struct ToCopy {
    path: String,
    content: Vec<u8>,
    executable: bool,
}

/// A file in the copy: what goes to shared, and whether shared had something different.
struct Copied {
    path: String,
    for_shared: overrides::ForShared,
    changed: bool,
}

/// Makes the shared version of files equal to this printer's version, and follows them. `confirm` gets the
/// changes that go to the other printers first, for all files at once. All files go to shared in one commit.
pub fn copy_to_shared(
    paths: &Paths,
    files: &[String],
    confirm: Confirm,
) -> Result<Vec<String>, Error> {
    let (settings, printer_name, remote) = shared_setup(paths)?;
    let names = config_paths(paths, files, |path| {
        not_a_symlink(paths, path)?;
        if !paths.config.join(path).is_file() {
            return Err(Error::NoSuchFile(path.to_string()));
        }
        Ok(())
    })?;
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
    let sources = check_all(&names, |path| {
        let disk = paths.config.join(path);
        let io = |source| Error::Io {
            context: format!("Can't read {}", disk.display()),
            source,
        };
        let content = std::fs::read(&disk).map_err(io)?;
        if state
            .files
            .get(path)
            .is_some_and(|f| f.needs_decision.is_some())
            || crate::merge::has_block(&content)
        {
            return Err(Error::FileNeedsDecision(path.clone()));
        }
        let executable = std::fs::metadata(&disk).map_err(io)?.permissions().mode() & 0o111 != 0;
        Ok(ToCopy {
            path: path.clone(),
            content,
            executable,
        })
    })?;

    let git = Git::new(&paths.git_dir);
    let all = names.join(", ");
    let mut attempts = 0;
    let (commit, copied) = loop {
        attempts += 1;
        let tip = shared::fetch(&git, &remote)?;
        let shared_tree = match &tip {
            Some(tip) => git.read_tree(tip)?,
            None => git::Tree::new(),
        };
        let mut copied = Vec::new();
        let mut entries = Vec::new();
        let mut previews = Vec::new();
        for source in &sources {
            let path = &source.path;
            // A followed file must have all changes from shared, else the copy removes changes of other
            // printers.
            if let (Some(entry), Some(tip)) = (state.files.get(path), &tip) {
                let base = shared::content_at(&git, &entry.base, path)?;
                let now = shared::content_at(&git, tip, path)?;
                if base.map(|b| crate::merge::to_lf(&b)) != now.map(|n| crate::merge::to_lf(&n)) {
                    return Err(Error::SharedChanged(path.clone()));
                }
            }
            // Override lines stay on this printer (C6).
            let in_shared = match &tip {
                Some(tip) => shared::content_at(&git, tip, path)?,
                None => None,
            };
            let for_shared = overrides::for_shared(&source.content, in_shared.as_deref());
            let entry = git::TreeEntry {
                mode: if source.executable {
                    "100755"
                } else {
                    "100644"
                }
                .to_string(),
                oid: git.hash_blob(&for_shared.content, true)?,
            };
            let old_oid = shared_tree.get(path.as_bytes()).map(|e| e.oid.clone());
            let changed = old_oid.as_deref() != Some(entry.oid.as_str());
            if changed {
                previews.push(copy_preview(
                    &git,
                    path,
                    old_oid.as_deref(),
                    &entry.oid,
                    &for_shared,
                )?);
            }
            entries.push((path.as_str(), entry));
            copied.push(Copied {
                path: path.clone(),
                for_shared,
                changed,
            });
        }
        // The changes that go to the other printers: the user sees them before they are sent (C6).
        if attempts == 1 && !previews.is_empty() {
            let yes = confirm(&previews.concat()).map_err(|source| Error::Io {
                context: "Can't copy to shared".into(),
                source,
            })?;
            if !yes {
                return Ok(vec![match names.as_slice() {
                    [one] => format!("Nothing copied: {one} in shared stays as it was."),
                    _ => format!("Nothing copied: {all} in shared stay as they were."),
                }]);
            }
        }
        // Only the files that change go into the commit and its message.
        let changed: Vec<&str> = copied
            .iter()
            .filter(|file| file.changed)
            .map(|file| file.path.as_str())
            .collect();
        entries.retain(|(path, _)| changed.contains(path));
        let message = format!("Copied {} from {printer_name}", changed.join(", "));
        // The README of shared, with a copy: when shared has none, or printerkeep's README is not up to date.
        let shared_readme = branch_readme::shared_content(remote.url.as_str());
        let readme_in_shared = match &tip {
            Some(tip) => shared::content_at(&git, tip, branch_readme::README_FILE)?,
            None => None,
        };
        if !changed.is_empty()
            && branch_readme::shared_needs_write(readme_in_shared.as_deref(), &shared_readme)
        {
            entries.push((
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
            entries,
            &message,
            &identity(&printer_name),
        )?;
        let Some(new) = new else {
            break (tip.unwrap_or_default(), copied);
        };
        match git.push(&remote, &new, SHARED_BRANCH) {
            Ok(()) => {
                git.set_ref(SHARED_REF, &new, "printerkeep: copied to shared")?;
                break (new, copied);
            }
            Err(error) => match repository::classify(error, &remote.url) {
                // Another printer changed shared at the same time. Try again on its version.
                RepositoryError::RepositoryAhead if attempts < 3 => continue,
                error => return Err(error.into()),
            },
        }
    };

    for path in &names {
        settings::add_to_list(&paths.settings(), "follow", path)?;
        state.files.insert(
            path.clone(),
            FileState {
                base: commit.clone(),
                needs_decision: None,
            },
        );
    }
    state::save(&state_file, &state)?;
    let settings = settings::load(&paths.settings())?;
    let summary = format!("Copied {all} to shared");
    let report = backup_and_send(
        paths,
        &settings,
        &printer_name,
        Run {
            message: backup::Message::Replace(&summary),
            ..COMMAND
        },
    )?;
    lines.retain(|line| line != "No files changed. No backup made.");
    // With several files, each override line names its file.
    let prefix = |path: &str| {
        if names.len() > 1 {
            format!("{path}: ")
        } else {
            String::new()
        }
    };
    for file in &copied {
        let path = &file.path;
        if file.changed {
            lines.push(format!(
                "Copied {path} to shared. Other printers can now follow it."
            ));
        } else {
            lines.push(format!(
                "{path} in shared is already the same as on this printer."
            ));
        }
        for key in &file.for_shared.kept {
            lines.push(format!(
                "{}Kept on this printer: {key} (override). Shared keeps its own line.",
                prefix(path)
            ));
        }
        for key in &file.for_shared.placeholders {
            lines.push(format!(
                "{}Kept on this printer: {key} (override). Shared has the value {} there, and each \
                 printer that follows the file sets its own value.",
                prefix(path),
                overrides::PLACEHOLDER
            ));
        }
    }
    lines.extend(
        report.lines().into_iter().filter(|line| {
            !line.starts_with("Backup made:") && !line.starts_with("No files changed")
        }),
    );
    Ok(lines)
}

/// Finishes needs-decision files without editing them: keeps this printer's lines, or uses the shared
/// lines. The same choice for each file.
pub fn decide(paths: &Paths, files: &[String], use_shared: bool) -> Result<Vec<String>, Error> {
    let (settings, printer_name) = load(paths)?;
    let _lock = lock(paths)?;
    let names = config_paths(paths, files, |_| Ok(()))?;
    let decided = check_all(&names, |path| {
        let disk = paths.config.join(path);
        let content = std::fs::read(&disk).map_err(|source| Error::Io {
            context: format!("Can't read {}", disk.display()),
            source,
        })?;
        if !crate::merge::has_block(&content) {
            return Err(Error::NoDecision(path.clone()));
        }
        let decided = crate::merge::decide(&content, use_shared)
            .ok_or_else(|| Error::BlockChanged(path.clone()))?;
        Ok((path.clone(), disk, decided))
    })?;
    let which = if use_shared {
        "used the shared lines"
    } else {
        "kept the lines of this printer"
    };
    let mut lines = Vec::new();
    for (path, disk, content) in &decided {
        crate::files::replace(disk, content, None).map_err(|source| Error::Io {
            context: format!("Can't write {}", disk.display()),
            source,
        })?;
        lines.push(format!("{path}: {which}."));
    }
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
