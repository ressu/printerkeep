//! Bundled changes: changes that never make a backup alone (`bundled_changes:` in the settings file).
//!
//! A bundled change waits for the next backup that has another change, and goes into it. The rules:
//! - A rule in brackets matches sections of the SAVE_CONFIG block by name (`[bed_mesh *]` matches
//!   `#*# [bed_mesh default]`).
//! - Any other rule matches files, with the syntax of the ignore rules (`variables.cfg`).

use crate::ignore::{RuleError, translate};
use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use std::collections::BTreeMap;
use std::path::Path;

/// The first line of the block that Klipper's SAVE_CONFIG writes at the end of printer.cfg.
const SAVE_CONFIG_START: &[u8] = b"#*# <---------------------- SAVE_CONFIG ---------------------->";

/// The start of a section line in the SAVE_CONFIG block: `#*# [bed_mesh default]`.
const SECTION_START: &[u8] = b"#*# [";

/// The bundled-change rules, ready to match.
#[derive(Debug)]
pub struct BundleRules {
    files: GlobSet,
    sections: GlobSet,
}

impl BundleRules {
    pub fn new<'a>(rules: impl IntoIterator<Item = &'a str>) -> Result<Self, RuleError> {
        let mut files = GlobSetBuilder::new();
        let mut sections = GlobSetBuilder::new();
        for rule in rules {
            let invalid = |message: String| RuleError {
                option: "bundled_changes",
                rule: rule.to_string(),
                message,
            };
            let rule = rule.trim();
            if let Some(section) = rule.strip_prefix('[') {
                let Some(name) = section.strip_suffix(']') else {
                    return Err(invalid("a section rule must end with ']'".to_string()));
                };
                let glob = GlobBuilder::new(name.trim())
                    .case_insensitive(true)
                    .build()
                    .map_err(|e| invalid(e.kind().to_string()))?;
                sections.add(glob);
            } else {
                let (pattern, _) = translate(rule);
                if pattern.is_empty() {
                    continue;
                }
                let glob = GlobBuilder::new(&pattern)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| invalid(e.kind().to_string()))?;
                files.add(glob);
            }
        }
        let build = |builder: GlobSetBuilder| {
            builder.build().map_err(|e| RuleError {
                option: "bundled_changes",
                rule: String::new(),
                message: e.to_string(),
            })
        };
        Ok(Self {
            files: build(files)?,
            sections: build(sections)?,
        })
    }

    /// `true` if the change from `old` to `new` is bundled: the file matches a file rule, or only sections
    /// of the SAVE_CONFIG block that match a section rule changed. `path` is relative to the config folder.
    pub fn is_bundled(&self, path: &Path, old: &[u8], new: &[u8]) -> bool {
        if self.files.is_match(path) {
            return true;
        }
        if self.sections.is_empty() {
            return false;
        }
        let (old, new) = (crate::merge::to_lf(old), crate::merge::to_lf(new));
        let (old_body, old_block) = split(&old);
        let (new_body, new_block) = split(&new);
        if old_block.is_none() && new_block.is_none() {
            return false;
        }
        if trim_end_lines(old_body) != trim_end_lines(new_body) {
            return false;
        }
        let (old_head, old_sections) = sections(old_block.unwrap_or_default());
        let (new_head, new_sections) = sections(new_block.unwrap_or_default());
        // The lines above the first section are Klipper's header; a new block has a new header.
        if old_block.is_some() && new_block.is_some() && old_head != new_head {
            return false;
        }
        old_sections
            .keys()
            .chain(new_sections.keys())
            .filter(|name| old_sections.get(*name) != new_sections.get(*name))
            .all(|name| {
                self.sections
                    .is_match(String::from_utf8_lossy(name).as_ref())
            })
    }
}

/// The part of the file above the SAVE_CONFIG block, and the block (from its first line).
pub fn split(content: &[u8]) -> (&[u8], Option<&[u8]>) {
    let mut start = 0;
    for line in content.split_inclusive(|&b| b == b'\n') {
        if line.starts_with(SAVE_CONFIG_START) {
            return (&content[..start], Some(&content[start..]));
        }
        start += line.len();
    }
    (content, None)
}

type Lines<'a> = Vec<&'a [u8]>;

/// The lines of the block above its first section, and its sections by name. The `#*#` lines between
/// sections are not part of a section, so a section added after another doesn't change that one.
fn sections(block: &[u8]) -> (Lines<'_>, BTreeMap<&[u8], Lines<'_>>) {
    let mut head = Vec::new();
    let mut sections: BTreeMap<&[u8], Vec<&[u8]>> = BTreeMap::new();
    let mut current: Option<&[u8]> = None;
    for line in block.split(|&b| b == b'\n') {
        if let Some(rest) = line.strip_prefix(SECTION_START)
            && let Some(end) = rest.iter().position(|&b| b == b']')
        {
            let name = &rest[..end];
            sections.entry(name).or_default();
            current = Some(name);
            continue;
        }
        match current {
            Some(name) => sections.entry(name).or_default().push(line),
            None => head.push(line),
        }
    }
    for lines in sections.values_mut() {
        while lines.last().is_some_and(|line| is_blank_block_line(line)) {
            lines.pop();
        }
    }
    (head, sections)
}

/// `#*#`, an empty line in the block, or an empty line.
fn is_blank_block_line(line: &[u8]) -> bool {
    line.strip_prefix(b"#*#")
        .unwrap_or(line)
        .iter()
        .all(u8::is_ascii_whitespace)
}

/// `content` without empty lines and spaces at the end.
pub fn trim_end_lines(content: &[u8]) -> &[u8] {
    let end = content
        .iter()
        .rposition(|&b| b != b'\n' && b != b' ')
        .map_or(0, |i| i + 1);
    &content[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK: &str = "#*# <---------------------- SAVE_CONFIG ---------------------->\n\
                         #*# DO NOT EDIT THIS BLOCK OR BELOW. The contents are auto-generated.\n\
                         #*#\n";

    fn mesh(value: &str) -> String {
        format!("#*# [bed_mesh default]\n#*# points =\n#*# \t{value}, 0.050\n")
    }

    const PROBE: &str = "#*# [probe]\n#*# z_offset = 1.000\n";

    fn file(sections: &[&str]) -> String {
        format!("[printer]\n\n{BLOCK}{}", sections.join("#*#\n"))
    }

    fn bundled(rules: &[&str], old: &str, new: &str) -> bool {
        BundleRules::new(rules.iter().copied()).unwrap().is_bundled(
            Path::new("printer.cfg"),
            old.as_bytes(),
            new.as_bytes(),
        )
    }

    const MESH: &[&str] = &["[bed_mesh *]"];

    #[test]
    fn mesh_change_is_bundled() {
        let old = file(&[PROBE, &mesh("0.100")]);
        let new = file(&[PROBE, &mesh("0.125")]);
        assert!(bundled(MESH, &old, &new));
        assert!(!bundled(&[], &old, &new));
        assert!(!bundled(&["[probe]"], &old, &new));
    }

    #[test]
    fn other_section_change_is_not_bundled() {
        let old = file(&[PROBE, &mesh("0.100")]);
        let new = file(&["#*# [probe]\n#*# z_offset = 1.125\n", &mesh("0.125")]);
        assert!(!bundled(MESH, &old, &new));
    }

    #[test]
    fn mesh_added_or_removed_is_bundled() {
        let probe_only = file(&[PROBE]);
        let with_mesh = file(&[PROBE, &mesh("0.100")]);
        assert!(bundled(MESH, &probe_only, &with_mesh));
        assert!(bundled(MESH, &with_mesh, &probe_only));
        // A second mesh profile.
        let two = format!("{with_mesh}#*#\n#*# [bed_mesh adaptive]\n#*# points = 1\n");
        assert!(bundled(MESH, &with_mesh, &two));
    }

    #[test]
    fn first_block_with_only_a_mesh_is_bundled() {
        let old = "[printer]\n";
        assert!(bundled(MESH, old, &file(&[&mesh("0.100")])));
        assert!(!bundled(MESH, old, &file(&[PROBE, &mesh("0.100")])));
    }

    #[test]
    fn change_above_the_block_is_not_bundled() {
        let old = file(&[&mesh("0.100")]);
        let new = file(&[&mesh("0.125")]).replace("[printer]", "[printer]\nkinematics: corexy");
        assert!(!bundled(MESH, &old, &new));
        assert!(!bundled(MESH, "[printer]\n", "[printer]\nx: 1\n"));
    }

    #[test]
    fn crlf_is_the_same_as_lf() {
        let old = file(&[PROBE, &mesh("0.100")]).replace('\n', "\r\n");
        let new = file(&[PROBE, &mesh("0.125")]);
        assert!(bundled(MESH, &old, &new));
    }

    #[test]
    fn file_rules() {
        let rules = BundleRules::new(["variables.cfg", "/mmu/mmu_vars.cfg"]).unwrap();
        assert!(rules.is_bundled(Path::new("variables.cfg"), b"a", b"b"));
        assert!(rules.is_bundled(Path::new("macros/variables.cfg"), b"a", b"b"));
        assert!(rules.is_bundled(Path::new("mmu/mmu_vars.cfg"), b"a", b"b"));
        assert!(!rules.is_bundled(Path::new("printer.cfg"), b"a", b"b"));
    }

    #[test]
    fn invalid_rule() {
        let error = BundleRules::new(["[bed_mesh *"]).unwrap_err();
        assert_eq!(
            error.to_string(),
            "The bundled_changes rule '[bed_mesh *' isn't valid: a section rule must end with ']'"
        );
    }
}
