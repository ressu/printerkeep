//! The overview (C7): `README.md` on the `shared` branch. It shows each printer and the status of its
//! followed files.
//!
//! Each printer writes only its own section, between two marker lines, and only when the section changes.
//! printerkeep writes the text above the sections once, when it makes the file; the user can change it.

use crate::shared::FileStatus;

const FILE_MARKER: &str =
    "<!-- printerkeep: overview. printerkeep writes the printer sections. -->";
const PROJECT: &str = "https://github.com/ressu/printerkeep";

fn start(printer_name: &str) -> String {
    format!("<!-- printerkeep: printer {printer_name} -->")
}

fn end(printer_name: &str) -> String {
    format!("<!-- printerkeep: end {printer_name} -->")
}

/// The section of one printer.
pub fn section(printer_name: &str, files: &[(String, FileStatus)]) -> String {
    let count = |status: &FileStatus| files.iter().filter(|(_, s)| s == status).count();
    let mut summary = format!(
        "{printer_name} follows {} shared {}.",
        files.len(),
        if files.len() == 1 { "file" } else { "files" }
    );
    for status in [FileStatus::NeedsDecision, FileStatus::ChangesWaiting] {
        let n = count(&status);
        if n > 0 {
            summary.push_str(&format!(" {n} {}.", status.word()));
        }
    }
    let mut text = format!(
        "{}\n### {printer_name}\n\n{summary} Branch: `{printer_name}`.\n\n| File | Status |\n|---|---|\n",
        start(printer_name)
    );
    for (path, status) in files {
        text.push_str(&format!("| `{path}` | {} |\n", status.word()));
    }
    // An empty line ends the table in all Markdown renderers.
    text.push('\n');
    text.push_str(&end(printer_name));
    text.push('\n');
    text
}

/// The text of a new overview, without printer sections.
fn new_file(url: &str) -> String {
    format!(
        "{FILE_MARKER}\n\
         # Printers\n\
         \n\
         This repository has the configuration of more printers, made by [printerkeep]({PROJECT}).\n\
         Each printer has its own branch, with the files of its config folder. This branch, `shared`, has\n\
         the shared files. A printer that follows a shared file gets the changes to it, and keeps its local\n\
         changes.\n\
         \n\
         To restore a printer, install Klipper, Moonraker and printerkeep ({PROJECT}#install), then run\n\
         on the printer:\n\
         \n\
         ```sh\n\
         printerkeep restore {url} <printer name>\n\
         ```\n\
         \n\
         ## Printers\n\
         \n"
    )
}

/// The overview with the section of `printer_name` replaced (`None` removes it). Returns `None` if nothing
/// changes, or if the file is not printerkeep's overview (a README that the user made on `shared`).
pub fn update(
    readme: Option<&str>,
    url: &str,
    printer_name: &str,
    section: Option<&str>,
) -> Option<String> {
    let text = match readme {
        Some(text) if !text.starts_with(FILE_MARKER) => return None,
        Some(text) => text.to_string(),
        None if section.is_none() => return None,
        None => new_file(url),
    };
    let (start, end) = (start(printer_name), end(printer_name));
    let old = text.find(&start).and_then(|from| {
        let to = text[from..].find(&end)? + from + end.len();
        // The line end after the end marker belongs to the section.
        Some((
            from,
            if text[to..].starts_with('\n') {
                to + 1
            } else {
                to
            },
        ))
    });
    let new = match (old, section) {
        (Some((from, to)), Some(section)) => format!("{}{section}{}", &text[..from], &text[to..]),
        (Some((from, to)), None) => {
            // Also remove the empty line that separates it from the next section.
            let to = if text[to..].starts_with('\n') {
                to + 1
            } else {
                to
            };
            format!("{}{}", &text[..from], &text[to..])
        }
        (None, Some(section)) => {
            let mut text = text.clone();
            if !text.ends_with("\n\n") {
                text.push('\n');
            }
            text.push_str(section);
            text
        }
        (None, None) => return None,
    };
    (new != text).then_some(new)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://github.com/me/printers.git";

    fn files(statuses: &[(&str, FileStatus)]) -> Vec<(String, FileStatus)> {
        statuses
            .iter()
            .map(|(p, s)| (p.to_string(), s.clone()))
            .collect()
    }

    #[test]
    fn sections_are_added_replaced_and_removed() {
        let one = section("printer-1", &files(&[("macros.cfg", FileStatus::UpToDate)]));
        let text = update(None, URL, "printer-1", Some(&one)).unwrap();
        assert!(text.starts_with(FILE_MARKER));
        assert!(text.contains(&format!("printerkeep restore {URL} <printer name>")));
        assert!(text.ends_with(
            "### printer-1\n\nprinter-1 follows 1 shared file. Branch: `printer-1`.\n\n\
             | File | Status |\n|---|---|\n| `macros.cfg` | up to date |\n\
             \n<!-- printerkeep: end printer-1 -->\n"
        ));
        assert_eq!(update(Some(&text), URL, "printer-1", Some(&one)), None);

        let two = section(
            "printer-2",
            &files(&[
                ("macros.cfg", FileStatus::NeedsDecision),
                ("sensor.cfg", FileStatus::LocalChanges),
            ]),
        );
        assert!(two.contains("printer-2 follows 2 shared files. 1 needs decision."));
        let both = update(Some(&text), URL, "printer-2", Some(&two)).unwrap();
        assert!(both.contains(&format!("{one}\n{two}")), "{both}");

        // The user's own text stays.
        let edited = both.replace("## Printers\n", "## Printers\n\nOur farm.\n");
        let changed = section(
            "printer-1",
            &files(&[("macros.cfg", FileStatus::LocalChanges)]),
        );
        let after = update(Some(&edited), URL, "printer-1", Some(&changed)).unwrap();
        assert!(after.contains("Our farm.\n"));
        assert!(after.contains(&format!("{changed}\n{two}")), "{after}");

        let removed = update(Some(&after), URL, "printer-1", None).unwrap();
        assert!(!removed.contains("printer-1 follows"));
        assert!(removed.ends_with(&two), "{removed}");
    }

    #[test]
    fn a_readme_of_the_user_is_not_changed() {
        let one = section("printer-1", &[]);
        assert_eq!(
            update(Some("# Our printers\n"), URL, "printer-1", Some(&one)),
            None
        );
        assert_eq!(update(None, URL, "printer-1", None), None);
    }
}
