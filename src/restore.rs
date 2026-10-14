//! Restore: which backup to take, and which files of it to write. The commands are in `app::restore`.

use crate::paths::SETTINGS_FILE;
use crate::state::STATE_FILE;

/// A backup that has the file (or the files) to restore.
pub type Version = crate::git::LogEntry;

impl Version {
    /// The local time without seconds, for messages.
    pub fn shown_time(&self) -> &str {
        self.local_time
            .get(..16)
            .unwrap_or(self.local_time.as_str())
    }

    /// The local date, `YYYY-MM-DD`.
    pub fn date(&self) -> &str {
        self.local_time
            .get(..10)
            .unwrap_or(self.local_time.as_str())
    }
}

/// What to restore.
#[derive(Debug, PartialEq, Eq)]
pub enum Choice<'a> {
    /// Take this version without a question.
    Use(&'a Version),
    /// Show these versions (newest first). `default` is the one that Enter takes.
    Ask {
        list: Vec<&'a Version>,
        default: Option<usize>,
    },
}

/// The most versions that a list shows.
pub const LIST_LENGTH: usize = 10;

/// The choice for a time: `on_day` has the versions of that day (newest first), `at_or_before` is the
/// newest version at or before that time. With 2 or more versions on that day, the user picks, and Enter
/// takes `at_or_before`. Otherwise printerkeep takes `at_or_before`.
pub fn choose_at<'a>(on_day: &'a [Version], at_or_before: &'a Version) -> Choice<'a> {
    if on_day.len() < 2 {
        return Choice::Use(at_or_before);
    }
    let mut list: Vec<&Version> = on_day.iter().take(LIST_LENGTH).collect();
    if !list.iter().any(|v| v.commit == at_or_before.commit) {
        list.push(at_or_before);
    }
    let default = list.iter().position(|v| v.commit == at_or_before.commit);
    Choice::Ask { list, default }
}

/// The question for a list of versions: one line for each, then the question on the last line.
pub fn question(title: &str, list: &[&Version], default: Option<usize>) -> String {
    let mut text = format!("{title}\n");
    for (index, version) in list.iter().enumerate() {
        text.push_str(&format!(
            "  {}  {}  {}\n",
            index + 1,
            version.shown_time(),
            version.subject
        ));
    }
    match default {
        Some(index) => text.push_str(&format!(
            "Which one do you want? Type its number, or press Enter for {}",
            index + 1
        )),
        None => text.push_str("Which one do you want? Type its number"),
    }
    text
}

/// The index that an answer selects. `None` if the answer isn't one of the numbers (or is empty without a
/// default).
pub fn answer(text: &str, len: usize, default: Option<usize>) -> Option<usize> {
    let text = text.trim();
    if text.is_empty() {
        return default;
    }
    text.parse::<usize>()
        .ok()
        .filter(|n| (1..=len).contains(n))
        .map(|n| n - 1)
}

/// Where the files come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// This printer's history.
    Own,
    /// Another printer, or another repository.
    Other,
    /// A new install: printerkeep continues the history of the repository's branch.
    NewInstall,
}

/// `true` if a restore writes `path`. `named`: the user named this file.
///
/// The state of the followed files comes back only with a new install: an older state would merge changes
/// from shared again. printerkeep.conf names this printer and its repository: it comes back with a new
/// install, or when the user names it in this printer's own history.
pub fn writes(path: &[u8], named: bool, source: Source) -> bool {
    if path == STATE_FILE.as_bytes() {
        source == Source::NewInstall
    } else if path == SETTINGS_FILE.as_bytes() {
        source == Source::NewInstall || (named && source == Source::Own)
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(n: u64) -> Version {
        Version {
            commit: format!("c{n}"),
            time: n,
            local_time: format!("2026-10-12 0{n}:00:00"),
            subject: format!("Backup {n}"),
        }
    }

    #[test]
    fn one_or_no_version_on_that_day_is_taken_without_a_question() {
        let older = version(1);
        assert_eq!(choose_at(&[], &older), Choice::Use(&older));
        let on_day = [version(2)];
        assert_eq!(choose_at(&on_day, &on_day[0]), Choice::Use(&on_day[0]));
        // The only backup of that day is after the time: the state then is the older one.
        assert_eq!(choose_at(&on_day, &older), Choice::Use(&older));
    }

    #[test]
    fn several_versions_on_that_day_are_listed() {
        let on_day = [version(3), version(2), version(1)];
        let Choice::Ask { list, default } = choose_at(&on_day, &on_day[1]) else {
            panic!("no list");
        };
        assert_eq!(list.len(), 3);
        assert_eq!(default, Some(1));
    }

    #[test]
    fn the_state_at_that_time_is_in_the_list_also_from_an_earlier_day() {
        let on_day = [version(4), version(3)];
        let older = version(1);
        let Choice::Ask { list, default } = choose_at(&on_day, &older) else {
            panic!("no list");
        };
        assert_eq!(list.len(), 3);
        assert_eq!(default, Some(2));
    }

    #[test]
    fn answers() {
        assert_eq!(answer("", 3, Some(1)), Some(1));
        assert_eq!(answer(" 3 ", 3, Some(1)), Some(2));
        assert_eq!(answer("4", 3, Some(1)), None);
        assert_eq!(answer("0", 3, None), None);
        assert_eq!(answer("x", 3, None), None);
        assert_eq!(answer("", 3, None), None);
    }

    #[test]
    fn the_question_ends_with_the_question() {
        let (a, b) = (version(2), version(1));
        let text = question("printer.cfg has these versions:", &[&a, &b], Some(0));
        assert_eq!(
            text,
            "printer.cfg has these versions:\n\
             \x20 1  2026-10-12 02:00  Backup 2\n\
             \x20 2  2026-10-12 01:00  Backup 1\n\
             Which one do you want? Type its number, or press Enter for 1"
        );
    }

    #[test]
    fn the_state_file_and_the_settings_come_back_only_when_they_should() {
        let state = STATE_FILE.as_bytes();
        let settings = SETTINGS_FILE.as_bytes();
        assert!(!writes(state, true, Source::Own));
        assert!(!writes(state, false, Source::Other));
        assert!(writes(state, false, Source::NewInstall));
        assert!(!writes(settings, false, Source::Own));
        assert!(writes(settings, true, Source::Own));
        assert!(!writes(settings, true, Source::Other));
        assert!(writes(settings, false, Source::NewInstall));
        assert!(writes(b"printer.cfg", false, Source::Other));
    }
}
