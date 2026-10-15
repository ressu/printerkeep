//! Copy to shared, change by change: a printer sends its fix and keeps its own values.

mod common;

use common::*;

use printerkeep::app::{self, ChangeQuestion, CopyChoice, How, Pick};
use printerkeep::paths::Paths;
use std::cell::RefCell;

const MACROS: &str = "\
[gcode_macro LOAD]
variable_temp: 230
gcode:
    M109 S{printer['gcode_macro LOAD'].temp}
    G1 E50 F300
";

/// Fixed answers. Records the changes it was asked about.
struct Answers {
    how: How,
    picks: RefCell<Vec<Pick>>,
    asked: RefCell<Vec<String>>,
}

impl Answers {
    fn new(how: How, picks: &[Pick]) -> Self {
        Self {
            how,
            picks: RefCell::new(picks.to_vec()),
            asked: RefCell::new(Vec::new()),
        }
    }
}

impl CopyChoice for Answers {
    fn how(&self, _preview: &str, _can_pick: bool) -> std::io::Result<How> {
        Ok(self.how)
    }

    fn change(&self, question: &ChangeQuestion) -> std::io::Result<Pick> {
        self.asked.borrow_mut().push(format!(
            "{} {}/{} {:?} {} {}",
            question.path,
            question.number,
            question.total,
            question.section,
            question.can_keep_always,
            question.lines
        ));
        Ok(self.picks.borrow_mut().remove(0))
    }
}

fn copy(printer: &Printer, answers: &Answers) -> Vec<String> {
    app::copy_to_shared(
        &Paths::new(printer.dir.path()),
        &["macros.cfg".to_string()],
        answers,
    )
    .unwrap()
}

/// Printer 1 copies the file to shared, printer 2 follows it with its own temperature.
fn two_printers(url: &str) -> (Printer, Printer, String) {
    let printer1 = Printer::connected("printer-1", url);
    let printer2 = Printer::connected("printer-2", url);
    printer1.write("macros.cfg", MACROS);
    printer1.ok(&["copy-to-shared", "--yes", "macros.cfg"]);
    let printer2_macros = MACROS.replace("variable_temp: 230", "variable_temp: 250");
    printer2.write("macros.cfg", &printer2_macros);
    printer2.ok(&["follow", "macros.cfg"]);
    (printer1, printer2, printer2_macros)
}

#[test]
fn a_kept_change_stays_on_this_printer() {
    let (_dir, url) = repository();
    let (printer1, printer2, printer2_macros) = two_printers(&url);

    // Printer 2 fixes the macro. It sends the fix and keeps its temperature.
    let fixed = printer2_macros.replace("G1 E50 F300", "G1 E60 F300");
    printer2.write("macros.cfg", &fixed);
    let answers = Answers::new(How::Pick, &[Pick::Keep, Pick::Send]);
    let lines = copy(&printer2, &answers).join("\n");
    assert_eq!(
        *answers.asked.borrow(),
        [
            "macros.cfg 1/2 Some(\"gcode_macro LOAD\") true -variable_temp: 230\n+variable_temp: 250\n",
            "macros.cfg 2/2 Some(\"gcode_macro LOAD\") false -    G1 E50 F300\n+    G1 E60 F300\n",
        ]
    );
    assert!(
        lines.contains("Kept on this printer: 1 change in macros.cfg. It stays a local change."),
        "{lines}"
    );
    assert_eq!(
        show(&url, "shared", "macros.cfg"),
        MACROS.replace("G1 E50 F300", "G1 E60 F300")
    );
    assert_eq!(printer2.read("macros.cfg"), fixed);
    assert_eq!(printer2.status_of("macros.cfg"), "local changes");

    // Printer 1 gets the fix and keeps its own temperature.
    printer1.ok(&["sync"]);
    assert_eq!(
        printer1.read("macros.cfg"),
        MACROS.replace("G1 E50 F300", "G1 E60 F300")
    );
}

#[test]
fn keep_always_marks_the_line_as_override() {
    let (_dir, url) = repository();
    let (_printer1, printer2, printer2_macros) = two_printers(&url);

    let fixed = printer2_macros.replace("G1 E50 F300", "G1 E60 F300");
    printer2.write("macros.cfg", &fixed);
    let answers = Answers::new(How::Pick, &[Pick::KeepAlways, Pick::Send]);
    let lines = copy(&printer2, &answers).join("\n");
    let marked = fixed.replace(
        "variable_temp: 250\n",
        "variable_temp: 250   # printerkeep: override\n",
    );
    assert_eq!(printer2.read("macros.cfg"), marked);
    assert_eq!(
        show(&url, "shared", "macros.cfg"),
        MACROS.replace("G1 E50 F300", "G1 E60 F300")
    );
    assert!(
        lines.contains(
            "Kept on this printer: variable_temp in [gcode_macro LOAD] (override). Shared keeps its own line."
        ),
        "{lines}"
    );

    // Later copies of the whole file never send the temperature.
    let improved = marked.replace("M109", "M104 S0\n    M109");
    printer2.write("macros.cfg", &improved);
    printer2.ok(&["copy-to-shared", "--yes", "macros.cfg"]);
    assert_eq!(
        show(&url, "shared", "macros.cfg"),
        MACROS
            .replace("G1 E50 F300", "G1 E60 F300")
            .replace("M109", "M104 S0\n    M109")
    );
}

#[test]
fn nothing_is_copied_when_every_change_is_kept() {
    let (_dir, url) = repository();
    let (_printer1, printer2, printer2_macros) = two_printers(&url);
    let before = show(&url, "shared", "macros.cfg");

    printer2.write(
        "macros.cfg",
        &printer2_macros.replace("G1 E50 F300", "G1 E60 F300"),
    );
    let answers = Answers::new(How::Pick, &[Pick::Keep, Pick::Keep]);
    let lines = copy(&printer2, &answers);
    assert_eq!(
        lines,
        ["Nothing copied: macros.cfg in shared stays as it was."]
    );
    assert_eq!(show(&url, "shared", "macros.cfg"), before);
}

#[test]
fn cancel_copies_nothing() {
    let (_dir, url) = repository();
    let (_printer1, printer2, printer2_macros) = two_printers(&url);
    let before = show(&url, "shared", "macros.cfg");

    printer2.write(
        "macros.cfg",
        &printer2_macros.replace("G1 E50 F300", "G1 E60 F300"),
    );
    let answers = Answers::new(How::Cancel, &[]);
    let lines = copy(&printer2, &answers);
    assert_eq!(
        lines,
        ["Nothing copied: macros.cfg in shared stays as it was."]
    );
    assert!(answers.asked.borrow().is_empty());
    assert_eq!(show(&url, "shared", "macros.cfg"), before);
}

#[test]
fn gcode_lines_can_be_kept_but_never_marked() {
    let (_dir, url) = repository();
    let (printer1, printer2, printer2_macros) = two_printers(&url);

    // Printer 2 adds a line in gcode, and changes its temperature with the gcode: line next to it.
    let edited = printer2_macros
        .replace(
            "variable_temp: 250\ngcode:\n",
            "variable_temp: 260\ngcode: # load\n",
        )
        .replace("    G1 E50 F300\n", "    M118 Loading\n    G1 E50 F300\n");
    printer2.write("macros.cfg", &edited);
    // A wrong answer from a script: keep always on gcode lines only keeps them, without marks.
    let answers = Answers::new(How::Pick, &[Pick::KeepAlways, Pick::KeepAlways]);
    let lines = copy(&printer2, &answers);
    assert_eq!(
        *answers.asked.borrow(),
        [
            "macros.cfg 1/2 Some(\"gcode_macro LOAD\") false -variable_temp: 230\n-gcode:\n+variable_temp: 260\n+gcode: # load\n",
            "macros.cfg 2/2 Some(\"gcode_macro LOAD\") false +    M118 Loading\n",
        ]
    );
    assert_eq!(
        lines,
        ["Nothing copied: macros.cfg in shared stays as it was."]
    );
    assert_eq!(printer2.read("macros.cfg"), edited);

    // The gcode line stays a local change: a change from shared on other lines still merges.
    let answers = Answers::new(How::Pick, &[Pick::Keep, Pick::Send]);
    copy(&printer2, &answers);
    assert_eq!(
        show(&url, "shared", "macros.cfg"),
        MACROS.replace("    G1 E50 F300\n", "    M118 Loading\n    G1 E50 F300\n")
    );
    printer1.write(
        "macros.cfg",
        &printer1
            .read("macros.cfg")
            .replace("    G1 E50 F300\n", "    G1 E50 F300\n    M400\n"),
    );
    printer1.ok(&["copy-to-shared", "--yes", "macros.cfg"]);
    printer2.ok(&["sync"]);
    assert_eq!(
        printer2.read("macros.cfg"),
        edited.replace("    G1 E50 F300\n", "    G1 E50 F300\n    M400\n")
    );
    assert_eq!(printer2.status_of("macros.cfg"), "local changes");
}
