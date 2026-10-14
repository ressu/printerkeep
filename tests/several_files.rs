//! Commands that take file names take several. printerkeep checks all files first: if one can't be used, it
//! changes nothing and names each problem.

mod common;

use common::*;

const A: &str = "[gcode_macro A]\ngcode:\n    M118 a\n";
const B: &str = "[gcode_macro B]\ngcode:\n    M118 b\n";

fn follows(printer: &Printer) -> String {
    printer.read("printerkeep.conf")
}

/// Several files go to shared in one copy: one question, one commit to shared, one backup.
#[test]
fn copy_several_files_to_shared_at_once() {
    let (_dir, url) = repository();
    let printer = Printer::connected("printer-1", &url);
    printer.write("a.cfg", A);
    printer.write("macros/b.cfg", B);

    let output = printer.ok(&["copy-to-shared", "--yes", "a.cfg", "macros/b.cfg"]);
    assert_eq!(
        output,
        "a.cfg isn't in shared yet. It goes to shared with 3 lines.\n\
         macros/b.cfg isn't in shared yet. It goes to shared with 3 lines.\n\
         Backup made: Added 2 files\n\
         Copied a.cfg to shared. Other printers can now follow it.\n\
         Copied macros/b.cfg to shared. Other printers can now follow it.\n\
         Sent 2 backups to the repository.\n"
    );
    assert!(
        output.contains("Copied macros/b.cfg to shared. Other printers can now follow it."),
        "{output}"
    );
    assert_eq!(show(&url, "shared", "a.cfg"), A);
    assert_eq!(show(&url, "shared", "macros/b.cfg"), B);
    assert_eq!(
        git(
            std::path::Path::new(&url),
            &["log", "-1", "--format=%s", "shared"]
        ),
        "Copied a.cfg, macros/b.cfg from printer-1"
    );
    assert!(
        follows(&printer).contains("follow:\n  a.cfg\n  macros/b.cfg\n"),
        "{}",
        follows(&printer)
    );
    assert_eq!(
        printer.last_backup("printer-1"),
        "Copied a.cfg, macros/b.cfg to shared"
    );

    // The preview shows each file, and printerkeep asks once.
    printer.write("a.cfg", &A.replace("M118 a", "M118 a2"));
    printer.write("macros/b.cfg", &B.replace("M118 b", "M118 b2"));
    let output = printer.ok(&["copy-to-shared", "--yes", "a.cfg", "macros/b.cfg"]);
    assert!(output.contains("+    M118 a2"), "{output}");
    assert!(output.contains("+    M118 b2"), "{output}");
}

/// A missing file and a symlink: printerkeep names both problems and copies nothing.
#[test]
fn one_file_that_cant_be_used_stops_the_copy() {
    let (_dir, url) = repository();
    let printer = Printer::connected("printer-1", &url);
    printer.write("a.cfg", A);
    printer.write("target.cfg", B);
    std::os::unix::fs::symlink(
        printer.config().join("target.cfg"),
        printer.config().join("mainsail.cfg"),
    )
    .unwrap();

    let error = printer.fails(&[
        "copy-to-shared",
        "--yes",
        "a.cfg",
        "missing.cfg",
        "mainsail.cfg",
    ]);
    assert!(
        error.contains("missing.cfg isn't in the config folder"),
        "{error}"
    );
    assert!(error.contains("mainsail.cfg is a symlink"), "{error}");
    assert!(
        error.contains("Nothing changed, because some files can't be used:"),
        "{error}"
    );
    assert!(!follows(&printer).contains("follow:"));
    let branches = git(std::path::Path::new(&url), &["branch", "--list", "shared"]);
    assert_eq!(branches, "");
}

#[test]
fn follow_and_stop_following_several_files() {
    let (_dir, url) = repository();
    let printer1 = Printer::connected("printer-1", &url);
    let printer2 = Printer::connected("printer-2", &url);
    printer1.write("a.cfg", A);
    printer1.write("macros/b.cfg", B);
    printer1.ok(&["copy-to-shared", "--yes", "a.cfg", "macros/b.cfg"]);

    // One file isn't in shared: nothing changes.
    let error = printer2.fails(&["follow", "a.cfg", "nothing.cfg"]);
    assert!(error.contains("nothing.cfg isn't in shared yet"), "{error}");
    assert!(!follows(&printer2).contains("follow:"));

    printer2.ok(&["follow", "a.cfg", "macros/b.cfg"]);
    assert_eq!(printer2.read("a.cfg"), A);
    assert_eq!(printer2.read("macros/b.cfg"), B);
    assert!(follows(&printer2).contains("follow:\n  a.cfg\n  macros/b.cfg\n"));

    printer2.ok(&["stop-following", "a.cfg", "macros/b.cfg"]);
    assert!(
        !follows(&printer2).contains("  a.cfg\n"),
        "{}",
        follows(&printer2)
    );
    assert!(!follows(&printer2).contains("  macros/b.cfg\n"));
    assert_eq!(printer2.read("a.cfg"), A);
}

#[test]
fn decide_several_files() {
    let (_dir, url) = repository();
    let printer1 = Printer::connected("printer-1", &url);
    let printer2 = Printer::connected("printer-2", &url);
    printer1.write("a.cfg", A);
    printer1.write("b.cfg", B);
    printer1.ok(&["copy-to-shared", "--yes", "a.cfg", "b.cfg"]);
    printer2.ok(&["follow", "a.cfg", "b.cfg"]);

    // Both printers change the same lines: both files need a decision on printer 2.
    printer2.write("a.cfg", &A.replace("M118 a", "M118 mine"));
    printer2.write("b.cfg", &B.replace("M118 b", "M118 mine"));
    printer1.write("a.cfg", &A.replace("M118 a", "M118 shared"));
    printer1.write("b.cfg", &B.replace("M118 b", "M118 shared"));
    printer1.ok(&["copy-to-shared", "--yes", "a.cfg", "b.cfg"]);
    printer2.ok(&["backup"]);
    assert!(printer2.read("a.cfg").contains("####"));
    assert!(printer2.read("b.cfg").contains("####"));

    let output = printer2.ok(&["decide", "--use-shared", "a.cfg", "b.cfg"]);
    assert!(output.contains("a.cfg: used the shared lines."), "{output}");
    assert!(output.contains("b.cfg: used the shared lines."), "{output}");
    assert_eq!(printer2.read("a.cfg"), A.replace("M118 a", "M118 shared"));
    assert_eq!(printer2.read("b.cfg"), B.replace("M118 b", "M118 shared"));
}

/// The first file is already in shared: the other files still go to shared.
#[test]
fn a_file_already_in_shared_doesnt_stop_the_others() {
    let (_dir, url) = repository();
    let printer = Printer::connected("printer-1", &url);
    printer.write("a.cfg", A);
    printer.ok(&["copy-to-shared", "--yes", "a.cfg"]);
    printer.write("b.cfg", B);

    let output = printer.ok(&["copy-to-shared", "--yes", "a.cfg", "b.cfg"]);
    assert!(
        output.contains("a.cfg in shared is already the same as on this printer."),
        "{output}"
    );
    assert!(!output.contains("isn't in shared. To add it"), "{output}");
    assert_eq!(show(&url, "shared", "b.cfg"), B);
    assert_eq!(
        git(
            std::path::Path::new(&url),
            &["log", "-1", "--format=%s", "shared"]
        ),
        "Copied b.cfg from printer-1"
    );
}

/// A followed file that isn't in shared yet (for example after a copy that didn't work): copying it doesn't
/// tell you to copy it.
#[test]
fn copying_a_followed_file_that_isnt_in_shared_has_no_notice() {
    let (_dir, url) = repository();
    let printer = Printer::connected("printer-1", &url);
    printer.write("a.cfg", A);
    printer.write(
        "printerkeep.conf",
        &format!("[printerkeep]\nprinter_name: printer-1\nrepository: {url}\nfollow:\n  a.cfg\n"),
    );

    let output = printer.ok(&["copy-to-shared", "--yes", "a.cfg"]);
    assert!(!output.contains("To add it, run:"), "{output}");
    assert!(output.contains("Copied a.cfg to shared."), "{output}");
}
