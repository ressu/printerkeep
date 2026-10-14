//! Restore from the history: one file or all files, at a time (`--at`), from this printer, another printer
//! or another repository. The backups get fixed times with GIT_COMMITTER_DATE; TZ=UTC fixes the days.

mod common;

use common::*;
use printerkeep::app::{self, Ask, RestoreChoices, TokenRequest};
use printerkeep::paths::Paths;
use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

const HOUR: u64 = 3_600;
const DAY: u64 = 86_400;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

/// Midnight (UTC) of the day `days` days ago.
fn midnight(days: u64) -> u64 {
    (now() / DAY - days) * DAY
}

/// A backup with the time `time`.
fn backup_at(printer: &Printer, time: u64) {
    let date = format!("@{time} +0000");
    printer
        .command(&["backup"])
        .env("GIT_COMMITTER_DATE", &date)
        .env("GIT_AUTHOR_DATE", &date)
        .env("TZ", "UTC")
        .assert()
        .success();
}

/// printerkeep with TZ=UTC: expects success, returns stdout.
fn ok(printer: &Printer, args: &[&str]) -> String {
    let output = printer
        .command(args)
        .env("TZ", "UTC")
        .assert()
        .success()
        .get_output()
        .clone();
    String::from_utf8(output.stdout).unwrap()
}

/// printerkeep with TZ=UTC: expects failure, returns stderr.
fn fails(printer: &Printer, args: &[&str]) -> String {
    let output = printer
        .command(args)
        .env("TZ", "UTC")
        .assert()
        .failure()
        .get_output()
        .clone();
    String::from_utf8(output.stderr).unwrap()
}

/// The subjects of the last `n` backups of a branch, newest first.
fn subjects(printer: &Printer, branch: &str, n: usize) -> Vec<String> {
    printer
        .git(&["log", "--format=%s", &format!("-{n}"), branch])
        .lines()
        .map(str::to_string)
        .collect()
}

/// A fixed answer for each question (`None`: no terminal). Records the questions.
#[derive(Default)]
struct Answers {
    text: Option<String>,
    asked: RefCell<Vec<String>>,
}

impl Ask for Answers {
    fn confirm(&self, question: &str) -> Option<bool> {
        self.asked.borrow_mut().push(question.to_string());
        None
    }

    fn text(&self, question: &str) -> Option<String> {
        self.asked.borrow_mut().push(question.to_string());
        self.text.clone()
    }
}

fn no_token(_: TokenRequest, _: &str) -> std::io::Result<String> {
    panic!("a folder repository needs no token")
}

fn restore_file(printer: &Printer, file: &str, at: Option<&str>, ask: &Answers) {
    let choices = RestoreChoices {
        file: Some(file.to_string()),
        at: at.map(str::to_string),
        ..RestoreChoices::default()
    };
    app::restore(&Paths::new(printer.dir.path()), &choices, &no_token, ask).unwrap();
}

/// The main case: you edit a file, change your mind, and go back. The last version before that time is 10
/// days old, so printerkeep takes it without a question. Your edit stays in the history, and the output
/// gives the command that brings it back.
#[test]
fn one_file_from_an_older_backup_without_a_question() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    backup_at(&printer, now() - 10 * DAY);
    printer.write("printer.cfg", "v2\n");
    backup_at(&printer, now() - DAY);
    printer.write("printer.cfg", "v3\n");

    let output = ok(&printer, &["restore", "printer.cfg", "--at", "2 days ago"]);
    assert_eq!(printer.read("printer.cfg"), "v1\n");
    assert!(
        output.contains("Restored printer.cfg from the backup of "),
        "{output}"
    );
    assert!(
        output.contains("sudo systemctl restart klipper"),
        "{output}"
    );
    let subjects = subjects(&printer, "printer-1", 2);
    assert!(
        subjects[0].starts_with("Restored printer.cfg from the backup of "),
        "{subjects:?}"
    );
    // The edit from before the restore.
    assert_eq!(subjects[1], "Changed printer.cfg");

    let undo = output
        .lines()
        .skip_while(|line| !line.contains("To go back to it, run:"))
        .nth(1)
        .unwrap_or_else(|| panic!("no undo command:\n{output}"))
        .trim();
    let time = undo
        .strip_prefix("printerkeep restore printer.cfg --at \"")
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_else(|| panic!("{undo}"));
    ok(&printer, &["restore", "printer.cfg", "--at", time]);
    assert_eq!(printer.read("printer.cfg"), "v3\n");
}

/// Several backups on that day, and no terminal: the state at that time.
#[test]
fn several_backups_on_that_day_a_script_takes_the_state_at_that_time() {
    let printer = Printer::named("printer-1");
    let day = midnight(5);
    for (version, hour) in [("v1", 9), ("v2", 10), ("v3", 11)] {
        printer.write("printer.cfg", &format!("{version}\n"));
        backup_at(&printer, day + hour * HOUR);
    }
    printer.write("printer.cfg", "v4\n");
    backup_at(&printer, now() - DAY);

    let at = format!("@{}", day + 10 * HOUR + 30 * 60);
    ok(&printer, &["restore", "printer.cfg", "--at", &at]);
    assert_eq!(printer.read("printer.cfg"), "v2\n");
}

/// A date alone means the end of that day.
#[test]
fn a_date_alone_means_the_end_of_that_day() {
    let printer = Printer::named("printer-1");
    let day = midnight(5);
    for (version, hour) in [("v1", 9), ("v2", 10)] {
        printer.write("printer.cfg", &format!("{version}\n"));
        backup_at(&printer, day + hour * HOUR);
    }
    printer.write("printer.cfg", "v3\n");
    backup_at(&printer, now() - DAY);

    let date = printer.git(&[
        "log",
        "-1",
        "--format=%cd",
        "--date=format:%Y-%m-%d",
        "printer-1~1",
    ]);
    ok(&printer, &["restore", "printer.cfg", "--at", &date]);
    assert_eq!(printer.read("printer.cfg"), "v2\n");
}

/// On a terminal, printerkeep lists the backups of that day. Enter takes the state at that time.
#[test]
fn on_a_terminal_printerkeep_lists_the_backups_of_that_day() {
    let printer = Printer::named("printer-1");
    let day = midnight(5);
    for (version, hour) in [("v1", 9), ("v2", 10), ("v3", 11)] {
        printer.write("printer.cfg", &format!("{version}\n"));
        backup_at(&printer, day + hour * HOUR);
    }
    printer.write("printer.cfg", "v4\n");
    backup_at(&printer, now() - DAY);
    let at = format!("@{}", day + 10 * HOUR + 30 * 60);

    let enter = Answers {
        text: Some(String::new()),
        ..Answers::default()
    };
    restore_file(&printer, "printer.cfg", Some(&at), &enter);
    assert_eq!(printer.read("printer.cfg"), "v2\n");
    let question = enter.asked.borrow()[0].clone();
    assert!(question.contains("  1  "), "{question}");
    assert!(question.contains("  3  "), "{question}");
    assert!(!question.contains("  4  "), "{question}");
    assert!(question.ends_with("press Enter for 2"), "{question}");

    let first = Answers {
        text: Some("1".to_string()),
        ..Answers::default()
    };
    restore_file(&printer, "printer.cfg", Some(&at), &first);
    assert_eq!(printer.read("printer.cfg"), "v3\n");
}

/// Without a time, a terminal shows the last versions, but not the one that's in the file now.
#[test]
fn without_a_time_a_terminal_lists_the_earlier_versions() {
    let printer = Printer::named("printer-1");
    for (version, days) in [("v1", 3), ("v2", 2), ("v3", 1)] {
        printer.write("printer.cfg", &format!("{version}\n"));
        backup_at(&printer, now() - days * DAY);
    }

    let ask = Answers {
        text: Some("1".to_string()),
        ..Answers::default()
    };
    restore_file(&printer, "printer.cfg", None, &ask);
    assert_eq!(printer.read("printer.cfg"), "v2\n");
    let question = ask.asked.borrow()[0].clone();
    assert!(question.contains("  2  "), "{question}");
    assert!(!question.contains("  3  "), "{question}");
}

#[test]
fn without_a_time_a_script_gets_the_command_with_at() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    backup_at(&printer, now() - DAY);
    printer.write("printer.cfg", "v2\n");

    let error = fails(&printer, &["restore", "printer.cfg"]);
    assert!(
        error.contains("printerkeep restore printer.cfg --at \"1 hour ago\""),
        "{error}"
    );
    assert_eq!(printer.read("printer.cfg"), "v2\n");
}

/// All files: the files of that backup come back. Files added later stay, and printerkeep.conf isn't
/// restored unless you name it.
#[test]
fn all_files_go_back_and_later_files_stay() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    printer.write("macros.cfg", "m1\n");
    backup_at(&printer, now() - 10 * DAY);
    printer.write("printer.cfg", "v2\n");
    printer.write("macros.cfg", "m2\n");
    printer.write("later.cfg", "later\n");
    printer.write(
        "printerkeep.conf",
        "[printerkeep]\nprinter_name: printer-1\n# changed\n",
    );
    backup_at(&printer, now() - DAY);

    let output = ok(&printer, &["restore", "--at", "3 days ago"]);
    assert_eq!(printer.read("printer.cfg"), "v1\n");
    assert_eq!(printer.read("macros.cfg"), "m1\n");
    assert_eq!(printer.read("later.cfg"), "later\n");
    assert!(printer.read("printerkeep.conf").contains("# changed"));
    assert!(
        output.contains("Restored 2 files from the backup of "),
        "{output}"
    );
    assert!(output.contains("later.cfg"), "{output}");
}

#[test]
fn a_deleted_file_comes_back() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.write("old.cfg", "old\n");
    backup_at(&printer, now() - 10 * DAY);
    std::fs::remove_file(printer.config().join("old.cfg")).unwrap();
    backup_at(&printer, now() - DAY);

    ok(&printer, &["restore", "old.cfg", "--at", "3 days ago"]);
    assert_eq!(printer.read("old.cfg"), "old\n");
}

#[test]
fn the_same_content_changes_nothing() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    backup_at(&printer, now() - 10 * DAY);
    printer.write("printer.cfg", "v2\n");
    backup_at(&printer, now() - DAY);
    printer.write("printer.cfg", "v1\n");
    backup_at(&printer, now() - HOUR);
    let tip = printer.tip("printer-1");

    let output = ok(&printer, &["restore", "printer.cfg", "--at", "3 days ago"]);
    assert!(output.contains("already has"), "{output}");
    assert_eq!(printer.tip("printer-1"), tip);
}

#[test]
fn a_time_that_printerkeep_cant_read() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    backup_at(&printer, now() - DAY);

    let error = fails(&printer, &["restore", "printer.cfg", "--at", "blue moon"]);
    assert!(error.contains("Can't read the time 'blue moon'"), "{error}");
    assert!(error.contains("--at \"2 days ago\""), "{error}");
}

#[test]
fn a_time_before_the_first_backup() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    backup_at(&printer, now() - 10 * DAY);

    let error = fails(&printer, &["restore", "printer.cfg", "--at", "20 days ago"]);
    assert!(
        error.contains("The oldest backup of printer.cfg is from "),
        "{error}"
    );
}

#[test]
fn a_file_that_was_never_backed_up() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    backup_at(&printer, now() - DAY);

    let error = fails(&printer, &["restore", "nothing.cfg", "--at", "1 hour ago"]);
    assert!(error.contains("nothing.cfg isn't in any backup"), "{error}");
}

/// A file from another printer in the repository. This printer keeps its name, and records the restore
/// in its own history.
#[test]
fn a_file_from_another_printer() {
    let (_dir, url) = repository();
    let printer_1 = Printer::connected("printer-1", &url);
    let printer_2 = Printer::named("printer-2");
    printer_2.write("printer.cfg", "[printer]\n# printer-2\n");
    printer_2.write("macros.cfg", "# printer-2 macros\n");
    printer_2.ok(&["connect", &url]);

    ok(
        &printer_1,
        &["restore", "macros.cfg", "--printer-name", "printer-2"],
    );
    assert_eq!(printer_1.read("macros.cfg"), "# printer-2 macros\n");
    assert_eq!(printer_1.read("printer.cfg"), "[printer]\n# printer-1\n");
    assert!(
        printer_1
            .read("printerkeep.conf")
            .contains("printer_name: printer-1\n")
    );
    assert!(
        printer_1
            .last_backup("printer-1")
            .starts_with("Restored macros.cfg from the backup of printer-2 of "),
        "{}",
        printer_1.last_backup("printer-1")
    );
    assert_eq!(remote_tip(&url, "printer-1"), printer_1.tip("printer-1"));
    assert_eq!(
        printer_1.git(&["for-each-ref", "refs/printerkeep/restore-source"]),
        ""
    );
}

/// A file from another repository. The connection doesn't change.
#[test]
fn a_file_from_another_repository_keeps_the_connection() {
    let (_dir_a, url_a) = repository();
    let (_dir_b, url_b) = repository();
    let printer = Printer::connected("printer-1", &url_a);
    let other = Printer::named("printer-1");
    other.write("printer.cfg", "[printer]\n# from b\n");
    other.ok(&["connect", &url_b]);

    ok(
        &printer,
        &["restore", "printer.cfg", "--repository", &url_b],
    );
    assert_eq!(printer.read("printer.cfg"), "[printer]\n# from b\n");
    assert!(
        printer
            .read("printerkeep.conf")
            .contains(&format!("repository: {url_a}\n"))
    );
    assert_eq!(remote_tip(&url_a, "printer-1"), printer.tip("printer-1"));
}

/// The state of the followed files is never taken from an older backup.
#[test]
fn the_state_file_is_never_restored_from_an_older_backup() {
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "v1\n");
    printer.write(
        ".printerkeep-state.json",
        "{\n  \"version\": 1,\n  \"files\": {}\n}\n",
    );
    backup_at(&printer, now() - 10 * DAY);
    std::fs::remove_file(printer.config().join(".printerkeep-state.json")).unwrap();
    printer.write("printer.cfg", "v2\n");
    backup_at(&printer, now() - DAY);

    ok(&printer, &["restore", "--at", "3 days ago"]);
    assert_eq!(printer.read("printer.cfg"), "v1\n");
    assert!(!printer.config().join(".printerkeep-state.json").exists());

    let error = fails(
        &printer,
        &["restore", ".printerkeep-state.json", "--at", "3 days ago"],
    );
    assert!(error.contains("printerkeep keeps"), "{error}");
}
