//! `README.md` at the top of each printer branch: what the branch is and how to restore the printer. The
//! git host shows it on the page of the branch.
//!
//! printerkeep adds it to each backup, but it is not in the config folder. Its first line is a marker, so that
//! restore and changes from the repository never write it into the config folder. If the config folder has
//! its own `README.md`, that file is backed up, and printerkeep adds none.

use crate::git::{Git, GitError, Tree};

pub const README_FILE: &str = "README.md";

/// The first line. Markdown does not show it.
pub const MARKER: &str =
    "<!-- printerkeep: this file is not in the config folder. printerkeep writes it. -->";

const PROJECT: &str = "https://github.com/ressu/printerkeep";

/// The README of a printer branch.
pub fn content(printer_name: &str, url: &str, follow: &[String]) -> Vec<u8> {
    let mut text = format!(
        "{MARKER}\n\
         # {printer_name}\n\
         \n\
         This branch has the backups of the Klipper configuration of **{printer_name}**, made by\n\
         [printerkeep]({PROJECT}). The files of this branch are the files of the config folder\n\
         (`printer_data/config`) of the printer. Only this README.md is not in the config folder:\n\
         printerkeep writes it, and replaces changes to it.\n\
         \n\
         Each backup is a commit. To see an older version of a file, open the history of the file.\n\
         \n\
         ## Restore the printer\n\
         \n\
         For example after a new SD card:\n\
         \n\
         1. Install Klipper and Moonraker (for example with KIAUH).\n\
         2. Install printerkeep: {PROJECT}#install\n\
         3. On the printer, run:\n\
         \n\
         \x20  ```sh\n\
         \x20  printerkeep restore {url} {printer_name}\n\
         \x20  ```\n\
         \n\
         4. Restart Moonraker and Klipper.\n"
    );
    if !follow.is_empty() {
        text.push_str(
            "\n## Shared files\n\
             \n\
             This printer follows these files from the `shared` branch. Changes from shared come to the\n\
             printer, and the printer keeps its local changes:\n\n",
        );
        for file in follow {
            text.push_str(&format!("- `{file}`\n"));
        }
    }
    text.into_bytes()
}

/// `true` if the content is a README that printerkeep wrote.
pub fn is_generated(content: &[u8]) -> bool {
    content.starts_with(MARKER.as_bytes())
}

/// `true` if `tree` has a README that printerkeep wrote.
pub fn in_tree(git: &Git, tree: &Tree) -> Result<bool, GitError> {
    match tree.get(README_FILE.as_bytes()) {
        Some(entry) => Ok(is_generated(&git.blob(&entry.oid)?)),
        None => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_has_the_marker_and_the_restore_command() {
        let text = content(
            "printer-1",
            "https://github.com/me/printers.git",
            &["macros.cfg".into()],
        );
        assert!(is_generated(&text));
        let text = String::from_utf8(text).unwrap();
        assert!(
            text.contains("   printerkeep restore https://github.com/me/printers.git printer-1\n")
        );
        assert!(text.contains("- `macros.cfg`\n"));
        assert!(!is_generated(b"# My notes\n"));
    }
}
