//! Override lines (C6): `option: value   # printerkeep: override`. A merge always keeps the printer's override
//! line. Copy to shared never copies it.
//!
//! An override marks one `option: value` line, found by its `[section]` and option name. Lines in a
//! `gcode:` value have no such name, so they cannot be overrides.

use crate::merge;
use std::fmt;

/// The value that copy to shared writes when shared has no line for an override. A new printer that
/// follows the file must set its own value; it never gets the value of a different printer. Klipper refuses
/// this value for pins, numbers and macro variables, so the printer does not start with it.
pub const PLACEHOLDER: &str = "CHANGE_ME";

/// The `[section]` and option name of a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Key {
    pub section: String,
    pub option: String,
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} in [{}]", self.option, self.section)
    }
}

/// `true` if the line has the override mark. Spaces and capitals in the mark do not matter.
pub fn is_override(line: &str) -> bool {
    line.match_indices('#').any(|(index, _)| {
        line[index + 1..]
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
            .starts_with("printerkeep:override")
    })
}

/// The lines of a text, each with its key if it is an `option: value` line.
fn keyed_lines(text: &str) -> Vec<(Option<Key>, &str)> {
    let mut section = String::new();
    text.split_inclusive('\n')
        .map(|line| {
            let content = line.trim_end_matches(['\n', '\r']);
            if content.starts_with(char::is_whitespace) || content.starts_with(['#', ';']) {
                return (None, line);
            }
            if let Some(rest) = content.strip_prefix('[') {
                if let Some(end) = rest.find(']') {
                    section = rest[..end].trim().to_string();
                }
                return (None, line);
            }
            let key = content.find([':', '=']).map(|separator| Key {
                section: section.clone(),
                option: content[..separator].trim().to_string(),
            });
            (key.filter(|k| !k.option.is_empty()), line)
        })
        .collect()
}

/// The override lines of the printer's file, by key. The first line wins if a key is there twice.
fn override_lines(text: &str) -> Vec<(Key, &str)> {
    let mut found: Vec<(Key, &str)> = Vec::new();
    for (key, line) in keyed_lines(text) {
        if let Some(key) = key
            && is_override(line)
            && !found.iter().any(|(k, _)| k == &key)
        {
            found.push((key, line));
        }
    }
    found
}

/// `other` (a base or shared version, LF line ends) with each line that has the key of an override line in
/// `mine` replaced by the printer's line. Then a merge sees no change on those lines.
pub fn protect(mine: &[u8], other: &[u8]) -> Vec<u8> {
    let (Ok(mine), Ok(other)) = (
        String::from_utf8(merge::to_lf(mine)),
        std::str::from_utf8(other),
    ) else {
        return other.to_vec();
    };
    let overrides = override_lines(&mine);
    if overrides.is_empty() {
        return other.as_bytes().to_vec();
    }
    let mut done = Vec::new();
    keyed_lines(other)
        .into_iter()
        .map(|(key, line)| match key {
            Some(key) if !done.contains(&key) => match overrides.iter().find(|(k, _)| k == &key) {
                Some((_, printer_line)) => {
                    done.push(key);
                    with_line_end(printer_line, line)
                }
                None => line.to_string(),
            },
            _ => line.to_string(),
        })
        .collect::<String>()
        .into_bytes()
}

/// The content that copy to shared sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForShared {
    pub content: Vec<u8>,
    /// Override lines where shared keeps its own line.
    pub kept: Vec<Key>,
    /// Override lines that shared did not have. Shared gets the placeholder value.
    pub placeholders: Vec<Key>,
}

/// The printer's file for shared: each override line is replaced by the line in `shared`, or by a line with
/// the placeholder value if `shared` does not have that key.
pub fn for_shared(mine: &[u8], shared: Option<&[u8]>) -> ForShared {
    let crlf = merge::uses_crlf(mine);
    let Ok(text) = String::from_utf8(merge::to_lf(mine)) else {
        return ForShared {
            content: mine.to_vec(),
            kept: Vec::new(),
            placeholders: Vec::new(),
        };
    };
    let shared_text = shared
        .map(|s| String::from_utf8_lossy(&merge::to_lf(s)).into_owned())
        .unwrap_or_default();
    let shared_lines = keyed_lines(&shared_text);
    let mut result = ForShared {
        content: Vec::new(),
        kept: Vec::new(),
        placeholders: Vec::new(),
    };
    let mut content = String::new();
    for (key, line) in keyed_lines(&text) {
        match key {
            Some(key) if is_override(line) => {
                let in_shared = shared_lines
                    .iter()
                    .find(|(k, _)| k.as_ref() == Some(&key))
                    .map(|(_, l)| *l);
                match in_shared {
                    Some(shared_line) => {
                        content.push_str(&with_line_end(shared_line, line));
                        result.kept.push(key);
                    }
                    None => {
                        content.push_str(&placeholder(line));
                        result.placeholders.push(key);
                    }
                }
            }
            _ => content.push_str(line),
        }
    }
    result.content = if crlf {
        merge::to_crlf(content.as_bytes())
    } else {
        content.into_bytes()
    };
    result
}

/// The `[section]` that line `line` (from 0) of a text is in.
pub fn section_at(text: &[u8], line: usize) -> Option<String> {
    let text = String::from_utf8_lossy(text);
    let mut section = None;
    for content in text.lines().take(line + 1) {
        if let Some(rest) = content.strip_prefix('[')
            && let Some(end) = rest.find(']')
        {
            section = Some(rest[..end].trim().to_string());
        }
    }
    section
}

/// The keys of `count` lines from line `start` (from 0), if each of them is an `option: value` line that
/// can be an override. `None` for other lines: comments, empty lines, and lines in a `gcode:` value, which
/// have no option name. The `gcode:` line itself is not an override either: its value goes on in the lines
/// below it.
pub fn keys_at(text: &[u8], start: usize, count: usize) -> Option<Vec<Key>> {
    let text = String::from_utf8(merge::to_lf(text)).ok()?;
    let lines = keyed_lines(&text);
    lines
        .get(start..start + count)?
        .iter()
        .map(|(key, _)| key.clone().filter(|k| k.option != "gcode"))
        .collect()
}

/// The printer's file with the override mark on each line with one of the keys.
pub fn mark(mine: &[u8], keys: &[Key]) -> Vec<u8> {
    let crlf = merge::uses_crlf(mine);
    let Ok(text) = String::from_utf8(merge::to_lf(mine)) else {
        return mine.to_vec();
    };
    let content: String = keyed_lines(&text)
        .into_iter()
        .map(|(key, line)| match key {
            Some(key) if keys.contains(&key) && !is_override(line) => {
                let end = &line[line.trim_end_matches('\n').len()..];
                format!("{}   # printerkeep: override{end}", line.trim_end())
            }
            _ => line.to_string(),
        })
        .collect();
    if crlf {
        merge::to_crlf(content.as_bytes())
    } else {
        content.into_bytes()
    }
}

/// The number of override lines with the placeholder value.
pub fn placeholders(content: &[u8]) -> usize {
    let text = String::from_utf8_lossy(content);
    keyed_lines(&text)
        .iter()
        .filter(|(key, line)| {
            key.is_some() && is_override(line) && value(line).trim() == PLACEHOLDER
        })
        .count()
}

/// The value of an `option: value   # comment` line, without the comment.
fn value(line: &str) -> &str {
    let after = line.find([':', '=']).map_or("", |i| &line[i + 1..]);
    after.find('#').map_or(after, |i| &after[..i])
}

/// The line with the placeholder value. Keeps the option name, the spaces and the comment.
fn placeholder(line: &str) -> String {
    let separator = line.find([':', '=']).unwrap_or(line.len() - 1);
    let rest = &line[separator + 1..];
    let comment = rest.find('#').unwrap_or(rest.len());
    let spaces = rest[..comment].len() - rest[..comment].trim_end().len();
    format!(
        "{} {PLACEHOLDER}{}{}",
        &line[..=separator],
        &rest[comment - spaces..comment],
        &rest[comment..]
    )
}

/// `line` with the line end of `like`.
fn with_line_end(line: &str, like: &str) -> String {
    let content = line.trim_end_matches(['\n', '\r']);
    let end = &like[like.trim_end_matches(['\n', '\r']).len()..];
    format!("{content}{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINE: &str = "[filament_switch_sensor runout]\n\
                        switch_pin: ^PG12   # printerkeep: override\n\
                        pause_on_runout: True\n\
                        [fan]\n\
                        pin: PA8 #printerkeep:override\n";

    #[test]
    fn marks_are_found() {
        assert!(is_override("switch_pin: ^PG12   # printerkeep: override"));
        assert!(is_override("pin: PA8 #PrinterKeep : Override"));
        assert!(!is_override("pin: PA8 # printerkeep"));
        assert!(!is_override("pin: PA8"));
    }

    #[test]
    fn protect_puts_the_printer_lines_into_shared() {
        let shared = "[filament_switch_sensor runout]\nswitch_pin: ^PA1\npause_on_runout: False\n\
                      [fan]\npin: PB1\n[heater_fan x]\npin: PB2\n";
        assert_eq!(
            String::from_utf8(protect(MINE.as_bytes(), shared.as_bytes())).unwrap(),
            "[filament_switch_sensor runout]\nswitch_pin: ^PG12   # printerkeep: override\n\
             pause_on_runout: False\n[fan]\npin: PA8 #printerkeep:override\n[heater_fan x]\npin: PB2\n"
        );
    }

    #[test]
    fn copy_to_shared_keeps_shared_lines_or_writes_the_placeholder() {
        let shared =
            "[filament_switch_sensor runout]\nswitch_pin: CHANGE_ME   # printerkeep: override\n";
        let result = for_shared(MINE.as_bytes(), Some(shared.as_bytes()));
        assert_eq!(
            String::from_utf8(result.content.clone()).unwrap(),
            "[filament_switch_sensor runout]\n\
             switch_pin: CHANGE_ME   # printerkeep: override\n\
             pause_on_runout: True\n\
             [fan]\n\
             pin: CHANGE_ME #printerkeep:override\n"
        );
        assert_eq!(result.kept.len(), 1);
        assert_eq!(
            result.kept[0].to_string(),
            "switch_pin in [filament_switch_sensor runout]"
        );
        assert_eq!(result.placeholders[0].to_string(), "pin in [fan]");
        assert_eq!(placeholders(&result.content), 2);
        assert_eq!(placeholders(MINE.as_bytes()), 0);
    }

    #[test]
    fn keys_and_sections_of_lines() {
        let text = b"[gcode_macro LOAD]\nvariable_temp: 230\ngcode:\n    G1 E50\n# note\n";
        assert_eq!(section_at(text, 3).as_deref(), Some("gcode_macro LOAD"));
        assert_eq!(section_at(text, 0).as_deref(), Some("gcode_macro LOAD"));
        let keys = keys_at(text, 1, 1).unwrap();
        assert_eq!(keys[0].to_string(), "variable_temp in [gcode_macro LOAD]");
        // Lines in gcode, the gcode: line, and comments can't be overrides.
        assert_eq!(keys_at(text, 3, 1), None);
        assert_eq!(keys_at(text, 2, 1), None);
        assert_eq!(keys_at(text, 1, 3), None);
        assert_eq!(keys_at(text, 4, 1), None);
        assert_eq!(keys_at(text, 9, 1), None);
    }

    #[test]
    fn mark_adds_the_override_comment() {
        let text = "[gcode_macro LOAD]\r\nvariable_temp: 230 \r\nvariable_speed: 5\r\n";
        let keys = keys_at(text.as_bytes(), 1, 1).unwrap();
        let marked = String::from_utf8(mark(text.as_bytes(), &keys)).unwrap();
        assert_eq!(
            marked,
            "[gcode_macro LOAD]\r\nvariable_temp: 230   # printerkeep: override\r\nvariable_speed: 5\r\n"
        );
        assert_eq!(mark(marked.as_bytes(), &keys), marked.as_bytes());
    }

    #[test]
    fn crlf_files_stay_crlf() {
        let mine = MINE.replace('\n', "\r\n");
        let result = for_shared(mine.as_bytes(), None);
        let text = String::from_utf8(result.content).unwrap();
        assert!(
            text.contains("switch_pin: CHANGE_ME   # printerkeep: override\r\n"),
            "{text:?}"
        );
        assert!(!text.replace("\r\n", "").contains('\n'), "{text:?}");
    }
}
