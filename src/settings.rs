//! The settings file, `printerkeep.conf`.
//!
//! The format is a strict subset of the Moonraker format: one `[printerkeep]` section, `option: value`
//! lines, values that continue on indented lines, and full-line `#` or `;` comments. See `docs/naming.md`.

use std::path::Path;

/// The settings. A missing settings file gives the defaults.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Settings {
    pub printer_name: Option<String>,
    pub repository: Option<String>,
    pub follow: Vec<String>,
    pub ignore: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("{file}, line {line}: {message}")]
    Syntax {
        file: String,
        line: usize,
        message: String,
    },
    #[error("Cannot read {file}: {source}")]
    Read {
        file: String,
        #[source]
        source: std::io::Error,
    },
    #[error("Cannot write {file}: {source}")]
    Write {
        file: String,
        #[source]
        source: std::io::Error,
    },
}

const SECTION: &str = "printerkeep";
const OPTIONS: &str = "printer_name, repository, follow, ignore";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Single,
    List,
}

fn kind(option: &str) -> Option<Kind> {
    match option {
        "printer_name" | "repository" => Some(Kind::Single),
        "follow" | "ignore" => Some(Kind::List),
        _ => None,
    }
}

/// Reads the settings file. If the file does not exist, returns the defaults.
pub fn load(path: &Path) -> Result<Settings, SettingsError> {
    let file = path.display().to_string();
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, &file),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(source) => Err(SettingsError::Read { file, source }),
    }
}

struct Entry {
    option: String,
    line: usize,
    values: Vec<String>,
}

/// Parses the text of a settings file. `file` is used in error messages.
pub fn parse(text: &str, file: &str) -> Result<Settings, SettingsError> {
    let error = |line: usize, message: String| SettingsError::Syntax {
        file: file.to_string(),
        line,
        message,
    };

    let mut entries: Vec<Entry> = Vec::new();
    let mut in_section = false;
    // The entry whose value can continue on the next indented line.
    let mut current: Option<usize> = None;

    for (index, raw) in text.lines().enumerate() {
        let number = index + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let trimmed = line.trim();

        if trimmed.is_empty() {
            current = None;
            continue;
        }
        if trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }

        if line.starts_with(char::is_whitespace) {
            let Some(entry) = current else {
                return Err(error(
                    number,
                    "this line starts with a space, but it does not continue an option. \
                     Remove the space at the start of the line."
                        .to_string(),
                ));
            };
            let value = strip_comment(trimmed);
            if !value.is_empty() {
                entries[entry].values.push(value.to_string());
            }
            continue;
        }

        if let Some(rest) = line.strip_prefix('[') {
            let header = strip_comment(rest);
            let Some(name) = header.strip_suffix(']') else {
                return Err(error(
                    number,
                    "a section line must end with ']'.".to_string(),
                ));
            };
            let name = name.trim();
            if name != SECTION {
                return Err(error(
                    number,
                    format!("unknown section [{name}]. The file has only the [{SECTION}] section."),
                ));
            }
            if in_section {
                return Err(error(
                    number,
                    format!("the [{SECTION}] section is in the file two times."),
                ));
            }
            in_section = true;
            current = None;
            continue;
        }

        if !in_section {
            return Err(error(
                number,
                format!("this option is not in a section. Add [{SECTION}] above it."),
            ));
        }

        let Some(separator) = line.find([':', '=']) else {
            return Err(error(
                number,
                "this line is not 'option: value'.".to_string(),
            ));
        };
        let option = line[..separator].trim();
        let value = strip_comment(line[separator + 1..].trim());

        if kind(option).is_none() {
            return Err(error(
                number,
                format!("unknown option '{option}'. Valid options: {OPTIONS}."),
            ));
        }
        if let Some(earlier) = entries.iter().find(|e| e.option == option) {
            return Err(error(
                number,
                format!(
                    "the option '{option}' is in the file two times (also on line {}).",
                    earlier.line
                ),
            ));
        }

        let mut values = Vec::new();
        if !value.is_empty() {
            values.push(value.to_string());
        }
        entries.push(Entry {
            option: option.to_string(),
            line: number,
            values,
        });
        current = Some(entries.len() - 1);
    }

    let mut settings = Settings::default();
    for entry in entries {
        let single = || match entry.values.as_slice() {
            [value] => Ok(value.clone()),
            [] => Err(error(
                entry.line,
                format!("the option '{}' has no value.", entry.option),
            )),
            _ => Err(error(
                entry.line,
                format!("the option '{}' can have only one value.", entry.option),
            )),
        };
        match entry.option.as_str() {
            "printer_name" => settings.printer_name = Some(single()?),
            "repository" => settings.repository = Some(single()?),
            "follow" => settings.follow = entry.values,
            "ignore" => settings.ignore = entry.values,
            _ => unreachable!("options are checked above"),
        }
    }
    Ok(settings)
}

/// Sets single-value options in the settings file and keeps all other lines and comments.
/// Makes the file if it does not exist. The file is replaced in one step (no half-written file).
pub fn set_options(path: &Path, options: &[(&str, &str)]) -> Result<(), SettingsError> {
    let file = path.display().to_string();
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => return Err(SettingsError::Read { file, source }),
    };
    // Never change a file that printerkeep cannot read.
    parse(&text, &file)?;

    let mut lines: Vec<String> = text
        .lines()
        .map(|l| l.strip_suffix('\r').unwrap_or(l).to_string())
        .collect();
    for (option, value) in options {
        debug_assert!(
            kind(option) == Some(Kind::Single),
            "{option} is not a single-value option"
        );
        let new_line = format!("{option}: {value}");
        let existing = lines.iter().position(|line| {
            !line.starts_with(char::is_whitespace)
                && line
                    .find([':', '='])
                    .is_some_and(|i| line[..i].trim() == *option)
        });
        let header = lines.iter().position(|line| {
            line.strip_prefix('[')
                .map(strip_comment)
                .and_then(|h| h.strip_suffix(']'))
                .is_some_and(|name| name.trim() == SECTION)
        });
        match (existing, header) {
            (Some(index), _) => lines[index] = new_line,
            // New options go at the end of the section (the file has only this section).
            (None, Some(index)) => {
                let last = lines
                    .iter()
                    .rposition(|l| !l.trim().is_empty())
                    .map_or(index, |last| last.max(index));
                lines.insert(last + 1, new_line);
            }
            (None, None) => {
                if lines.last().is_some_and(|l| !l.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("[{SECTION}]"));
                lines.push(new_line);
            }
        }
    }

    let mut text = lines.join("\n");
    text.push('\n');
    let write_error = |source| SettingsError::Write {
        file: file.clone(),
        source,
    };
    let temporary = path.with_file_name(".printerkeep.conf.new");
    std::fs::write(&temporary, &text).map_err(write_error)?;
    std::fs::rename(&temporary, path).map_err(write_error)
}

/// Removes an inline comment: `#` or `;` after a space.
fn strip_comment(text: &str) -> &str {
    let mut previous_space = false;
    for (index, c) in text.char_indices() {
        if (c == '#' || c == ';') && previous_space {
            return text[..index].trim_end();
        }
        previous_space = c.is_whitespace();
    }
    text.trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(text: &str) -> Settings {
        parse(text, "printerkeep.conf").unwrap()
    }

    fn err(text: &str) -> String {
        parse(text, "printerkeep.conf").unwrap_err().to_string()
    }

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(ok(""), Settings::default());
        assert_eq!(ok("# only a comment\n\n"), Settings::default());
    }

    #[test]
    fn full_example_from_docs() {
        let settings = ok("# printerkeep.conf\n\
             [printerkeep]\n\
             printer_name: printer-1\n\
             repository: git@github.com:me/printers.git\n\
             follow:\n  macros/macros.cfg\n  KAMP_Settings.cfg\n\
             ignore:\n  timelapse/*\n");
        assert_eq!(
            settings,
            Settings {
                printer_name: Some("printer-1".into()),
                repository: Some("git@github.com:me/printers.git".into()),
                follow: vec!["macros/macros.cfg".into(), "KAMP_Settings.cfg".into()],
                ignore: vec!["timelapse/*".into()],
            }
        );
    }

    #[test]
    fn accepts_equals_crlf_comments_and_first_line_values() {
        let settings = ok("[printerkeep]\r\n\
             printer_name = printer-2   # inline comment\r\n\
             ignore: *.bak\r\n\
             \x20 ; a comment line inside the list\r\n\
             \x20 old/\r\n");
        assert_eq!(settings.printer_name.as_deref(), Some("printer-2"));
        assert_eq!(settings.ignore, vec!["*.bak", "old/"]);
    }

    #[test]
    fn https_url_is_not_cut_at_colon() {
        let settings = ok("[printerkeep]\nrepository: https://github.com/me/printers.git\n");
        assert_eq!(
            settings.repository.as_deref(),
            Some("https://github.com/me/printers.git")
        );
    }

    #[test]
    fn errors_have_line_numbers_and_clear_text() {
        assert_eq!(
            err("[printerkeep]\nfolow:\n  a.cfg\n"),
            "printerkeep.conf, line 2: unknown option 'folow'. \
             Valid options: printer_name, repository, follow, ignore."
        );
        assert!(err("[moonraker]\n").contains("line 1: unknown section [moonraker]"));
        assert!(err("ignore: a\n").contains("line 1: this option is not in a section"));
        assert!(err("[printerkeep]\n  a.cfg\n").contains("line 2: this line starts with a space"));
        assert!(err("[printerkeep]\nignore: a\nignore: b\n").contains("(also on line 2)"));
        assert!(err("[printerkeep]\nprinter_name:\n").contains("has no value"));
        assert!(err("[printerkeep]\nprinter_name: a\n  b\n").contains("only one value"));
        assert!(err("[printerkeep]\njust words\n").contains("is not 'option: value'"));
        assert!(err("[printerkeep]\n[printerkeep]\n").contains("two times"));
    }

    #[test]
    fn blank_line_ends_a_list() {
        assert!(err("[printerkeep]\nignore:\n  a\n\n  b\n").contains("line 5"));
    }

    #[test]
    fn set_options_keeps_comments_and_other_options() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("printerkeep.conf");
        std::fs::write(
            &path,
            "# My printer\n[printerkeep]\nrepository: old-url # old\nignore:\n  a/\n",
        )
        .unwrap();
        set_options(
            &path,
            &[("repository", "new-url"), ("printer_name", "printer-1")],
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# My printer\n[printerkeep]\nrepository: new-url\nignore:\n  a/\nprinter_name: printer-1\n"
        );
        assert!(!dir.path().join(".printerkeep.conf.new").exists());
    }

    #[test]
    fn set_options_makes_a_new_file_and_a_section() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("printerkeep.conf");
        set_options(&path, &[("repository", "/srv/printers.git")]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[printerkeep]\nrepository: /srv/printers.git\n"
        );

        std::fs::write(&path, "# only a comment\n").unwrap();
        set_options(&path, &[("printer_name", "printer-2")]).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "# only a comment\n\n[printerkeep]\nprinter_name: printer-2\n"
        );
    }

    #[test]
    fn set_options_refuses_an_invalid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("printerkeep.conf");
        std::fs::write(&path, "[printerkeep]\nfolow: a\n").unwrap();
        assert!(set_options(&path, &[("repository", "x")]).is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "[printerkeep]\nfolow: a\n"
        );
    }

    #[test]
    fn missing_file_gives_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(&dir.path().join("printerkeep.conf")).unwrap(),
            Settings::default()
        );
    }
}
