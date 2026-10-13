//! Backups: compare the config folder with the last backup, and make a commit only when a file changed.

use crate::git::{Git, GitError, Identity, Tree, TreeEntry};
use crate::snapshot::Scan;
use std::path::Path;

/// The changes between the last backup and the config folder. Paths are for messages.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Changes {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub removed: Vec<String>,
    /// Changed files where only the SAVE_CONFIG block changed. Also in `changed`.
    pub save_config: Vec<String>,
}

/// A backup, ready to commit.
#[derive(Debug)]
pub struct Prepared {
    /// The last backup, if there is one.
    pub parent: Option<String>,
    /// The files of the new backup.
    pub tree: Tree,
    pub changes: Changes,
}

const REGULAR: &str = "100644";
const EXECUTABLE: &str = "100755";

/// Compares the scanned files with the last backup on `reference`. With `write`, stores the files in git.
pub fn prepare(git: &Git, scan: &Scan, reference: &str, write: bool) -> Result<Prepared, GitError> {
    let parent = git.resolve(reference)?;
    let old = match &parent {
        Some(commit) => git.read_tree(commit)?,
        None => Tree::new(),
    };

    // Hash without storing. Storing a file that git already has changes its time: a write to the SD card at
    // each run. Only new and changed files are stored, below.
    let disk_paths: Vec<_> = scan.files.iter().map(|f| f.disk.clone()).collect();
    let oids = git.hash_files(&disk_paths, false)?;

    let mut tree = Tree::new();
    let mut changes = Changes::default();
    for (file, oid) in scan.files.iter().zip(oids) {
        let mode = if file.executable { EXECUTABLE } else { REGULAR };
        let mut entry = TreeEntry {
            mode: mode.to_string(),
            oid,
        };
        let name = display(&file.path);
        match old.get(&file.path) {
            None => changes.added.push(name),
            Some(previous) if previous == &entry => {}
            Some(previous) => {
                let new_content = std::fs::read(&file.disk)?;
                let old_content = git.blob(&previous.oid)?;
                if previous.oid != entry.oid && normalize(&new_content) == normalize(&old_content) {
                    // Only the line endings are different: keep the stored file.
                    entry.oid = previous.oid.clone();
                }
                if previous != &entry {
                    if previous.oid != entry.oid
                        && only_save_config_changed(&old_content, &new_content)
                    {
                        changes.save_config.push(name.clone());
                    }
                    changes.changed.push(name);
                }
            }
        }
        tree.insert(file.path.clone(), entry);
    }
    for path in old.keys() {
        if !tree.contains_key(path) {
            changes.removed.push(display(path));
        }
    }
    if write {
        let known: std::collections::BTreeSet<&str> =
            old.values().map(|entry| entry.oid.as_str()).collect();
        let new_files: Vec<_> = scan
            .files
            .iter()
            .filter(|file| {
                tree.get(&file.path)
                    .is_some_and(|entry| !known.contains(entry.oid.as_str()))
            })
            .collect();
        let disk: Vec<_> = new_files.iter().map(|file| file.disk.clone()).collect();
        // The ids of what git stored: a file can change between the two hash commands.
        for (file, oid) in new_files.iter().zip(git.hash_files(&disk, true)?) {
            if let Some(entry) = tree.get_mut(&file.path) {
                entry.oid = oid;
            }
        }
    }

    Ok(Prepared {
        parent,
        tree,
        changes,
    })
}

/// Makes the commit for `prepared`, if something changed. Returns the new commit and its message. `message`
/// replaces the message that lists the changes.
pub fn commit(
    git: &Git,
    prepared: &Prepared,
    reference: &str,
    index: &Path,
    identity: &Identity,
    message: Option<&str>,
) -> Result<Option<(String, String)>, GitError> {
    if prepared.changes.is_empty() {
        return Ok(None);
    }
    let tree = git.write_tree(&prepared.tree, index)?;
    let message = match message {
        Some(message) => format!("{message}\n"),
        None => prepared
            .changes
            .message(prepared.parent.is_none(), prepared.tree.len()),
    };
    let commit = git.commit(&tree, prepared.parent.as_deref(), &message, identity)?;
    git.update_ref(
        reference,
        &commit,
        prepared.parent.as_deref(),
        "printerkeep backup",
    )?;
    Ok(Some((commit, message)))
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }

    /// A short summary, for example "Changed printer.cfg, added 2 files".
    pub fn summary(&self) -> String {
        if !self.changed.is_empty()
            && self.changed.len() == self.save_config.len()
            && self.added.is_empty()
            && self.removed.is_empty()
        {
            return format!("SAVE_CONFIG changed {}", names(&self.save_config));
        }
        let parts: Vec<String> = [
            ("changed", &self.changed),
            ("added", &self.added),
            ("removed", &self.removed),
        ]
        .into_iter()
        .filter(|(_, files)| !files.is_empty())
        .map(|(verb, files)| format!("{verb} {}", names(files)))
        .collect();
        capitalize(&parts.join(", "))
    }

    /// The commit message: a subject line and, after a blank line, all files.
    pub fn message(&self, first: bool, file_count: usize) -> String {
        if first {
            return format!("First backup: {}", count(file_count));
        }
        let mut message = self.summary();
        message.push('\n');
        for (title, files) in [
            ("Changed", &self.changed),
            ("Added", &self.added),
            ("Removed", &self.removed),
        ] {
            if files.is_empty() {
                continue;
            }
            message.push_str(&format!("\n{title}:\n"));
            for file in files {
                if title == "Changed" && self.save_config.contains(file) {
                    message.push_str(&format!("  {file} (SAVE_CONFIG)\n"));
                } else {
                    message.push_str(&format!("  {file}\n"));
                }
            }
        }
        message
    }
}

fn names(files: &[String]) -> String {
    match files {
        [one] => one.clone(),
        many => count(many.len()),
    }
}

fn count(n: usize) -> String {
    if n == 1 {
        "1 file".to_string()
    } else {
        format!("{n} files")
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn display(path: &[u8]) -> String {
    String::from_utf8_lossy(path).into_owned()
}

/// The content with CRLF line endings changed to LF.
fn normalize(content: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(content.len());
    let mut bytes = content.iter().peekable();
    while let Some(&b) = bytes.next() {
        if b == b'\r' && bytes.peek() == Some(&&b'\n') {
            continue;
        }
        result.push(b);
    }
    result
}

/// The first line of the block that Klipper's SAVE_CONFIG writes at the end of printer.cfg.
const SAVE_CONFIG_START: &[u8] = b"#*# <---------------------- SAVE_CONFIG ---------------------->";

/// `true` if the files have a SAVE_CONFIG block and are equal above it.
fn only_save_config_changed(old: &[u8], new: &[u8]) -> bool {
    let (old, new) = (normalize(old), normalize(new));
    match (before_save_config(&old), before_save_config(&new)) {
        (Some(old_body), Some(new_body)) => old_body == new_body,
        // SAVE_CONFIG added the block for the first time.
        (None, Some(new_body)) => trim_end_lines(&old) == trim_end_lines(new_body),
        // The block was removed.
        (Some(old_body), None) => trim_end_lines(old_body) == trim_end_lines(&new),
        (None, None) => false,
    }
}

fn before_save_config(content: &[u8]) -> Option<&[u8]> {
    let mut start = 0;
    for line in content.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(SAVE_CONFIG_START) {
            return Some(&content[..start]);
        }
        start += line.len();
    }
    None
}

fn trim_end_lines(content: &[u8]) -> &[u8] {
    let end = content
        .iter()
        .rposition(|&b| b != b'\n' && b != b' ')
        .map_or(0, |i| i + 1);
    &content[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn changes(added: &[&str], changed: &[&str], removed: &[&str], save: &[&str]) -> Changes {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect();
        Changes {
            added: v(added),
            changed: v(changed),
            removed: v(removed),
            save_config: v(save),
        }
    }

    #[test]
    fn summaries() {
        assert_eq!(
            changes(&[], &["printer.cfg"], &[], &[]).summary(),
            "Changed printer.cfg"
        );
        assert_eq!(
            changes(&["a.cfg", "b.cfg"], &["printer.cfg"], &["old.cfg"], &[]).summary(),
            "Changed printer.cfg, added 2 files, removed old.cfg"
        );
        assert_eq!(
            changes(&[], &["printer.cfg"], &[], &["printer.cfg"]).summary(),
            "SAVE_CONFIG changed printer.cfg"
        );
        assert_eq!(
            changes(&[], &["printer.cfg", "a.cfg"], &[], &["printer.cfg"]).summary(),
            "Changed 2 files"
        );
    }

    #[test]
    fn message_lists_all_files() {
        let message = changes(
            &["new.cfg"],
            &["printer.cfg", "a.cfg"],
            &[],
            &["printer.cfg"],
        )
        .message(false, 10);
        assert_eq!(
            message,
            "Changed 2 files, added new.cfg\n\
             \nChanged:\n  printer.cfg (SAVE_CONFIG)\n  a.cfg\n\
             \nAdded:\n  new.cfg\n"
        );
        assert_eq!(
            Changes::default().message(true, 42),
            "First backup: 42 files"
        );
    }

    #[test]
    fn normalize_changes_crlf_only() {
        assert_eq!(normalize(b"a\r\nb\rc\r\n"), b"a\nb\rc\n");
    }

    const BODY: &str = "[printer]\nkinematics: corexy\n";
    const BLOCK: &str = "#*# <---------------------- SAVE_CONFIG ---------------------->\n\
                         #*# DO NOT EDIT THIS BLOCK OR BELOW. The contents are auto-generated.\n#*#\n";

    #[test]
    fn save_config_detection() {
        let old = format!("{BODY}\n{BLOCK}#*# [probe]\n#*# z_offset = 1.0\n");
        let new = format!("{BODY}\n{BLOCK}#*# [probe]\n#*# z_offset = 1.1\n");
        assert!(only_save_config_changed(old.as_bytes(), new.as_bytes()));

        let first_time = format!("{BODY}\n\n{BLOCK}#*# [probe]\n#*# z_offset = 1.0\n");
        assert!(only_save_config_changed(
            BODY.as_bytes(),
            first_time.as_bytes()
        ));

        let body_changed = format!("{BODY}max_velocity: 300\n\n{BLOCK}#*# [probe]\n");
        assert!(!only_save_config_changed(
            old.as_bytes(),
            body_changed.as_bytes()
        ));
        assert!(!only_save_config_changed(b"a\n", b"b\n"));
    }
}
