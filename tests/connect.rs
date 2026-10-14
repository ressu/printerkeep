//! M7: connect and restore find which printer in the repository this is. These tests call the library
//! with fixed answers, because a test has no terminal.

mod common;

use common::*;
use printerkeep::app::{self, Ask, ConnectChoices, Error, TokenRequest};
use printerkeep::paths::Paths;
use std::cell::RefCell;
use std::path::Path;

/// Fixed answers. `None` answers mean "no terminal". Records the questions.
#[derive(Default)]
struct Answers {
    confirm: Option<bool>,
    text: Option<String>,
    asked: RefCell<Vec<String>>,
}

impl Ask for Answers {
    fn confirm(&self, question: &str) -> Option<bool> {
        self.asked.borrow_mut().push(question.to_string());
        self.confirm
    }

    fn text(&self, question: &str) -> Option<String> {
        self.asked.borrow_mut().push(question.to_string());
        self.text.clone()
    }
}

fn no_token(_: TokenRequest, _: &str) -> std::io::Result<String> {
    panic!("a folder repository needs no token")
}

fn paths(printer: &Printer) -> Paths {
    Paths::new(printer.dir.path())
}

fn add(
    printer: &Printer,
    url: &str,
    choices: &ConnectChoices,
    ask: &Answers,
) -> Result<app::ConnectReport, Error> {
    app::connect(&paths(printer), url, choices, &no_token, ask)
}

fn kept_refs(printer: &Printer) -> Vec<String> {
    printer
        .git(&[
            "for-each-ref",
            "--format=%(refname)",
            "refs/printerkeep/kept/",
        ])
        .lines()
        .map(str::to_string)
        .collect()
}

/// The real case: printerkeep's local data was removed, printerkeep.conf kept the name. A new install has a
/// new history; the repository's branch has the old one. On a terminal, the user answers yes.
#[test]
fn a_reinstalled_printer_continues_its_branch_after_a_yes() {
    let (_dir, url) = repository();
    let before = Printer::connected("printer-1", &url);
    drop(before);
    let old_tip = remote_tip(&url, "printer-1");

    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "[printer]\n# printer-1\n");
    printer.ok(&["backup"]);

    let ask = Answers {
        confirm: Some(true),
        ..Answers::default()
    };
    let report = add(&printer, &url, &ConnectChoices::default(), &ask).unwrap();
    assert!(ask.asked.borrow()[0].contains("Continue that history on this printer?"));
    assert!(
        report
            .lines()
            .iter()
            .any(|l| l.contains("printerkeep continues that history"))
    );
    let tip = remote_tip(&url, "printer-1");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &old_tip, &tip],
    );
    assert_eq!(kept_refs(&printer).len(), 1);

    // No: nothing changes, and the error names both ways.
    let other = Printer::named("printer-1");
    other.write("printer.cfg", "[printer]\n");
    other.ok(&["backup"]);
    let ask = Answers {
        confirm: Some(false),
        ..Answers::default()
    };
    let error = add(&other, &url, &ConnectChoices::default(), &ask)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("--printer-name printer-1 --continue"),
        "{error}"
    );
    assert!(error.contains("--printer-name <another name>"), "{error}");
    assert_eq!(remote_tip(&url, "printer-1"), tip);
}

/// Installed as main, then connected as foobar by the answer to the question. Only foobar is on the
/// printer; the repository's main is not read or changed.
#[test]
fn install_as_main_then_connect_as_an_existing_printer() {
    let (_dir, url) = repository();
    let main = Printer::connected("main", &url);
    let foobar = Printer::connected("foobar", &url);
    drop((main, foobar));
    let main_tip = remote_tip(&url, "main");
    let foobar_tip = remote_tip(&url, "foobar");

    let printer = Printer::empty();
    printer.write("printer.cfg", "[printer]\n# new install\n");
    printer.ok(&["backup"]);
    let ask = Answers {
        confirm: Some(true),
        text: Some("foobar".to_string()),
        ..Answers::default()
    };
    let report = add(&printer, &url, &ConnectChoices::default(), &ask).unwrap();
    let asked = ask.asked.borrow();
    assert!(
        asked[0].starts_with(
            "The repository has these printers:\n  foobar\n  main\nThis printer's name is main (the default).\n"
        ),
        "{asked:?}"
    );
    assert!(
        report
            .lines()
            .contains(&"This printer's name is now foobar (it was main).".to_string())
    );

    let branches = printer.git(&["for-each-ref", "--format=%(refname)", "refs/heads/"]);
    assert_eq!(branches, "refs/heads/foobar");
    assert_eq!(kept_refs(&printer).len(), 1);
    assert_eq!(remote_tip(&url, "main"), main_tip);
    let tip = remote_tip(&url, "foobar");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &foobar_tip, &tip],
    );
    assert!(
        printer
            .read("printerkeep.conf")
            .contains("printer_name: foobar\n")
    );
}

/// No terminal: the default name main does not tell which printer this is, so printerkeep stops. A name
/// that the user chose is not a guess: a new branch.
#[test]
fn without_a_terminal_printerkeep_does_not_guess() {
    let (_dir, url) = repository();
    drop(Printer::connected("printer-1", &url));

    let printer = Printer::empty();
    printer.write("printer.cfg", "[printer]\n");
    let error = add(
        &printer,
        &url,
        &ConnectChoices::default(),
        &Answers::default(),
    )
    .unwrap_err();
    assert!(matches!(error, Error::WhichPrinter { .. }));
    let error = error.to_string();
    assert!(
        error.contains("This printer has the default name main"),
        "{error}"
    );
    assert!(error.contains("--printer-name <its name>"), "{error}");

    let named = Printer::named("printer-2");
    named.write("printer.cfg", "[printer]\n");
    add(
        &named,
        &url,
        &ConnectChoices::default(),
        &Answers::default(),
    )
    .unwrap();
    assert_eq!(remote_tip(&url, "printer-2").len(), 40);

    // --printer-name gives the name without printerkeep.conf.
    let choices = ConnectChoices {
        printer_name: Some("printer-3".to_string()),
        ..ConnectChoices::default()
    };
    add(&printer, &url, &choices, &Answers::default()).unwrap();
    assert_eq!(remote_tip(&url, "printer-3").len(), 40);
}

/// A new branch: "from now" sends one backup of the files now, and sets the local backups aside.
#[test]
fn from_now_sends_only_the_files_of_now() {
    let (_dir, url) = repository();
    let printer = Printer::named("printer-1");
    for n in 0..3 {
        printer.write("printer.cfg", &format!("[printer]\n# {n}\n"));
        printer.ok(&["backup"]);
    }
    let choices = ConnectChoices {
        from_now: true,
        ..ConnectChoices::default()
    };
    add(&printer, &url, &choices, &Answers::default()).unwrap();
    assert_eq!(
        git(Path::new(&url), &["rev-list", "--count", "printer-1"]),
        "1"
    );
    assert_eq!(kept_refs(&printer).len(), 1);

    // On a terminal, printerkeep asks when there is more than the install's first backup.
    let other = Printer::named("printer-2");
    for n in 0..2 {
        other.write("printer.cfg", &format!("[printer]\n# {n}\n"));
        other.ok(&["backup"]);
    }
    let ask = Answers {
        confirm: Some(false),
        text: Some(String::new()),
        ..Answers::default()
    };
    add(&other, &url, &ConnectChoices::default(), &ask).unwrap();
    let asked = ask.asked.borrow();
    assert!(asked[0].contains("Which printer is this?"), "{asked:?}");
    assert!(
        asked[1].contains("This printer has 2 backups."),
        "{asked:?}"
    );
    // The two backups, and the backup with the repository in printerkeep.conf.
    assert_eq!(
        git(Path::new(&url), &["rev-list", "--count", "printer-2"]),
        "3"
    );
}

/// Restore after a new install: printerkeep asks which printer; the install's first backup is set aside.
#[test]
fn restore_asks_which_printer_and_sets_the_install_backup_aside() {
    let (_dir, url) = repository();
    drop(Printer::connected("printer-1", &url));
    drop(Printer::connected("printer-2", &url));

    let printer = Printer::empty();
    printer.write("printer.cfg", "[printer]\n# default\n");
    printer.ok(&["backup"]);
    let ask = Answers {
        text: Some("printer-2".to_string()),
        ..Answers::default()
    };
    app::restore(&paths(&printer), &url, None, true, &no_token, &ask).unwrap();
    assert!(ask.asked.borrow()[0].contains("Which printer is this?"));
    assert_eq!(printer.read("printer.cfg"), "[printer]\n# printer-2\n");
    assert_eq!(
        printer.git(&["for-each-ref", "--format=%(refname)", "refs/heads/"]),
        "refs/heads/printer-2"
    );
    assert_eq!(kept_refs(&printer).len(), 1);
}
