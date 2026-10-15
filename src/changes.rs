//! The changes between two versions of a file, one by one, so that a copy to shared can send only some of
//! them. The changes come from `git diff -U0`: changes with unchanged lines between them are separate.

/// One change: lines of the old version that the new version replaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The first replaced line in the old version (from 0). Without removed lines: the line before which the
    /// added lines go.
    pub old_start: usize,
    /// The removed lines, with their line ends.
    pub removed: Vec<Vec<u8>>,
    /// The first added line in the new version (from 0). Without added lines: the line before which the
    /// removed lines were.
    pub new_start: usize,
    /// The added lines, with their line ends.
    pub added: Vec<Vec<u8>>,
}

impl Change {
    /// The change as diff lines: `-` for each removed line, `+` for each added line.
    pub fn lines(&self) -> String {
        let line = |sign: char, line: &Vec<u8>| {
            let text = String::from_utf8_lossy(line);
            format!("{sign}{}\n", text.trim_end_matches(['\n', '\r']))
        };
        self.removed
            .iter()
            .map(|l| line('-', l))
            .chain(self.added.iter().map(|l| line('+', l)))
            .collect()
    }
}

/// The lines of a text, each with its line end.
fn split(text: &[u8]) -> Vec<&[u8]> {
    text.split_inclusive(|&b| b == b'\n').collect()
}

/// The changes in `diff` (output of `git diff -U0` from `old` to `new`). The lines come from `old` and `new`,
/// so they keep their exact bytes.
pub fn parse(diff: &str, old: &[u8], new: &[u8]) -> Vec<Change> {
    let old_lines = split(old);
    let new_lines = split(new);
    let take = |lines: &[&[u8]], start: usize, count: usize| -> Vec<Vec<u8>> {
        lines
            .iter()
            .skip(start)
            .take(count)
            .map(|l| l.to_vec())
            .collect()
    };
    diff.lines()
        .filter_map(|line| {
            let ranges = line.strip_prefix("@@ -")?;
            let (old_range, rest) = ranges.split_once(" +")?;
            let (new_range, _) = rest.split_once(" @@")?;
            let (old_start, old_count) = range(old_range)?;
            let (new_start, new_count) = range(new_range)?;
            Some(Change {
                old_start,
                removed: take(&old_lines, old_start, old_count),
                new_start,
                added: take(&new_lines, new_start, new_count),
            })
        })
        .collect()
}

/// A range of a `@@` line (`start,count` or `start`), as the first line from 0 and the number of lines.
/// With no lines, git gives the line before the place, which is the line from 0 after it.
fn range(text: &str) -> Option<(usize, usize)> {
    let (start, count) = match text.split_once(',') {
        Some((start, count)) => (start.parse().ok()?, count.parse().ok()?),
        None => (text.parse().ok()?, 1),
    };
    Some(match count {
        0 => (start, 0),
        _ => (start.checked_sub(1)?, count),
    })
}

/// `old` with only the chosen changes (in the order of the file).
pub fn apply(old: &[u8], chosen: &[&Change]) -> Vec<u8> {
    let lines = split(old);
    let mut result = Vec::new();
    let mut at = 0;
    for change in chosen {
        let start = change.old_start.clamp(at, lines.len());
        result.extend(lines[at..start].concat());
        result.extend(change.added.concat());
        at = (start + change.removed.len()).min(lines.len());
    }
    result.extend(lines[at..].concat());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const OLD: &str = "a\nb\nc\nd\ne\n";
    const NEW: &str = "A\nb\nc\nx\nd\ne\nf\n";
    const DIFF: &str = "\
diff --git a/x b/x
@@ -1 +1 @@
-a
+A
@@ -3,0 +4 @@ c
+x
@@ -5,0 +7 @@ d
+f
";

    #[test]
    fn parses_each_change() {
        let changes = parse(DIFF, OLD.as_bytes(), NEW.as_bytes());
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].lines(), "-a\n+A\n");
        assert_eq!((changes[0].old_start, changes[0].new_start), (0, 0));
        assert_eq!(changes[1].lines(), "+x\n");
        assert_eq!((changes[1].old_start, changes[1].new_start), (3, 3));
        assert_eq!(changes[2].lines(), "+f\n");
        assert_eq!((changes[2].old_start, changes[2].new_start), (5, 6));
    }

    #[test]
    fn applies_only_the_chosen_changes() {
        let changes = parse(DIFF, OLD.as_bytes(), NEW.as_bytes());
        let all: Vec<&Change> = changes.iter().collect();
        assert_eq!(apply(OLD.as_bytes(), &all), NEW.as_bytes());
        assert_eq!(apply(OLD.as_bytes(), &[]), OLD.as_bytes());
        assert_eq!(apply(OLD.as_bytes(), &[&changes[0]]), b"A\nb\nc\nd\ne\n");
        assert_eq!(apply(OLD.as_bytes(), &[&changes[1]]), b"a\nb\nc\nx\nd\ne\n");
        assert_eq!(
            apply(OLD.as_bytes(), &[&changes[0], &changes[2]]),
            b"A\nb\nc\nd\ne\nf\n"
        );
    }

    #[test]
    fn removed_lines_and_crlf() {
        let old = b"a\r\nb\r\nc\r\n";
        let new = b"a\r\nc\r\n";
        let changes = parse("@@ -2 +1,0 @@\n-b\n", old, new);
        assert_eq!(changes[0].removed, [b"b\r\n".to_vec()]);
        assert_eq!(changes[0].lines(), "-b\n");
        assert_eq!(apply(old, &[&changes[0]]), new);
    }
}
