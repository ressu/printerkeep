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
         \x20  printerkeep restore --repository {url} --printer-name {printer_name}\n\
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

/// The first line of the README of `shared`. It is not `MARKER`: printerkeep 1.0 replaces a README of shared
/// only if it starts with `MARKER`, so it leaves this one as it is.
const SHARED_NOTE: &str = "<!-- printerkeep updates the parts between the printerkeep comments. Edit the rest as you like. -->";

const PRINTERS: &str = "printers";
const RESTORE: &str = "restore";
const TABLE_HEADER: &str = "| Printer | Follows from shared |\n| --- | --- |\n";

fn begin(block: &str) -> String {
    format!("<!-- printerkeep:begin {block} -->\n")
}

fn end(block: &str) -> String {
    format!("<!-- printerkeep:end {block} -->")
}

/// The README of `shared` with this printer's row, or `None` if it stays as it is.
///
/// printerkeep writes only the text between its begin and end comments; the rest is the owner's. A README
/// without these comments is never changed. The README changes only when this printer's row changes (or
/// when shared has none, or has the README of printerkeep 1.0), so that printers with other URLs for the
/// same repository don't change the restore command back and forth.
pub fn shared_readme(
    existing: Option<&[u8]>,
    url: &str,
    printer_name: &str,
    follow: &[String],
) -> Option<Vec<u8>> {
    let (text, new) = match existing {
        Some(content) if !is_generated(content) => {
            (String::from_utf8(content.to_vec()).ok()?, false)
        }
        // None, or the README of printerkeep 1.0.
        _ => (shared_template(), true),
    };
    let (start, stop) = block_range(&text, PRINTERS)?;
    let old = &text[start..stop];
    let mut rows: Vec<(String, String)> = old
        .lines()
        .filter(|line| line.starts_with('|') && !TABLE_HEADER.contains(line))
        .filter_map(|line| Some((row_name(line)?, line.to_string())))
        .collect();
    if follow.is_empty() && !rows.iter().any(|(name, _)| name == printer_name) {
        return None;
    }
    let link = link_format(&rows, url);
    rows.retain(|(name, _)| name != printer_name);
    if !follow.is_empty() {
        let printer = match &link {
            Some(prefix) => format!("[{printer_name}]({prefix}{printer_name})"),
            None => printer_name.to_string(),
        };
        let files: Vec<String> = follow.iter().map(|f| format!("`{f}`")).collect();
        rows.push((
            printer_name.to_string(),
            format!("| {printer} | {} |", files.join(", ")),
        ));
    }
    rows.sort();
    let diagram = known_host(url).is_some() || old.contains("```mermaid");
    let printers = printers_block(&rows, diagram);
    if !new && old == printers {
        return None;
    }
    let mut text = replace_block(&text, PRINTERS, &printers);
    text = replace_block(&text, RESTORE, &restore_block(url));
    Some(text.into_bytes())
}

/// The new README of `shared`, with empty blocks.
fn shared_template() -> String {
    format!(
        "{SHARED_NOTE}\n\
         # Printer configuration\n\
         \n\
         This repository holds the Klipper configuration of several printers, backed up by\n\
         [printerkeep]({PROJECT}). Each printer has its own branch, named after the printer. This branch,\n\
         `shared`, holds the files that the printers share.\n\
         \n\
         ## Printers\n\
         \n\
         {}{}\n\
         \n\
         ## How it works\n\
         \n\
         - **Each printer's branch** holds its config folder, and a README with the steps to restore it. Its\n\
         \x20 link is in the list above, or select it in the branch list of the repository.\n\
         - **A printer that follows a shared file** gets the changes that are made to it here, and keeps its\n\
         \x20 own changes.\n\
         - **To change a shared file for all printers**, edit it on one printer. Then copy it to shared from\n\
         \x20 that printer.\n\
         \n\
         ## On a printer\n\
         \n\
         | To | Run |\n\
         | --- | --- |\n\
         | See the status of each followed file | `printerkeep status` |\n\
         | Follow a shared file | `printerkeep follow \"<file>\"` |\n\
         | Copy a file to shared | `printerkeep copy-to-shared \"<file>\"` |\n\
         | Take one file from another printer | `printerkeep restore \"<file>\" --printer-name \"<printer name>\"` |\n\
         \n\
         ## Restore a printer\n\
         \n\
         {}{}\n\
         \n\
         ---\n\
         \n\
         printerkeep updates the parts between the `printerkeep:begin` and `printerkeep:end` comments. You can\n\
         edit the rest of this README, and printerkeep keeps your changes.\n",
        begin(PRINTERS),
        end(PRINTERS),
        begin(RESTORE),
        end(RESTORE),
    )
}

fn printers_block(rows: &[(String, String)], diagram: bool) -> String {
    if rows.is_empty() {
        return "No printer follows shared files yet.\n".to_string();
    }
    let mut text = TABLE_HEADER.to_string();
    for (_, row) in rows {
        text.push_str(row);
        text.push('\n');
    }
    if diagram {
        text.push_str("\n```mermaid\nflowchart LR\n  shared[(shared)]\n");
        for (i, (name, _)) in rows.iter().enumerate() {
            text.push_str(&format!(
                "  p{i}[\"{}\"] -- follows --> shared\n",
                name.replace('"', "#quot;")
            ));
        }
        text.push_str("```\n");
    }
    text.push_str(
        "\nEach printer updates its own row when it checks the repository, so the list can be behind for a\n\
         while. A printer with printerkeep 1.0 is in the list after you update printerkeep on it.\n",
    );
    text
}

fn restore_block(url: &str) -> String {
    format!(
        "Install Klipper, Moonraker, and [printerkeep]({PROJECT}#install). Then run this on the printer:\n\
         \n\
         ```sh\n\
         printerkeep restore --repository {url} --printer-name \"<printer name>\"\n\
         ```\n\
         \n\
         `<printer name>` is the name of the printer's branch.\n"
    )
}

/// The start and the end of the text inside the block.
fn block_range(text: &str, block: &str) -> Option<(usize, usize)> {
    let start = text.find(&begin(block))? + begin(block).len();
    let stop = start + text[start..].find(&end(block))?;
    Some((start, stop))
}

/// The text with new content in the block. A block that the owner deleted stays deleted.
fn replace_block(text: &str, block: &str, content: &str) -> String {
    match block_range(text, block) {
        Some((start, stop)) => format!("{}{content}{}", &text[..start], &text[stop..]),
        None => text.to_string(),
    }
}

/// The printer name in the first cell of a row: `[name](link)` or `name`.
fn row_name(row: &str) -> Option<String> {
    let cell = row.trim_start_matches('|').split('|').next()?.trim();
    let name = match cell.strip_prefix('[') {
        Some(rest) => rest.split(']').next()?,
        None => cell,
    };
    let name = name.trim_matches('`');
    (!name.is_empty()).then(|| name.to_string())
}

/// The start of a link to a printer's branch: from a link in another row that ends with that row's printer
/// name (so the owner can fix the format by hand), or the format of a known git host.
fn link_format(rows: &[(String, String)], url: &str) -> Option<String> {
    let from_row = rows.iter().find_map(|(name, row)| {
        let link = row.split("](").nth(1)?.split(')').next()?;
        link.strip_suffix(name.as_str()).map(str::to_string)
    });
    from_row.or_else(|| {
        let (host, web) = known_host(url)?;
        let path = if host == "github.com" {
            "tree"
        } else if host.contains("gitlab") {
            "-/tree"
        } else {
            "src/branch"
        };
        Some(format!("{web}/{path}/"))
    })
}

/// The host and the web address of the repository, if it is on a known git host. These hosts show Mermaid
/// diagrams.
fn known_host(url: &str) -> Option<(String, String)> {
    let (authority, path) = match url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("ssh://"))
    {
        Some(rest) => rest.split_once('/')?,
        // The scp form: [user@]host:path
        None if !url.contains("://") && !url.starts_with('/') => url.split_once(':')?,
        None => return None,
    };
    let host = authority
        .rsplit('@')
        .next()?
        .split(':')
        .next()?
        .to_lowercase();
    let known = host == "github.com"
        || host == "codeberg.org"
        || ["gitlab", "gitea", "forgejo"]
            .iter()
            .any(|h| host.contains(h));
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    (known && !path.is_empty()).then(|| (host.clone(), format!("https://{host}/{path}")))
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
            "   printerkeep restore --repository https://github.com/me/printers.git --printer-name printer-1\n"
        ));
        assert!(text.contains("- `macros.cfg`\n"));
        assert!(!is_generated(b"# My notes\n"));
    }

    const URL: &str = "https://github.com/me/printers.git";

    fn follow(files: &[&str]) -> Vec<String> {
        files.iter().map(|f| f.to_string()).collect()
    }

    fn text(content: Vec<u8>) -> String {
        String::from_utf8(content).unwrap()
    }

    #[test]
    fn a_new_shared_readme_lists_the_printer_and_the_restore_command() {
        let new =
            text(shared_readme(None, URL, "printer-1", &follow(&["macros.cfg", "a.cfg"])).unwrap());
        assert!(
            new.contains(
                "| [printer-1](https://github.com/me/printers/tree/printer-1) | `macros.cfg`, `a.cfg` |\n"
            ),
            "{new}"
        );
        assert!(new.contains(&format!(
            "printerkeep restore --repository {URL} --printer-name \"<printer name>\""
        )));
        assert!(new.contains("```mermaid\n"), "{new}");
        // Nothing to change: no new README.
        assert_eq!(
            shared_readme(
                Some(new.as_bytes()),
                URL,
                "printer-1",
                &follow(&["macros.cfg", "a.cfg"])
            ),
            None
        );
        // A printer that follows nothing and isn't listed writes nothing.
        assert_eq!(shared_readme(None, URL, "printer-2", &[]), None);
    }

    #[test]
    fn each_printer_changes_only_its_own_row() {
        let first = shared_readme(None, URL, "printer-2", &follow(&["macros.cfg"])).unwrap();
        let both =
            text(shared_readme(Some(&first), URL, "printer-1", &follow(&["a.cfg"])).unwrap());
        let rows: Vec<&str> = both.lines().filter(|l| l.starts_with("| [")).collect();
        assert_eq!(
            rows,
            [
                "| [printer-1](https://github.com/me/printers/tree/printer-1) | `a.cfg` |",
                "| [printer-2](https://github.com/me/printers/tree/printer-2) | `macros.cfg` |",
            ]
        );
        // A printer that follows nothing is removed.
        let one = text(shared_readme(Some(both.as_bytes()), URL, "printer-2", &[]).unwrap());
        assert!(!one.contains("[printer-2]"), "{one}");
        assert!(one.contains("[printer-1]"), "{one}");
    }

    #[test]
    fn the_owner_s_text_outside_the_blocks_stays() {
        let new = text(shared_readme(None, URL, "printer-1", &follow(&["a.cfg"])).unwrap());
        let edited = new.replace(
            "# Printer configuration\n",
            "# Our farm\n\nPrinters in the garage.\n",
        );
        let updated = text(
            shared_readme(
                Some(edited.as_bytes()),
                "/srv/new.git",
                "printer-1",
                &follow(&["b.cfg"]),
            )
            .unwrap(),
        );
        assert!(
            updated.contains("# Our farm\n\nPrinters in the garage.\n"),
            "{updated}"
        );
        assert!(updated.contains("`b.cfg`"), "{updated}");
        assert!(updated.contains("--repository /srv/new.git "), "{updated}");
        // A README without printerkeep's blocks is the owner's: printerkeep never changes it.
        assert_eq!(
            shared_readme(
                Some(b"# Our printers\n"),
                URL,
                "printer-1",
                &follow(&["a.cfg"])
            ),
            None
        );
    }

    #[test]
    fn the_restore_block_changes_only_with_a_row() {
        // Printers that use other URLs for the same repository don't change the README back and forth.
        let new = shared_readme(None, URL, "printer-1", &follow(&["a.cfg"])).unwrap();
        assert_eq!(
            shared_readme(
                Some(&new),
                "git@github.com:me/printers.git",
                "printer-1",
                &follow(&["a.cfg"])
            ),
            None
        );
    }

    #[test]
    fn a_readme_from_1_0_is_replaced_once_and_1_0_leaves_the_new_one() {
        let old = shared_content_1_0(URL);
        let new = shared_readme(Some(&old), URL, "printer-1", &follow(&["a.cfg"])).unwrap();
        assert!(String::from_utf8_lossy(&new).contains("| [printer-1]"));
        // printerkeep 1.0 writes the README of shared only if it starts with MARKER (and differs).
        assert!(!is_generated(&new));
    }

    /// The README of shared that printerkeep 1.0 wrote (its start is enough).
    fn shared_content_1_0(url: &str) -> Vec<u8> {
        format!(
            "{MARKER}\n# Printer configuration\n\nprinterkeep restore --repository {url} --printer-name <printer name>\n"
        )
        .into_bytes()
    }

    #[test]
    fn branch_links_follow_the_host_or_an_earlier_link() {
        let row = |url: &str| {
            let new = text(shared_readme(None, url, "printer-1", &follow(&["a.cfg"])).unwrap());
            new.lines()
                .find(|l| l.contains("`a.cfg`"))
                .unwrap()
                .to_string()
        };
        assert_eq!(
            row("git@github.com:me/printers.git"),
            "| [printer-1](https://github.com/me/printers/tree/printer-1) | `a.cfg` |"
        );
        assert_eq!(
            row("https://me@gitlab.example.com/me/printers.git"),
            "| [printer-1](https://gitlab.example.com/me/printers/-/tree/printer-1) | `a.cfg` |"
        );
        assert_eq!(
            row("ssh://git@codeberg.org/me/printers.git"),
            "| [printer-1](https://codeberg.org/me/printers/src/branch/printer-1) | `a.cfg` |"
        );
        assert_eq!(row("/srv/git/printers.git"), "| printer-1 | `a.cfg` |");
        let unknown = text(
            shared_readme(
                None,
                "git@git.example.com:printers.git",
                "printer-1",
                &follow(&["a.cfg"]),
            )
            .unwrap(),
        );
        assert!(!unknown.contains("mermaid"), "{unknown}");
        // A link that the owner fixed by hand shows the format.
        let fixed = unknown.replace(
            "| printer-1 |",
            "| [printer-1](https://git.example.com/p/printer-1) |",
        );
        let both = text(
            shared_readme(
                Some(fixed.as_bytes()),
                "git@git.example.com:printers.git",
                "printer-2",
                &follow(&["a.cfg"]),
            )
            .unwrap(),
        );
        assert!(
            both.contains("| [printer-2](https://git.example.com/p/printer-2) | `a.cfg` |"),
            "{both}"
        );
    }
}
