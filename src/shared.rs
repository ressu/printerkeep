//! Shared files: follow, stop following, copy to shared, and changes from shared (C3, C4, C5).
//!
//! The `shared` branch in the repository has the shared files, at the same paths as in the config folder.
//! printerkeep keeps its last copy of it in `SHARED_REF`.

use crate::git::{Git, GitError, Identity, Remote, Tree, TreeEntry};
use crate::merge::{self, Merged};
use crate::overrides;
use crate::paths::{Paths, SETTINGS_FILE};
use crate::repository::{self, RepositoryError};
use crate::state::{FileState, STATE_FILE, State};
use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};

/// The name of the branch for shared files. No printer can have this name.
pub const SHARED_BRANCH: &str = "shared";

/// printerkeep's last copy of the `shared` branch.
pub const SHARED_REF: &str = "refs/printerkeep/repository/shared";

#[derive(Debug, thiserror::Error)]
pub enum SharedError {
    #[error(transparent)]
    Git(#[from] GitError),
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
}

/// What happened to one followed file in a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// printerkeep follows the file now. The file was on the printer: its differences are local changes.
    Started,
    /// printerkeep follows the file now. The file was not on the printer: printerkeep wrote the shared version.
    Added,
    /// The file that printerkeep wrote has override lines with the placeholder value.
    Placeholders(usize),
    /// The file had no local changes. printerkeep wrote the shared version.
    Written,
    /// printerkeep merged the changes from shared with the local changes.
    Merged,
    /// The changes from shared and the local changes touch the same lines.
    NeedsDecision,
    /// The file still has a needs-decision block. printerkeep waits.
    StillNeedsDecision,
    /// The user removed the needs-decision block.
    Decided,
    /// The file is not in shared.
    NotInShared,
    /// printerkeep follows the file, but it is not in the config folder.
    MissingOnPrinter,
    /// printerkeep cannot follow this path.
    Invalid(String),
    /// The file is not in the follow list any more.
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    pub path: String,
    pub event: Event,
}

impl FileEvent {
    /// The line for the terminal and the log.
    pub fn line(&self) -> String {
        let path = &self.path;
        match &self.event {
            Event::Started => format!(
                "Follows {path}. The differences on this printer are local changes. They stay."
            ),
            Event::Added => format!("Follows {path}. Wrote the shared version."),
            Event::Placeholders(count) => format!(
                "{path}: {count} override {} the value {}. Set the values for this printer.",
                if *count == 1 {
                    "line has"
                } else {
                    "lines have"
                },
                crate::overrides::PLACEHOLDER
            ),
            Event::Written => format!("{path}: wrote the changes from shared."),
            Event::Merged => {
                format!("{path}: wrote the changes from shared. The local changes stay.")
            }
            Event::NeedsDecision | Event::StillNeedsDecision => {
                format!("{path}: needs decision. Open the file and look for the #### lines.")
            }
            Event::Decided => format!("{path}: decision recorded."),
            Event::NotInShared => format!(
                "{path}: not in shared. To add it to shared: printerkeep copy-to-shared {path}"
            ),
            Event::MissingOnPrinter => format!(
                "{path}: printerkeep follows it, but it is not in the config folder. \
                 To stop following it: printerkeep stop-following {path}"
            ),
            Event::Invalid(reason) => format!("{path}: printerkeep cannot follow it: {reason}"),
            Event::Stopped => format!("Stopped following {path}."),
        }
    }
}

/// The message of the backup that has the results of a run, or `None` if nothing changed.
pub fn message(events: &[FileEvent]) -> Option<String> {
    let groups: [(&str, &[Event]); 5] = [
        (
            "Changes from shared",
            &[Event::Written, Event::Merged, Event::Added],
        ),
        ("Needs decision", &[Event::NeedsDecision]),
        ("Decided", &[Event::Decided]),
        ("Follows", &[Event::Started]),
        ("Stopped following", &[Event::Stopped]),
    ];
    let parts: Vec<String> = groups
        .iter()
        .filter_map(|(title, kinds)| {
            let files: Vec<&str> = events
                .iter()
                .filter(|e| kinds.contains(&e.event))
                .map(|e| e.path.as_str())
                .collect();
            (!files.is_empty()).then(|| format!("{title}: {}", files.join(", ")))
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join(". "))
}

/// Gets the `shared` branch from the repository. Returns its commit, or `None` if it does not exist.
pub fn fetch(git: &Git, remote: &Remote) -> Result<Option<String>, RepositoryError> {
    let exists = git
        .fetch_if_exists(remote, SHARED_BRANCH, SHARED_REF)
        .map_err(|e| repository::classify(e, &remote.url))?;
    if exists {
        git.resolve(SHARED_REF).map_err(RepositoryError::Git)
    } else {
        Ok(None)
    }
}

/// The reason why printerkeep cannot follow or copy `path`, if there is one.
pub fn check_path(path: &str) -> Result<(), String> {
    let relative = Path::new(path);
    if path.is_empty() || relative.is_absolute() {
        return Err("use a path in the config folder, for example macros/macros.cfg".into());
    }
    if !relative
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
    {
        return Err("use a path in the config folder, without '..' or '.'".into());
    }
    if path == crate::readme::README_FILE {
        return Err("README.md on the shared branch tells what the repository is".into());
    }
    if path == SETTINGS_FILE || path == STATE_FILE {
        return Err("this file belongs to this printer only".into());
    }
    Ok(())
}

/// Applies changes from shared to the followed files (C4), and records decisions (C5). Writes files into the
/// config folder only when the result is final. Changes `state`; the caller saves it.
pub fn apply(
    git: &Git,
    paths: &Paths,
    follow: &[String],
    tip: Option<&str>,
    state: &mut State,
) -> Result<Vec<FileEvent>, SharedError> {
    let mut events = Vec::new();
    let mut event = |path: &str, event| {
        events.push(FileEvent {
            path: path.to_string(),
            event,
        })
    };

    let followed: BTreeSet<&str> = follow.iter().map(String::as_str).collect();
    let stopped: Vec<String> = state
        .files
        .keys()
        .filter(|path| !followed.contains(path.as_str()))
        .cloned()
        .collect();
    for path in stopped {
        state.files.remove(&path);
        event(&path, Event::Stopped);
    }

    let shared_tree = match tip {
        Some(tip) => git.read_tree(tip)?,
        None => Tree::new(),
    };
    for path in follow {
        if let Err(reason) = check_path(path) {
            event(path, Event::Invalid(reason));
            continue;
        }
        let disk = paths.config.join(path);
        let mut current = read(&disk)?;

        if let Some(entry) = state.files.get_mut(path)
            && let Some(cause) = entry.needs_decision.clone()
        {
            if current.as_deref().is_some_and(merge::has_block) {
                event(path, Event::StillNeedsDecision);
                continue;
            }
            // The lines that the user kept are now the local changes.
            entry.base = cause;
            entry.needs_decision = None;
            event(path, Event::Decided);
        }

        // A block from a change in the repository (M6): the user decides first.
        if current.as_deref().is_some_and(merge::has_block) {
            event(path, Event::StillNeedsDecision);
            continue;
        }
        let (Some(tip), Some(shared_entry)) = (tip, shared_tree.get(path.as_bytes())) else {
            event(path, Event::NotInShared);
            continue;
        };
        let shared = git.blob(&shared_entry.oid)?;
        let base = match state.files.get(path) {
            Some(entry) if entry.base == tip => continue,
            Some(entry) => content_at(git, &entry.base, path)?,
            None => None,
        };
        let new_base = FileState {
            base: tip.to_string(),
            needs_decision: None,
        };

        // No base (a new follow, or shared lost the base commit): the shared version is the base.
        let Some(base) = base else {
            match current {
                Some(_) => event(path, Event::Started),
                None => {
                    write(&disk, &shared, shared_entry)?;
                    event(path, Event::Added);
                    let count = overrides::placeholders(&shared);
                    if count > 0 {
                        event(path, Event::Placeholders(count));
                    }
                }
            }
            state.files.insert(path.clone(), new_base);
            continue;
        };
        let same = |a: &[u8], b: &[u8]| merge::to_lf(a) == merge::to_lf(b);
        // The file did not change in shared (only other files changed). The base stays: a
        // new base would change the state file, and make a backup, for nothing.
        if same(&base, &shared) {
            continue;
        }
        let Some(mine) = current.take() else {
            event(path, Event::MissingOnPrinter);
            continue;
        };
        // The printer's override lines never change (C6).
        let base = overrides::protect(&mine, &merge::to_lf(&base));
        let shared = overrides::protect(&mine, &merge::to_lf(&shared));
        if same(&mine, &shared) {
            // For example, this printer copied the file to shared.
            state.files.insert(path.clone(), new_base);
        } else if same(&mine, &base) {
            let content = if merge::uses_crlf(&mine) {
                merge::to_crlf(&shared)
            } else {
                shared
            };
            write(&disk, &content, shared_entry)?;
            state.files.insert(path.clone(), new_base);
            event(path, Event::Written);
        } else {
            match merge::merge(
                git,
                &paths.data,
                &mine,
                &base,
                &shared,
                merge::Source::Shared,
            )? {
                Merged::Clean(content) => {
                    state.files.insert(path.clone(), new_base);
                    if content != mine {
                        write(&disk, &content, shared_entry)?;
                        event(path, Event::Merged);
                    }
                }
                Merged::NeedsDecision(content) => {
                    write(&disk, &content, shared_entry)?;
                    if let Some(entry) = state.files.get_mut(path) {
                        entry.needs_decision = Some(tip.to_string());
                    }
                    event(path, Event::NeedsDecision);
                }
            }
        }
    }
    Ok(events)
}

/// The status of a followed file. See the status words in `docs/naming.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileStatus {
    /// Same as the shared version (override lines excepted).
    UpToDate,
    /// Has all changes from shared, plus local changes.
    LocalChanges,
    /// Changes from shared are not applied yet.
    ChangesWaiting,
    NeedsDecision,
    /// Not in the config folder.
    Missing,
    /// Not in printerkeep's last copy of shared.
    NotInShared,
}

impl FileStatus {
    pub fn text(&self) -> &'static str {
        match self {
            FileStatus::UpToDate => "up to date",
            FileStatus::LocalChanges => "local changes",
            FileStatus::ChangesWaiting => "changes waiting. The next backup applies them",
            FileStatus::NeedsDecision => {
                "needs decision. Open the file and look for the #### lines"
            }
            FileStatus::Missing => "not in the config folder",
            FileStatus::NotInShared => "not in shared",
        }
    }
}

/// The status of a followed file, from printerkeep's last copy of shared. Uses no network.
pub fn file_status(
    git: &Git,
    paths: &Paths,
    path: &str,
    state: &State,
    tip: Option<&str>,
) -> Result<FileStatus, SharedError> {
    let entry = state.files.get(path);
    if entry.is_some_and(|e| e.needs_decision.is_some()) {
        return Ok(FileStatus::NeedsDecision);
    }
    let Some(mine) = read(&paths.config.join(path))? else {
        return Ok(FileStatus::Missing);
    };
    let Some(shared) = (match tip {
        Some(tip) => content_at(git, tip, path)?,
        None => None,
    }) else {
        return Ok(FileStatus::NotInShared);
    };
    let Some(entry) = entry else {
        // In the follow list, but printerkeep did not take it in yet.
        return Ok(FileStatus::ChangesWaiting);
    };
    let same = |a: &[u8], b: &[u8]| merge::to_lf(a) == merge::to_lf(b);
    let base = content_at(git, &entry.base, path)?.unwrap_or_default();
    if !same(&base, &shared) {
        return Ok(FileStatus::ChangesWaiting);
    }
    if same(&mine, &overrides::protect(&mine, &merge::to_lf(&shared))) {
        Ok(FileStatus::UpToDate)
    } else {
        Ok(FileStatus::LocalChanges)
    }
}

/// The content of `path` in `commit`, or `None` if the commit or the file does not exist.
pub fn content_at(git: &Git, commit: &str, path: &str) -> Result<Option<Vec<u8>>, GitError> {
    if git.resolve(commit)?.is_none() {
        return Ok(None);
    }
    match git.read_tree(commit)?.get(path.as_bytes()) {
        Some(entry) => git.blob(&entry.oid).map(Some),
        None => Ok(None),
    }
}

/// A new shared commit on `tip` with these files (path and entry). Returns `None` if shared already has the
/// first file: the others (the README of shared) go only with a change of the first.
pub fn copy_commit(
    git: &Git,
    paths: &Paths,
    tip: Option<&str>,
    files: Vec<(&str, TreeEntry)>,
    message: &str,
    identity: &Identity,
) -> Result<Option<String>, GitError> {
    let mut tree = match tip {
        Some(tip) => git.read_tree(tip)?,
        None => Tree::new(),
    };
    let Some((path, entry)) = files.first() else {
        return Ok(None);
    };
    if tree.get(path.as_bytes()) == Some(entry) {
        return Ok(None);
    }
    for (path, entry) in files {
        tree.insert(path.as_bytes().to_vec(), entry);
    }
    let tree = git.write_tree(&tree, &paths.index())?;
    git.commit(&tree, tip, message, identity).map(Some)
}

pub(crate) fn read(disk: &Path) -> Result<Option<Vec<u8>>, SharedError> {
    match std::fs::read(disk) {
        Ok(content) => Ok(Some(content)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(SharedError::Io {
            context: format!("Cannot read {}", disk.display()),
            source,
        }),
    }
}

/// Replaces the file in one step. Keeps the permissions of the file on the printer; a new file gets the
/// mode from shared.
pub(crate) fn write(disk: &Path, content: &[u8], shared: &TreeEntry) -> Result<(), SharedError> {
    if std::fs::read(disk).is_ok_and(|old| old == content) {
        return Ok(());
    }
    let io = |source| SharedError::Io {
        context: format!("Cannot write {}", disk.display()),
        source,
    };
    let folder = disk
        .parent()
        .ok_or_else(|| io(std::io::ErrorKind::NotFound.into()))?;
    std::fs::create_dir_all(folder).map_err(io)?;
    let mode = match std::fs::metadata(disk) {
        Ok(metadata) => metadata.permissions().mode() & 0o777,
        Err(_) if shared.mode == "100755" => 0o755,
        Err(_) => 0o644,
    };
    let temporary = folder.join(".printerkeep-write.tmp");
    std::fs::write(&temporary, content).map_err(io)?;
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(mode)).map_err(io)?;
    std::fs::rename(&temporary, disk).map_err(io)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_that_cannot_be_followed() {
        assert!(check_path("macros/macros.cfg").is_ok());
        assert!(check_path("").is_err());
        assert!(check_path("/etc/passwd").is_err());
        assert!(check_path("../moonraker.conf").is_err());
        assert!(check_path("./a.cfg").is_err());
        assert!(check_path("printerkeep.conf").is_err());
        assert!(check_path(".printerkeep-state.json").is_err());
    }

    #[test]
    fn messages_group_the_files() {
        let e = |path: &str, event| FileEvent {
            path: path.into(),
            event,
        };
        assert_eq!(message(&[e("a.cfg", Event::StillNeedsDecision)]), None);
        assert_eq!(
            message(&[
                e("a.cfg", Event::Written),
                e("b.cfg", Event::NeedsDecision),
                e("c.cfg", Event::Merged),
            ])
            .unwrap(),
            "Changes from shared: a.cfg, c.cfg. Needs decision: b.cfg"
        );
    }
}
