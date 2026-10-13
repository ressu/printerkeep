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
         This branch holds the backups of the Klipper configuration of **{printer_name}**, made by\n\
         [printerkeep]({PROJECT}).\n\
         \n\
         The files here are the printer's config folder (`printer_data/config`) as it was at the last backup.\n\
         This README is the one exception: printerkeep adds it here, and it is not on the printer. printerkeep\n\
         replaces any changes to it at the next backup.\n\
         \n\
         Each backup is a commit, so the history of a file shows all its earlier versions.\n\
         \n\
         ## Restore this printer\n\
         \n\
         For example after a new SD card:\n\
         \n\
         1. Install Klipper and Moonraker, for example with KIAUH.\n\
         2. [Install printerkeep]({PROJECT}#install).\n\
         3. Run this on the printer:\n\
         \n\
         \x20  ```sh\n\
         \x20  printerkeep restore {url} --printer-name {printer_name}\n\
         \x20  ```\n\
         \n\
         4. Restart Moonraker and Klipper.\n"
    );
    if !follow.is_empty() {
        text.push_str(
            "\n## Shared files\n\
             \n\
             This printer follows these files from the `shared` branch. It gets the changes that are made to\n\
             them there, and keeps its own changes. To see the status of each file, run `printerkeep status`\n\
             on the printer.\n\n",
        );
        for file in follow {
            text.push_str(&format!("- `{file}`\n"));
        }
    }
    text.into_bytes()
}

/// The README of the `shared` branch. It changes only when the URL changes.
pub fn shared_content(url: &str) -> Vec<u8> {
    format!(
        "{MARKER}\n\
         # Printer configuration\n\
         \n\
         This repository holds the Klipper configuration of several printers, made by\n\
         [printerkeep]({PROJECT}).\n\
         \n\
         - **Each printer has its own branch,** named after the printer. Select it in the branch list of the\n\
         \x20 repository. It holds the printer's config folder, and a README with the steps to restore it.\n\
         - **This branch, `shared`, holds the shared files.** A printer that follows a shared file gets the\n\
         \x20 changes that are made to it here, and keeps its own changes. To see the status of each file, run\n\
         \x20 `printerkeep status` on the printer.\n\
         \n\
         ## Restore a printer\n\
         \n\
         Install Klipper, Moonraker and [printerkeep]({PROJECT}#install). Then run this on the printer:\n\
         \n\
         ```sh\n\
         printerkeep restore {url} --printer-name <printer name>\n\
         ```\n\
         \n\
         You can edit this README. Delete its first line (a hidden comment) first: printerkeep then leaves it\n\
         as it is.\n"
    )
    .into_bytes()
}

/// `true` if printerkeep writes the README of `shared`: shared has none, or it is printerkeep's and it is
/// different. A README that the user made (without the marker) is never changed.
pub fn shared_needs_write(existing: Option<&[u8]>, new: &[u8]) -> bool {
    match existing {
        None => true,
        Some(existing) => is_generated(existing) && existing != new,
    }
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
        assert!(text.contains(
            "   printerkeep restore https://github.com/me/printers.git --printer-name printer-1\n"
        ));
        assert!(text.contains("- `macros.cfg`\n"));
        assert!(!is_generated(b"# My notes\n"));
    }

    #[test]
    fn shared_readme_is_written_only_when_needed() {
        let url = "https://github.com/me/printers.git";
        let new = shared_content(url);
        assert!(String::from_utf8_lossy(&new).contains(&format!(
            "printerkeep restore {url} --printer-name <printer name>"
        )));
        assert!(shared_needs_write(None, &new));
        assert!(!shared_needs_write(Some(&new), &new));
        assert!(shared_needs_write(
            Some(&shared_content("/srv/old.git")),
            &new
        ));
        assert!(!shared_needs_write(Some(b"# Our printers\n"), &new));
    }
}
