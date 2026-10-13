//! The three-way merge of one file (C4), and the needs-decision block (C5).
//!
//! `git merge-file` does the merge. printerkeep reads its result as merged lines and places where the
//! changes touch the same lines. At each such place, it writes a needs-decision block: this printer's lines
//! stay active, and the shared lines are below them, commented out. Thus, the file is always valid
//! configuration. `tests/klipper/check_markers.py` checks the block format with Klipper's and Kalico's own
//! parsers.

use crate::git::{Git, GitError};
use std::path::Path;

/// The first line of a needs-decision block. It cannot change: printerkeep finds blocks with it.
pub const BLOCK_START: &str = "#### printerkeep: needs decision ##########";
/// The last line of a needs-decision block. It cannot change.
pub const BLOCK_END: &str = "#### printerkeep: end ######################";
/// The line in a block before the other lines starts with one of these.
const OTHER_VERSION: [&[u8]; 2] = [b"#### Shared version.", b"#### Repository version."];

/// Where the other version of a merge comes from. The block text names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The `shared` branch.
    Shared,
    /// The printer's own branch in the repository (for example, an edit in the web pages of the git host).
    Repository,
}

impl Source {
    /// "Shared changed …" / "The repository changed …"
    fn changer(self) -> &'static str {
        match self {
            Source::Shared => "Shared",
            Source::Repository => "The repository",
        }
    }

    /// "Shared version." / "Repository version."
    fn version(self) -> &'static str {
        match self {
            Source::Shared => "Shared version.",
            Source::Repository => "Repository version.",
        }
    }
}
/// Lines that start with this are printerkeep's block lines.
const BLOCK_PREFIX: &[u8] = b"#### printerkeep:";

/// Long markers, so that git's markers do not match lines of a configuration file.
const MARKER_SIZE: usize = 41;

#[derive(Debug, PartialEq, Eq)]
pub enum Merged {
    /// The changes touch different lines. The result has both.
    Clean(Vec<u8>),
    /// The changes touch the same lines in some places. The result has a needs-decision block at each place.
    NeedsDecision(Vec<u8>),
}

/// Merges the changes from `base` to `shared` into `mine`. `scratch` is a folder for temporary files,
/// outside the config folder. The result has the line endings of `mine`.
pub fn merge(
    git: &Git,
    scratch: &Path,
    mine: &[u8],
    base: &[u8],
    shared: &[u8],
    source: Source,
) -> Result<Merged, GitError> {
    let crlf = uses_crlf(mine);
    let missing_end = !mine.is_empty() && !mine.ends_with(b"\n");
    let inputs = [mine, base, shared].map(|content| with_line_end(&to_lf(content)));

    std::fs::create_dir_all(scratch)?;
    let files = ["mine", "base", "shared"].map(|name| scratch.join(format!("merge-{name}")));
    for (file, content) in files.iter().zip(&inputs) {
        std::fs::write(file, content)?;
    }
    let result = git.merge_file(&files[0], &files[1], &files[2], MARKER_SIZE);
    for file in &files {
        let _ = std::fs::remove_file(file);
    }
    let (output, conflicts) = result?;

    let (mut content, blocks) = if conflicts == 0 {
        (output, 0)
    } else {
        render_blocks(&output, source)?
    };
    if missing_end && content.ends_with(b"\n") && !shared.ends_with(b"\n") {
        content.pop();
    }
    if crlf {
        content = to_crlf(&content);
    }
    Ok(if blocks == 0 {
        Merged::Clean(content)
    } else {
        Merged::NeedsDecision(content)
    })
}

/// `true` if the file has a needs-decision block (or a part of one).
pub fn has_block(content: &[u8]) -> bool {
    content
        .split(|&b| b == b'\n')
        .any(|line| line.starts_with(BLOCK_PREFIX))
}

/// Finishes all needs-decision blocks of a file: keeps this printer's lines, or uses the shared lines. Returns
/// `None` if a block is not complete (for example, the user changed a part of it).
pub fn decide(content: &[u8], use_shared: bool) -> Option<Vec<u8>> {
    enum Part {
        Outside,
        Mine,
        Shared,
    }
    let mut part = Part::Outside;
    let mut out = Vec::with_capacity(content.len());
    for line in content.split_inclusive(|&b| b == b'\n') {
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        let text = text.strip_suffix(b"\r").unwrap_or(text);
        match part {
            Part::Outside if text == BLOCK_START.as_bytes() => part = Part::Mine,
            Part::Outside if text.starts_with(BLOCK_PREFIX) => return None,
            Part::Outside => out.extend_from_slice(line),
            _ if text == BLOCK_END.as_bytes() => part = Part::Outside,
            _ if text.starts_with(BLOCK_PREFIX) => return None,
            _ if OTHER_VERSION.iter().any(|v| text.starts_with(v)) => part = Part::Shared,
            _ if text.starts_with(b"####") => {}
            Part::Mine => {
                if !use_shared {
                    out.extend_from_slice(line);
                }
            }
            Part::Shared => {
                let shared = line.strip_prefix(b"#")?;
                if use_shared {
                    out.extend_from_slice(shared);
                }
            }
        }
    }
    matches!(part, Part::Outside).then_some(out)
}

/// Changes git's conflict parts into needs-decision blocks. Returns the content and the number of blocks.
///
/// git marks changes on lines next to each other as a conflict. In configuration files, that is common (two
/// options on lines next to each other). Thus, printerkeep looks into each part again: lines that are the
/// same on both sides are merged, and when no lines were added or removed, each line is merged alone. Only
/// lines that both sides changed get a block.
fn render_blocks(output: &[u8], source: Source) -> Result<(Vec<u8>, usize), GitError> {
    let start = format!("{} mine", "<".repeat(MARKER_SIZE));
    let middle_base = format!("{} base", "|".repeat(MARKER_SIZE));
    let middle = "=".repeat(MARKER_SIZE);
    let end = format!("{} shared", ">".repeat(MARKER_SIZE));
    let unexpected = || GitError::Unexpected {
        command: "merge-file".into(),
    };

    enum Part {
        Outside,
        Mine,
        Base,
        Shared,
    }
    let mut part = Part::Outside;
    let mut content = Vec::new();
    let mut lines: [Vec<&[u8]>; 3] = Default::default();
    let mut blocks = 0;
    for line in output.split_inclusive(|&b| b == b'\n') {
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        match part {
            Part::Outside if text == start.as_bytes() => part = Part::Mine,
            Part::Outside => content.extend_from_slice(line),
            Part::Mine if text == middle_base.as_bytes() => part = Part::Base,
            Part::Mine => lines[0].push(text),
            Part::Base if text == middle.as_bytes() => part = Part::Shared,
            Part::Base => lines[1].push(text),
            Part::Shared if text == end.as_bytes() => {
                let [mine, base, shared] = &lines;
                blocks += resolve(mine, base, shared, source, &mut content);
                lines = Default::default();
                part = Part::Outside;
            }
            Part::Shared => lines[2].push(text),
        }
    }
    if !matches!(part, Part::Outside) {
        return Err(unexpected());
    }
    Ok((content, blocks))
}

/// Writes one conflict part: merged lines, and blocks for the lines that both sides changed. Returns the
/// number of blocks.
fn resolve(
    mine: &[&[u8]],
    base: &[&[u8]],
    shared: &[&[u8]],
    source: Source,
    out: &mut Vec<u8>,
) -> usize {
    let push = |out: &mut Vec<u8>, line: &[u8]| {
        out.extend_from_slice(line);
        out.push(b'\n');
    };
    if mine.len() == base.len() && shared.len() == base.len() {
        // Lines were only changed: merge each line alone.
        let mut blocks = 0;
        let (mut my_lines, mut shared_lines) = (Vec::new(), Vec::new());
        for i in 0..base.len() {
            let line = if mine[i] == base[i] || mine[i] == shared[i] {
                Some(shared[i])
            } else if shared[i] == base[i] {
                Some(mine[i])
            } else {
                None
            };
            match line {
                Some(line) => {
                    if !my_lines.is_empty() {
                        out.extend_from_slice(block(&my_lines, &shared_lines, source).as_bytes());
                        blocks += 1;
                        my_lines.clear();
                        shared_lines.clear();
                    }
                    push(out, line);
                }
                None => {
                    my_lines.push(mine[i]);
                    shared_lines.push(shared[i]);
                }
            }
        }
        if !my_lines.is_empty() {
            out.extend_from_slice(block(&my_lines, &shared_lines, source).as_bytes());
            blocks += 1;
        }
        return blocks;
    }
    // Lines that are the same at the start and at the end of both sides are merged.
    let prefix = mine.iter().zip(shared).take_while(|(a, b)| a == b).count();
    let suffix = mine[prefix..]
        .iter()
        .rev()
        .zip(shared[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    for line in &mine[..prefix] {
        push(out, line);
    }
    let (my_rest, shared_rest) = (
        &mine[prefix..mine.len() - suffix],
        &shared[prefix..shared.len() - suffix],
    );
    // When the rest of one side is the base, only the other side changed it. (Real case: both added the same
    // retract lines, and only the printer commented out the line after them.)
    let blocks = if my_rest == shared_rest || shared_rest == base {
        for line in my_rest {
            push(out, line);
        }
        0
    } else if my_rest == base {
        for line in shared_rest {
            push(out, line);
        }
        0
    } else {
        out.extend_from_slice(block(my_rest, shared_rest, source).as_bytes());
        1
    };
    for line in &mine[mine.len() - suffix..] {
        push(out, line);
    }
    blocks
}

/// One needs-decision block. The shared lines get a `#` with no space, so that removing the `#` gives
/// exactly the shared line.
fn block(mine: &[&[u8]], shared: &[&[u8]], source: Source) -> String {
    let (changer, version) = (source.changer(), source.version());
    let lines = |n: usize| if n == 1 { "line" } else { "lines" };
    let text = |line: &[u8]| String::from_utf8_lossy(line).into_owned();
    let mut out = vec![BLOCK_START.to_string()];
    if mine.is_empty() {
        out.push(format!(
            "#### {changer} changed lines that this printer removed."
        ));
        out.push("#### Active now (this printer): no lines.".into());
    } else {
        let these = if mine.len() == 1 {
            "this line"
        } else {
            "these lines"
        };
        out.push(format!("#### {changer} changed {these} too."));
        out.push("#### Active now (this printer):".into());
        out.extend(mine.iter().map(|l| text(l)));
    }
    match (shared.len(), mine.len()) {
        (0, m) => {
            out.push(format!("#### {version} No lines. To use it,"));
            out.push(format!("#### delete the {} above.", lines(m)));
        }
        (s, 0) => {
            out.push(format!("#### {version} To use it, remove the"));
            out.push(format!("#### # from the next {}:", lines(s)));
        }
        (s, m) => {
            out.push(format!("#### {version} To use it, remove the"));
            out.push(format!("#### # from the next {} and delete the", lines(s)));
            out.push(format!("#### {} above:", lines(m)));
        }
    }
    out.extend(shared.iter().map(|l| format!("#{}", text(l))));
    out.push("#### When done, delete all #### lines.".into());
    out.push(BLOCK_END.to_string());
    let mut block = out.join("\n");
    block.push('\n');
    block
}

/// `true` if most line ends are CRLF.
pub fn uses_crlf(content: &[u8]) -> bool {
    let lines = content.iter().filter(|&&b| b == b'\n').count();
    let crlf = content.windows(2).filter(|w| w == b"\r\n").count();
    lines > 0 && crlf * 2 > lines
}

/// The content with CRLF line ends changed to LF.
pub fn to_lf(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len());
    let mut bytes = content.iter().peekable();
    while let Some(&b) = bytes.next() {
        if b == b'\r' && bytes.peek() == Some(&&b'\n') {
            continue;
        }
        out.push(b);
    }
    out
}

/// The content with LF line ends changed to CRLF.
pub fn to_crlf(content: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(content.len() + content.len() / 20);
    for &b in &to_lf(content) {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

fn with_line_end(content: &[u8]) -> Vec<u8> {
    let mut content = content.to_vec();
    if !content.is_empty() && !content.ends_with(b"\n") {
        content.push(b'\n');
    }
    content
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(mine: &str, base: &str, shared: &str) -> Merged {
        let dir = tempfile::tempdir().unwrap();
        let git = Git::new(dir.path().join("repo.git"));
        merge(
            &git,
            &dir.path().join("scratch"),
            mine.as_bytes(),
            base.as_bytes(),
            shared.as_bytes(),
            Source::Shared,
        )
        .unwrap()
    }

    fn text(merged: Merged) -> String {
        match merged {
            Merged::Clean(c) | Merged::NeedsDecision(c) => String::from_utf8(c).unwrap(),
        }
    }

    /// The files that printerkeep writes must be exactly the files that the Klipper parser check uses.
    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/klipper/cases")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn changes_on_different_lines_merge() {
        let base = "[a]\nx: 1\ny: 2\nz: 3\n";
        let mine = "[a]\nx: 10\ny: 2\nz: 3\n";
        let shared = "[a]\nx: 1\ny: 2\nz: 30\n";
        assert_eq!(
            run(mine, base, shared),
            Merged::Clean(b"[a]\nx: 10\ny: 2\nz: 30\n".to_vec())
        );
    }

    #[test]
    fn same_line_gives_the_tested_block() {
        let base = "[gcode_macro _KAMP_Settings]\ndescription: Settings of printer 1\n\
                    variable_mesh_margin: 5\nvariable_purge_margin: 10\nvariable_purge_amount: 40\ngcode:\n";
        let mine = base.replace("purge_margin: 10", "purge_margin: 20");
        let shared = base.replace("purge_margin: 10", "purge_margin: 12");
        let merged = run(&mine, base, &shared);
        assert!(matches!(merged, Merged::NeedsDecision(_)));
        assert_eq!(text(merged), fixture("option-value.cfg"));
    }

    #[test]
    fn gcode_lines_give_the_tested_block() {
        let base = fixture("gcode-block.mine.cfg").replace(
            "    G28\n    BED_MESH_CALIBRATE\n",
            "    G28\n    BED_MESH_CALIBRATE ADAPTIVE=0\n",
        );
        let mine = base.replace("    LINE_PURGE\n", "    # Purge\n    LINE_PURGE\n");
        let mine = mine.replace(
            "    G28\n    BED_MESH_CALIBRATE ADAPTIVE=0\n",
            "    G28\n    BED_MESH_CALIBRATE\n",
        );
        let shared = fixture("gcode-block.shared.cfg");
        assert_eq!(text(run(&mine, &base, &shared)), fixture("gcode-block.cfg"));
    }

    #[test]
    fn removed_lines_give_the_tested_block() {
        let base = "[filament_switch_sensor runout]\nswitch_pin: ^PG12   # printerkeep: override\n\
                    pause_on_runout: True\nrunout_gcode: PAUSE\nevent_delay: 3.0\n";
        let mine = base.replace("runout_gcode: PAUSE\n", "");
        let shared = base.replace("runout_gcode: PAUSE", "runout_gcode: M600");
        assert_eq!(
            text(run(&mine, base, &shared)),
            fixture("no-active-lines.cfg")
        );
    }

    #[test]
    fn changes_on_lines_next_to_each_other_merge() {
        let base = "[gcode_macro _KAMP_Settings]\nvariable_a: 1\nvariable_b: 2\nvariable_c: 3\n";
        let mine = base.replace("variable_a: 1", "variable_a: 10");
        let shared = base.replace("variable_b: 2", "variable_b: 20");
        assert_eq!(
            text(run(&mine, base, &shared)),
            base.replace("variable_a: 1", "variable_a: 10")
                .replace("variable_b: 2", "variable_b: 20")
        );
        assert!(matches!(run(&mine, base, &shared), Merged::Clean(_)));

        // One line changed on both sides gets a block. The line next to it merges.
        let mine = base
            .replace("variable_a: 1", "variable_a: 10")
            .replace("variable_b: 2", "variable_b: 5");
        let merged = run(&mine, base, &shared);
        let content = text(merged);
        assert!(
            content.starts_with(
                "[gcode_macro _KAMP_Settings]\nvariable_a: 10\n#### printerkeep: needs decision"
            ),
            "{content}"
        );
        assert!(
            content.contains("#### Active now (this printer):\nvariable_b: 5\n"),
            "{content}"
        );
        assert!(content.contains("\n#variable_b: 20\n"), "{content}");
        assert!(
            content.ends_with("#### printerkeep: end ######################\nvariable_c: 3\n"),
            "{content}"
        );
    }

    #[test]
    fn same_lines_at_the_ends_of_a_part_are_merged() {
        let base = "[a]\nx: 1\n";
        // Both add the same first line, and different second lines.
        let mine = "[a]\nx: 1\ny: 2\nz: 3\n";
        let shared = "[a]\nx: 1\ny: 2\nz: 4\nw: 5\n";
        let content = text(run(mine, base, shared));
        assert!(
            content.starts_with("[a]\nx: 1\ny: 2\n#### printerkeep: needs decision"),
            "{content}"
        );
        assert!(
            content.contains("#### Active now (this printer):\nz: 3\n"),
            "{content}"
        );
        assert!(content.contains("\n#z: 4\n#w: 5\n"), "{content}");
    }

    #[test]
    fn same_added_lines_then_one_side_changed_the_next_line() {
        let base = "gcode:\n    _RUNOUT_OFF\n    CLEAR_PAUSE\n";
        let mine = "gcode:\n    {% set r = 20 %}\n    # _RUNOUT_OFF\n    CLEAR_PAUSE\n";
        let shared = "gcode:\n    {% set r = 20 %}\n    _RUNOUT_OFF\n    CLEAR_PAUSE\n";
        assert_eq!(
            run(mine, base, shared),
            Merged::Clean(mine.as_bytes().to_vec())
        );
    }

    #[test]
    fn crlf_stays_crlf() {
        let base = "[a]\r\nx: 1\r\ny: 2\r\nz: 3\r\n";
        let mine = "[a]\r\nx: 10\r\ny: 2\r\nz: 3\r\n";
        let shared = "[a]\nx: 1\ny: 2\nz: 30\n";
        assert_eq!(
            text(run(mine, base, shared)),
            "[a]\r\nx: 10\r\ny: 2\r\nz: 30\r\n"
        );
    }

    #[test]
    fn decide_gives_mine_or_shared() {
        for case in [
            "option-value",
            "gcode-block",
            "no-active-lines",
            "section-header",
            "save-config",
        ] {
            let with_block = fixture(&format!("{case}.cfg"));
            let mine = decide(with_block.as_bytes(), false).unwrap();
            let shared = decide(with_block.as_bytes(), true).unwrap();
            assert!(!has_block(&mine) && !has_block(&shared), "{case}");
            let crlf = to_crlf(with_block.as_bytes());
            assert_eq!(decide(&crlf, true).unwrap(), to_crlf(&shared), "{case}");
        }
        let option_value = fixture("option-value.cfg");
        assert_eq!(
            String::from_utf8(decide(option_value.as_bytes(), true).unwrap()).unwrap(),
            fixture("option-value.shared.cfg")
        );
        assert_eq!(
            String::from_utf8(decide(option_value.as_bytes(), false).unwrap()).unwrap(),
            fixture("option-value.mine.cfg")
        );
        // An incomplete block: the user deleted the end line.
        let broken = option_value.replace(BLOCK_END, "");
        assert_eq!(decide(broken.as_bytes(), true), None);
    }

    #[test]
    fn blocks_are_found() {
        assert!(has_block(fixture("option-value.cfg").as_bytes()));
        assert!(!has_block(b"#### Macros ####\n[gcode_macro A]\n"));
    }
}
