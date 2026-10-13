//! M5: property tests for the merge rules (C4, C5, C6), with random configuration files.
//!
//! The files are `option: value` lines. Each side changes some values. The expected result per line is the
//! three-way rule: the side that changed the line wins; if both changed it to different values, the line needs
//! decision.

use printerkeep::git::Git;
use printerkeep::merge::{self, Merged, Source};
use printerkeep::overrides;
use proptest::prelude::*;

const OVERRIDE: &str = "   # printerkeep: override";

/// One merge with the real `git merge-file`.
fn run(mine: &str, base: &str, shared: &str) -> Merged {
    let dir = tempfile::tempdir().unwrap();
    let git = Git::new(dir.path().join("repo.git"));
    merge::merge(
        &git,
        dir.path(),
        mine.as_bytes(),
        base.as_bytes(),
        shared.as_bytes(),
        Source::Shared,
    )
    .unwrap()
}

fn file(values: &[u8], override_line: Option<usize>) -> String {
    let mut text = String::from("[gcode_macro _SETTINGS]\n");
    for (i, value) in values.iter().enumerate() {
        text.push_str(&format!("variable_{i}: {value}"));
        if override_line == Some(i) {
            text.push_str(OVERRIDE);
        }
        text.push('\n');
    }
    text
}

/// The configuration that Klipper reads: it deletes all text from the first `#` on each line. Empty lines do
/// nothing.
fn klipper_view(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split('#').next().unwrap_or("").trim_end().to_string())
        .filter(|line| !line.trim().is_empty())
        .collect()
}

fn text(merged: &Merged) -> String {
    match merged {
        Merged::Clean(c) | Merged::NeedsDecision(c) => String::from_utf8(c.clone()).unwrap(),
    }
}

/// Base values, and for each side a new value or none (no change). Small value ranges, so that both sides
/// often change the same line, sometimes to the same value.
fn sides() -> impl Strategy<Value = (Vec<u8>, Vec<Option<u8>>, Vec<Option<u8>>)> {
    (3usize..12).prop_flat_map(|n| {
        (
            prop::collection::vec(0u8..3, n),
            prop::collection::vec(prop::option::weighted(0.4, 0u8..4), n),
            prop::collection::vec(prop::option::weighted(0.4, 0u8..4), n),
        )
    })
}

fn apply(base: &[u8], changes: &[Option<u8>]) -> Vec<u8> {
    base.iter()
        .zip(changes)
        .map(|(b, c)| c.unwrap_or(*b))
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    /// C4: changes on different lines merge, and the result has both. Lines that both sides changed (to
    /// different values) need decision; the user's two choices give exactly the expected files.
    #[test]
    fn merge_follows_the_three_way_rule((base, mine_changes, shared_changes) in sides()) {
        let mine = apply(&base, &mine_changes);
        let shared = apply(&base, &shared_changes);
        let conflict: Vec<bool> = (0..base.len())
            .map(|i| mine[i] != base[i] && shared[i] != base[i] && mine[i] != shared[i])
            .collect();
        let pick = |use_shared: bool| -> Vec<u8> {
            (0..base.len())
                .map(|i| {
                    if conflict[i] {
                        if use_shared { shared[i] } else { mine[i] }
                    } else if mine[i] != base[i] {
                        mine[i]
                    } else {
                        shared[i]
                    }
                })
                .collect()
        };

        let merged = run(&file(&mine, None), &file(&base, None), &file(&shared, None));
        let result = text(&merged);
        if conflict.iter().any(|&c| c) {
            prop_assert!(matches!(merged, Merged::NeedsDecision(_)), "{}", result);
            let keep = String::from_utf8(merge::decide(result.as_bytes(), false).unwrap()).unwrap();
            let take = String::from_utf8(merge::decide(result.as_bytes(), true).unwrap()).unwrap();
            prop_assert_eq!(keep, file(&pick(false), None));
            prop_assert_eq!(take, file(&pick(true), None));
        } else {
            prop_assert!(matches!(merged, Merged::Clean(_)), "{}", result);
            prop_assert_eq!(result, file(&pick(false), None));
        }
    }

    /// C5: a file that needs decision is always valid configuration: for Klipper, it is the same as the file
    /// with this printer's lines. printerkeep added only comments.
    #[test]
    fn needs_decision_adds_only_comments((base, mine_changes, shared_changes) in sides()) {
        let mine = apply(&base, &mine_changes);
        let shared = apply(&base, &shared_changes);
        let result = text(&run(&file(&mine, None), &file(&base, None), &file(&shared, None)));
        let keep = String::from_utf8(merge::decide(result.as_bytes(), false).unwrap()).unwrap();
        prop_assert_eq!(klipper_view(&result), klipper_view(&keep));
    }

    /// C6: an override line on the printer never changes, whatever shared and the base have on that line.
    #[test]
    fn override_lines_never_change(
        (base, mine_changes, shared_changes) in sides(),
        line in 0usize..3,
        shared_marked in any::<bool>(),
    ) {
        let mine_text = file(&apply(&base, &mine_changes), Some(line));
        let base_text = file(&base, None);
        let shared_text = file(&apply(&base, &shared_changes), shared_marked.then_some(line));
        let merged = run(
            &mine_text,
            &String::from_utf8(overrides::protect(mine_text.as_bytes(), base_text.as_bytes())).unwrap(),
            &String::from_utf8(overrides::protect(mine_text.as_bytes(), shared_text.as_bytes())).unwrap(),
        );
        let override_line = mine_text.lines().nth(line + 1).unwrap();
        let result = text(&merged);
        // Never in a block: both choices keep it.
        for use_shared in [false, true] {
            let decided = merge::decide(result.as_bytes(), use_shared).unwrap();
            let decided = String::from_utf8(decided).unwrap();
            prop_assert_eq!(decided.lines().nth(line + 1), Some(override_line), "{}", result);
        }
    }

    /// C4: lines that each side adds at different places (with at least one line between them that no side
    /// changed) merge, and the result has both.
    #[test]
    fn added_lines_at_different_places_merge(
        base in prop::collection::vec(0u8..3, 4..12),
        at in any::<prop::sample::Index>(),
        gap in 1usize..4,
    ) {
        let mine_at = at.index(base.len() - gap);
        let shared_at = mine_at + gap;
        let base_text = file(&base, None);
        let insert = |after: usize, line: &str| -> String {
            let mut lines: Vec<&str> = base_text.lines().collect();
            // Line 0 is the section header.
            lines.insert(after + 2, line);
            lines.iter().map(|l| format!("{l}\n")).collect()
        };
        let mine = insert(mine_at, "variable_mine: 1");
        let shared = insert(shared_at, "variable_shared: 1");
        let merged = run(&mine, &base_text, &shared);
        let result = text(&merged);
        prop_assert!(matches!(merged, Merged::Clean(_)), "{}", result);
        let mut expected: Vec<&str> = base_text.lines().collect();
        expected.insert(shared_at + 2, "variable_shared: 1");
        expected.insert(mine_at + 2, "variable_mine: 1");
        let expected: String = expected.iter().map(|l| format!("{l}\n")).collect();
        prop_assert_eq!(result, expected);
    }

    /// The result has the line ends of the printer's file.
    #[test]
    fn crlf_files_stay_crlf((base, mine_changes, shared_changes) in sides()) {
        let mine = file(&apply(&base, &mine_changes), None).replace('\n', "\r\n");
        let shared = file(&apply(&base, &shared_changes), None);
        let result = text(&run(&mine, &file(&base, None), &shared));
        prop_assert!(!result.replace("\r\n", "").contains('\n'), "{:?}", result);
    }
}
