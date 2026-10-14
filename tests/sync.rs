//! `printerkeep sync` is the full run: a backup, changes from shared and from the repository, and send.
//! `printerkeep backup` only makes a backup and sends it: it never writes a file in the config folder.

mod common;

use common::*;
use std::fs;

const A: &str = "[gcode_macro A]\ngcode:\n    M118 a\n";

#[test]
fn backup_never_writes_files_and_sync_gets_the_changes() {
    let (_dir, url) = repository();
    let printer1 = Printer::connected("printer-1", &url);
    let printer2 = Printer::connected("printer-2", &url);
    printer1.write("a.cfg", A);
    printer1.ok(&["copy-to-shared", "--yes", "a.cfg"]);
    printer2.ok(&["follow", "a.cfg"]);
    printer1.write("a.cfg", &A.replace("M118 a", "M118 new"));
    printer1.ok(&["copy-to-shared", "--yes", "a.cfg"]);

    printer2.write("printer.cfg", "[printer]\n# changed\n");
    let output = printer2.ok(&["backup"]);
    assert_eq!(output, "Backup made: Changed printer.cfg\n");
    assert_eq!(printer2.read("a.cfg"), A);
    assert_eq!(remote_tip(&url, "printer-2"), printer2.tip("printer-2"));

    let output = printer2.ok(&["sync"]);
    assert!(
        output.contains("a.cfg: wrote the changes from shared."),
        "{output}"
    );
    assert_eq!(printer2.read("a.cfg"), A.replace("M118 a", "M118 new"));
}

/// The repository has changes on this printer's branch: backup keeps its backup on the printer and says
/// how to get them; sync merges them and sends.
#[test]
fn backup_tells_you_to_sync_when_the_repository_has_changes() {
    let (_dir, url) = repository();
    let printer = Printer::connected("printer-1", &url);
    web_edit(&url, "printer-1", |work| {
        fs::write(work.join("web.cfg"), "# from the web\n").unwrap()
    });

    printer.write("printer.cfg", "[printer]\n# changed\n");
    let output = printer.ok(&["backup"]);
    assert!(
        output.starts_with("Backup made: Changed printer.cfg\n"),
        "{output}"
    );
    assert!(
        output.contains(
            "The repository has changes on this printer's branch, so printerkeep didn't send the backup. \
             To get the changes and send it, run:\n  printerkeep sync"
        ),
        "{output}"
    );
    assert!(!printer.config().join("web.cfg").exists());

    printer.ok(&["sync"]);
    assert_eq!(printer.read("web.cfg"), "# from the web\n");
    assert_eq!(remote_tip(&url, "printer-1"), printer.tip("printer-1"));
}

/// Timers installed before sync run `printerkeep backup`. Started by systemd, it still does the full run.
#[test]
fn an_old_timer_that_runs_backup_still_gets_the_changes() {
    let (_dir, url) = repository();
    let printer1 = Printer::connected("printer-1", &url);
    let printer2 = Printer::connected("printer-2", &url);
    printer1.write("a.cfg", A);
    printer1.ok(&["copy-to-shared", "--yes", "a.cfg"]);
    printer2.write(
        "printerkeep.conf",
        &format!("[printerkeep]\nprinter_name: printer-2\nrepository: {url}\nfollow:\n  a.cfg\n"),
    );

    printer2
        .command(&["backup"])
        .env("INVOCATION_ID", "0123456789abcdef")
        .assert()
        .success();
    assert_eq!(printer2.read("a.cfg"), A);
}
