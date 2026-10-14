//! M6: tests with real configuration files from four printers (`tests/fixtures/four-printers`, shortened to
//! the lines that show each case). The cases are in "Real differences, checked" in docs/solution-options.md.

mod common;

use common::*;

use std::fs;
use std::path::Path;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/four-printers");

/// Files that have CRLF line ends on the real printer (the fixtures are stored with LF).
const CRLF: &[(u8, &str)] = &[
    (3, "macros/temperature.cfg"),
    (4, "macros/temperature.cfg"),
    (3, "macros/ui-helpers.cfg"),
    (4, "macros/ui-helpers.cfg"),
    (3, "macros/macros.cfg"),
    (4, "macros/macros.cfg"),
    (1, "macros/reset.cfg"),
    (2, "macros/reset.cfg"),
    (3, "macros/reset.cfg"),
];

/// Printer `n` with its real files, connected to the repository.
fn printer(n: u8, url: &str) -> Printer {
    let printer = Printer::named(&format!("printer-{n}"));
    let source = Path::new(FIXTURES).join(format!("printer-{n}"));
    copy_folder(&source, &source, &printer.config(), n);
    printer.ok(&["connect", url]);
    printer
}

fn copy_folder(root: &Path, from: &Path, to: &Path, printer: u8) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_folder(root, &entry.path(), &target, printer);
            continue;
        }
        let relative = entry
            .path()
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        let content = fs::read_to_string(entry.path()).unwrap();
        let content = if CRLF.contains(&(printer, relative.as_str())) {
            content.replace('\n', "\r\n")
        } else {
            content
        };
        fs::write(target, content).unwrap();
    }
}

fn fixture(printer: u8, path: &str) -> String {
    fs::read_to_string(Path::new(FIXTURES).join(format!("printer-{printer}/{path}"))).unwrap()
}

/// A change to a file on the shared branch, as in the web pages of the git host.
fn edit_shared(url: &str, path: &str, change: impl Fn(String) -> String) {
    web_edit(url, "shared", |work| {
        let file = work.join(path);
        fs::write(&file, change(fs::read_to_string(&file).unwrap())).unwrap();
    });
}

/// Only line ends are different (printers 2, 3 and 4), and the same path has an unrelated file (printer 1).
#[test]
fn line_ends_and_unrelated_files_with_the_same_path() {
    let (_dir, url) = repository();
    let printers: Vec<Printer> = (1..=4).map(|n| printer(n, &url)).collect();
    let [p1, p2, p3, p4] = &printers[..] else {
        unreachable!()
    };
    let path = "macros/temperature.cfg";
    p2.ok(&["copy-to-shared", "--yes", path]);
    for printer in [p3, p4] {
        printer.ok(&["follow", path]);
        assert_eq!(printer.status_of(path), "up to date");
        assert!(printer.read(path).contains("\r\n"));
    }

    // A change from shared: printers 3 and 4 get it, and keep CRLF.
    let changed = fixture(2, path).replace("MAXIMUM={s+1}", "MAXIMUM={s+2}");
    p2.write(path, &changed);
    p2.ok(&["copy-to-shared", "--yes", path]);
    for printer in [p3, p4] {
        printer.ok(&["backup"]);
        assert_eq!(printer.read(path), changed.replace('\n', "\r\n"));
    }

    // Printer 1 has a different file at the same path. It does not follow it: nothing changes.
    p1.ok(&["backup"]);
    assert_eq!(p1.read(path), fixture(1, path));
    assert!(!p1.ok(&["status"]).contains("Followed files"));
}

/// The same file on three printers.
#[test]
fn identical_files_are_up_to_date() {
    let (_dir, url) = repository();
    let printers: Vec<Printer> = (1..=3).map(|n| printer(n, &url)).collect();
    let path = "macros/reset.cfg";
    printers[0].ok(&["copy-to-shared", "--yes", path]);
    for printer in &printers[1..] {
        printer.ok(&["follow", path]);
        assert_eq!(printer.status_of(path), "up to date");
    }
}

/// KAMP settings: each printer has its own values, many on lines next to each other.
#[test]
fn kamp_values_stay_on_each_printer() {
    let (_dir, url) = repository();
    let printers: Vec<Printer> = (1..=4).map(|n| printer(n, &url)).collect();
    let [p1, p2, p3, p4] = &printers[..] else {
        unreachable!()
    };
    let path = "KAMP_Settings.cfg";
    p2.ok(&["copy-to-shared", "--yes", path]);
    for printer in [p1, p3, p4] {
        printer.ok(&["follow", path]);
        assert_eq!(printer.status_of(path), "local changes");
    }

    // A shared change on a line that no printer changed. Printer 4's tip_distance is on the line next to it.
    let shared = fixture(2, path).replace("purge_height: 0.8", "purge_height: 0.6");
    p2.write(path, &shared);
    p2.ok(&["copy-to-shared", "--yes", path]);
    for (n, printer) in [(1, p1), (3, p3), (4, p4)] {
        printer.ok(&["backup"]);
        assert_eq!(
            printer.read(path),
            fixture(n, path).replace("purge_height: 0.8", "purge_height: 0.6"),
            "printer {n}"
        );
    }

    // A shared change on a value that the printers changed too. Printer 1 marked its value as an override
    // first: it keeps it. Printers 3 and 4 must decide.
    let p1_marked = p1.read(path).replace(
        "variable_purge_margin: 20",
        "variable_purge_margin: 20   # printerkeep: override",
    );
    p1.write(path, &p1_marked);
    p1.ok(&["backup"]);
    p2.write(
        path,
        &shared.replace("purge_margin: 15", "purge_margin: 12"),
    );
    p2.ok(&["copy-to-shared", "--yes", path]);
    p1.ok(&["backup"]);
    assert_eq!(p1.read(path), p1_marked);
    for printer in [p3, p4] {
        let output = printer.ok(&["backup"]);
        assert!(
            output.contains("KAMP_Settings.cfg: needs a decision."),
            "{output}"
        );
        let file = printer.read(path);
        assert!(
            file.contains("variable_purge_margin: 10\n#### Shared version."),
            "{file}"
        );
        assert!(file.contains("\n#variable_purge_margin: 12\n"), "{file}");
    }
}

/// Lines commented out for hardware that a printer does not have (a runout sensor), in gcode.
#[test]
fn commented_out_lines_for_missing_hardware_stay() {
    let (_dir, url) = repository();
    let printers: Vec<Printer> = (2..=4).map(|n| printer(n, &url)).collect();
    let [p2, p3, p4] = &printers[..] else {
        unreachable!()
    };
    let path = "macros/ui-helpers.cfg";
    p2.ok(&["copy-to-shared", "--yes", path]);
    p3.ok(&["follow", path]);
    p4.ok(&["follow", path]);

    // An improvement in RESUME, in shared. The printers without a runout sensor keep their lines.
    let improved = |text: &str| {
        text.replace(
            "NAME=PAUSE MOVE=1 MOVE_SPEED=60",
            "NAME=PAUSE MOVE=1 MOVE_SPEED=100",
        )
    };
    p2.write(path, &improved(&fixture(2, path)));
    p2.ok(&["copy-to-shared", "--yes", path]);
    for (n, printer) in [(3, p3), (4, p4)] {
        printer.ok(&["backup"]);
        let expected = improved(&fixture(n, path)).replace('\n', "\r\n");
        assert_eq!(printer.read(path), expected, "printer {n}");
        assert!(expected.contains("# _RUNOUT_OFF") && expected.contains("# _RUNOUT_ON"));
    }
}

/// An improvement that never spread: printer 4's retract in PRINT_END. It goes to shared by an edit in the
/// web pages (only the improvement, not printer 4's other changes), and reaches the other printers. Printer 1
/// has the file at a different path: it moves the file once, then follows it.
#[test]
fn an_improvement_reaches_the_other_printers() {
    let (_dir, url) = repository();
    let printers: Vec<Printer> = (1..=4).map(|n| printer(n, &url)).collect();
    let [p1, p2, p3, p4] = &printers[..] else {
        unreachable!()
    };
    let path = "macros/macros.cfg";
    p2.ok(&["copy-to-shared", "--yes", path]);
    p3.ok(&["follow", path]);
    p4.ok(&["follow", path]);

    let retract = |text: String| {
        text.replace(
            "gcode:\n    _RUNOUT_OFF\n",
            "gcode:\n    {% set retract_dist_default = 20 %}\n    \
             {% if printer['gcode_macro _KAMP_Settings'] is defined %}\n      \
             {% set retract_dist_default = printer[\"gcode_macro _KAMP_Settings\"].tip_distance %}\n    \
             {% endif %}\n    \
             {% set retract_dist = params.RETRACT|default(retract_dist_default)|int %}\n    _RUNOUT_OFF\n",
        )
        .replace(
            "    G1 E-10.0 F3600                 ; retract filament\n",
            "    G1 E-{retract_dist} F3600      ; retract filament\n    \
             {% if printer['gcode_macro _KAMP_Settings'] is defined %}\n      \
             SET_GCODE_VARIABLE MACRO=_KAMP_Settings VARIABLE=tip_distance VALUE={retract_dist}\n    \
             {% endif %}\n",
        )
    };
    edit_shared(&url, path, retract);

    // Printer 2 had no local changes; printer 3 keeps its own PRINT_START and LOAD_FILAMENT.
    p2.ok(&["backup"]);
    assert_eq!(p2.read(path), retract(fixture(2, path)));
    p3.ok(&["backup"]);
    assert_eq!(
        p3.read(path),
        retract(fixture(3, path)).replace('\n', "\r\n")
    );
    // Printer 4 has the improvement already, with its commented-out lines: nothing to write.
    let output = p4.ok(&["backup"]);
    assert!(!output.contains(path), "{output}");
    assert_eq!(p4.read(path), fixture(4, path).replace('\n', "\r\n"));
    assert_eq!(p4.status_of(path), "local changes");

    // Printer 1: the same file at the top of the config folder. Move it once, then follow it.
    fs::create_dir_all(p1.config().join("macros")).unwrap();
    fs::rename(p1.config().join("macros.cfg"), p1.config().join(path)).unwrap();
    p1.ok(&["follow", path]);
    assert_eq!(p1.read(path), fixture(1, "macros.cfg"));
    assert_eq!(p1.status_of(path), "local changes");
}
