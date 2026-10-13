//! Changes in the repository on the printer's own branch (M6), for example an edit in the web pages of the
//! git host. printerkeep merges each changed file with the file on the printer, as for changes from shared
//! (C4), and then joins the two histories with a merge commit. It never removes a file from the config
//! folder, and never writes its own state file from the repository.

use crate::branch_readme;
use crate::git::Git;
use crate::ignore::IgnoreRules;
use crate::merge::{self, Merged, Source};
use crate::paths::Paths;
use crate::shared::{self, SharedError};
use crate::state::STATE_FILE;
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The file had no changes on the printer, or was new. printerkeep wrote the repository's version.
    Written,
    /// printerkeep merged the repository's changes with the changes on the printer.
    Merged,
    /// The changes touch the same lines.
    NeedsDecision,
    /// The repository removed the file. printerkeep does not remove files.
    RemovedThere,
    /// The repository changed the file, but the printer removed it.
    RemovedHere,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEvent {
    pub path: String,
    pub event: Event,
}

impl FileEvent {
    pub fn line(&self) -> String {
        let path = &self.path;
        match self.event {
            Event::Written => format!("{path}: wrote the changes from the repository."),
            Event::Merged => format!(
                "{path}: wrote the changes from the repository. The changes on this printer stay."
            ),
            Event::NeedsDecision => {
                format!("{path}: needs a decision. Open the file and look for the #### lines.")
            }
            Event::RemovedThere => format!(
                "{path}: removed in the repository. printerkeep does not remove files from the config \
                 folder. If you do not need it, remove it on this printer."
            ),
            Event::RemovedHere => format!(
                "{path}: changed in the repository, but removed on this printer. printerkeep did not write \
                 it."
            ),
        }
    }
}

/// The message of the merge commit.
pub fn message(events: &[FileEvent]) -> String {
    let files = |kinds: &[Event]| -> Vec<&str> {
        events
            .iter()
            .filter(|e| kinds.contains(&e.event))
            .map(|e| e.path.as_str())
            .collect()
    };
    let mut parts = Vec::new();
    let written = files(&[Event::Written, Event::Merged]);
    if !written.is_empty() {
        parts.push(format!(
            "Changes from the repository: {}",
            written.join(", ")
        ));
    }
    let decision = files(&[Event::NeedsDecision]);
    if !decision.is_empty() {
        parts.push(format!("Needs decision: {}", decision.join(", ")));
    }
    if parts.is_empty() {
        "Joined the history of the repository".to_string()
    } else {
        parts.join(". ")
    }
}

/// Writes the changes between `base` and `remote` (commits of the printer's branch) into the config folder.
pub fn apply(
    git: &Git,
    paths: &Paths,
    rules: &IgnoreRules,
    base: &str,
    remote: &str,
) -> Result<Vec<FileEvent>, SharedError> {
    let base_tree = git.read_tree(base)?;
    let remote_tree = git.read_tree(remote)?;
    let changed: BTreeSet<&Vec<u8>> = base_tree
        .keys()
        .chain(remote_tree.keys())
        .filter(|path| base_tree.get(*path) != remote_tree.get(*path))
        .collect();

    let mut events = Vec::new();
    for path_bytes in changed {
        let Ok(path) = std::str::from_utf8(path_bytes) else {
            continue;
        };
        // The state belongs to the printer. Ignored files are never in a backup.
        if path == STATE_FILE || rules.is_ignored_file(Path::new(path)) {
            continue;
        }
        let mut event = |event| {
            events.push(FileEvent {
                path: path.to_string(),
                event,
            })
        };
        let disk = paths.config.join(path);
        let mine = shared::read(&disk)?;
        let base = match base_tree.get(path_bytes) {
            Some(entry) => Some(git.blob(&entry.oid)?),
            None => None,
        };
        // printerkeep's README.md is not in the config folder.
        if path == branch_readme::README_FILE {
            let theirs = match remote_tree.get(path_bytes) {
                Some(entry) => Some(git.blob(&entry.oid)?),
                None => None,
            };
            if [&base, &theirs]
                .iter()
                .any(|c| c.as_deref().is_some_and(branch_readme::is_generated))
            {
                continue;
            }
        }
        let Some(theirs_entry) = remote_tree.get(path_bytes) else {
            if mine.is_some() {
                event(Event::RemovedThere);
            }
            continue;
        };
        let theirs = git.blob(&theirs_entry.oid)?;
        let Some(mine) = mine else {
            if base.is_none() {
                // A new file in the repository.
                shared::write(&disk, &theirs, theirs_entry)?;
                event(Event::Written);
            } else {
                event(Event::RemovedHere);
            }
            continue;
        };
        let base = base.unwrap_or_default();
        if merge::same_text(&mine, &theirs) {
            continue;
        }
        if merge::same_text(&mine, &base) {
            shared::write(
                &disk,
                &merge::with_line_ends_of(&mine, theirs),
                theirs_entry,
            )?;
            event(Event::Written);
            continue;
        }
        match merge::merge(git, &paths.data, &mine, &base, &theirs, Source::Repository)? {
            Merged::Clean(content) => {
                if content != mine {
                    shared::write(&disk, &content, theirs_entry)?;
                    event(Event::Merged);
                }
            }
            Merged::NeedsDecision(content) => {
                shared::write(&disk, &content, theirs_entry)?;
                event(Event::NeedsDecision);
            }
        }
    }
    Ok(events)
}
