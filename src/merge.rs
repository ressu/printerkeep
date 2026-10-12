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
        render_blocks(&output)?
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

/// Changes git's conflict markers into needs-decision blocks. Returns the content and the number of blocks.
fn render_blocks(output: &[u8]) -> Result<(Vec<u8>, usize), GitError> {
    let start = format!("{} mine", "<".repeat(MARKER_SIZE));
    let middle = "=".repeat(MARKER_SIZE);
    let end = format!("{} shared", ">".repeat(MARKER_SIZE));
    let unexpected = || GitError::Unexpected {
        command: "merge-file".into(),
    };

    enum Part {
        Outside,
        Mine,
        Shared,
    }
    let mut part = Part::Outside;
    let mut content = Vec::new();
    let (mut mine, mut shared): (Vec<&[u8]>, Vec<&[u8]>) = (Vec::new(), Vec::new());
    let mut blocks = 0;
    for line in output.split_inclusive(|&b| b == b'\n') {
        let text = line.strip_suffix(b"\n").unwrap_or(line);
        match part {
            Part::Outside if text == start.as_bytes() => part = Part::Mine,
            Part::Outside => content.extend_from_slice(line),
            Part::Mine if text == middle.as_bytes() => part = Part::Shared,
            Part::Mine => mine.push(text),
            Part::Shared if text == end.as_bytes() => {
                content.extend_from_slice(block(&mine, &shared).as_bytes());
                mine.clear();
                shared.clear();
                blocks += 1;
                part = Part::Outside;
            }
            Part::Shared => shared.push(text),
        }
    }
    if !matches!(part, Part::Outside) {
        return Err(unexpected());
    }
    Ok((content, blocks))
}

/// One needs-decision block. The shared lines get a `#` with no space, so that removing the `#` gives
/// exactly the shared line.
fn block(mine: &[&[u8]], shared: &[&[u8]]) -> String {
    let lines = |n: usize| if n == 1 { "line" } else { "lines" };
    let text = |line: &[u8]| String::from_utf8_lossy(line).into_owned();
    let mut out = vec![BLOCK_START.to_string()];
    if mine.is_empty() {
        out.push("#### Shared changed lines that this printer removed.".into());
        out.push("#### Active now (this printer): no lines.".into());
    } else {
        let these = if mine.len() == 1 {
            "this line"
        } else {
            "these lines"
        };
        out.push(format!("#### Shared changed {these} too."));
        out.push("#### Active now (this printer):".into());
        out.extend(mine.iter().map(|l| text(l)));
    }
    match (shared.len(), mine.len()) {
        (0, m) => {
            out.push("#### Shared version. No lines. To use it,".into());
            out.push(format!("#### delete the {} above.", lines(m)));
        }
        (s, 0) => {
            out.push("#### Shared version. To use it, remove the".into());
            out.push(format!("#### # from the next {}:", lines(s)));
        }
        (s, m) => {
            out.push("#### Shared version. To use it, remove the".into());
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
    fn blocks_are_found() {
        assert!(has_block(fixture("option-value.cfg").as_bytes()));
        assert!(!has_block(b"#### Macros ####\n[gcode_macro A]\n"));
    }
}
