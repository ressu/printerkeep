//! M2: repository, sending and restore. A local bare repository stands in for GitHub.

mod common;

use common::*;

use predicates::str::contains;
use std::fs;
use std::path::Path;

#[test]
fn add_repository_sends_all_backups_and_stores_only_the_url() {
    let (_dir, url) = repository();
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["backup"]);

    let output = printer.ok(&["add-repository", &url]);
    assert!(output.contains(&format!("Connected to the repository {url}.")));
    assert!(output.contains("The repository has no branch printer-1 yet"));
    // add-repository writes the URL into printerkeep.conf, so a backup follows.
    assert!(output.contains("Backup made: Changed printerkeep.conf\n"));
    assert!(output.contains("Sent 2 backups to the repository.\n"));
    assert_eq!(remote_tip(&url, "printer-1"), printer.tip("printer-1"));
    assert_eq!(
        printer.read("printerkeep.conf"),
        format!("[printerkeep]\nprinter_name: printer-1\nrepository: {url}\n")
    );
}

#[test]
fn backups_are_sent_and_no_change_sends_nothing() {
    let (_dir, url) = repository();
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);

    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    assert_eq!(
        printer.ok(&["backup"]),
        "Backup made: Changed printer.cfg\nSent 1 backup to the repository.\n"
    );
    assert_eq!(remote_tip(&url, "printer-1"), printer.tip("printer-1"));

    // No change: no backup, nothing sent.
    assert_eq!(
        printer.ok(&["backup"]),
        "No files changed. No backup made.\n"
    );
    assert!(
        printer
            .ok(&["status"])
            .contains("All backups are in the repository.\n")
    );
}

#[test]
fn a_printer_never_takes_over_the_branch_of_another_printer() {
    let (_dir, url) = repository();
    let first = Printer::named("printer-1");
    first.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    first.ok(&["add-repository", &url]);
    let history = remote_tip(&url, "printer-1");

    // A second printer (or a new install) with the same name and its own backups.
    let second = Printer::named("printer-1");
    second.write("printer.cfg", "[printer]\nkinematics: cartesian\n");
    second.ok(&["backup"]);
    let error = second
        .command(&["add-repository", &url])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    let error = String::from_utf8(error).unwrap();
    assert!(
        error.contains(
            "The branch printer-1 in the repository has backups that have no history in common with this printer's"
        ),
        "{error}"
    );
    assert!(
        error.contains(&format!(
            "printerkeep add-repository {url} --printer-name printer-1 --continue"
        )),
        "{error}"
    );
    assert!(
        error.contains(&format!(
            "printerkeep add-repository {url} --printer-name <another name>"
        )),
        "{error}"
    );
    assert_eq!(remote_tip(&url, "printer-1"), history);

    // A different printer: rename it, then add the repository.
    let output = second.ok(&["rename", "printer-2"]);
    assert!(
        output.contains("Renamed the printer from printer-1 to printer-2."),
        "{output}"
    );
    second.ok(&["add-repository", &url]);
    assert_eq!(remote_tip(&url, "printer-1"), history);
    assert_eq!(
        git(Path::new(&url), &["show", "printer-2:printer.cfg"]),
        "[printer]\nkinematics: cartesian"
    );
}

#[test]
fn a_new_repository_with_a_readme_is_joined() {
    let (_dir, url) = repository();
    // As GitHub makes it with "Add a README file": one commit on main, no configuration.
    web_commit(&url, "main", "Initial commit", |work| {
        fs::write(work.join("README.md"), "# printers\n").unwrap();
    });
    let readme_commit = remote_tip(&url, "main");

    // No printer_name: the printer is "main".
    let printer = Printer::empty();
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["backup"]);
    let output = printer.ok(&["add-repository", &url]);
    assert!(
        output.contains("has no configuration yet, so printerkeep continues from it"),
        "{output}"
    );
    let tip = remote_tip(&url, "main");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &readme_commit, &tip],
    );
    assert_eq!(
        git(Path::new(&url), &["show", "main:printer.cfg"]),
        "[printer]"
    );
    assert!(printer.ok(&["status"]).contains("Printer name: main\n"));
}

#[test]
fn repository_with_newer_changes_is_never_overwritten() {
    let (_dir, url) = repository();
    let first = Printer::named("printer-1");
    first.write("printer.cfg", "[printer]\n");
    first.ok(&["add-repository", &url]);

    // Another install of printer-1 (restored from the repository) sends a newer backup.
    let other = Printer::empty();
    other.ok(&["restore", &url, "--printer-name", "printer-1"]);
    other.write("printer.cfg", "[printer]\nmax_velocity: 300\n");
    other.ok(&["backup"]);
    let newer = remote_tip(&url, "printer-1");

    // Both changed the same line: the file needs decision (M6). The newer backup stays in the history.
    first.write("printer.cfg", "[printer]\nmax_velocity: 200\n");
    let output = first.ok(&["backup"]);
    assert!(
        output.starts_with("Backup made: Changed printer.cfg\n"),
        "{output}"
    );
    assert!(
        output
            .contains("printer.cfg: needs a decision. Open the file and look for the #### lines."),
        "{output}"
    );
    assert!(
        output.contains("Sent 2 backups to the repository."),
        "{output}"
    );
    let tip = remote_tip(&url, "printer-1");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &newer, &tip],
    );
    let file = first.read("printer.cfg");
    assert!(
        file.contains(
            "#### The repository changed this line too.\n#### Active now (this printer):\n\
             max_velocity: 200\n#### Repository version. To use it, remove the\n"
        ),
        "{file}"
    );
    assert!(file.contains("\n#max_velocity: 300\n"), "{file}");
    assert!(
        first
            .ok(&["status"])
            .contains("All backups are in the repository.")
    );
}

#[test]
fn changes_in_the_repository_get_to_the_printer() {
    let (_dir, url) = repository();
    let printer = Printer::named("printer-1");
    printer.write(
        "printer.cfg",
        "[printer]\nmax_velocity: 300\nmax_accel: 3000\n",
    );
    printer.write("macros.cfg", "[gcode_macro A]\ngcode:\n    G28\n");
    printer.ok(&["add-repository", &url]);

    // An edit in the web pages of the git host: a different line, a new file, a removed file.
    web_edit(&url, "printer-1", |work| {
        fs::write(
            work.join("printer.cfg"),
            "[printer]\nmax_velocity: 300\nmax_accel: 5000\n",
        )
        .unwrap();
        fs::write(
            work.join("new.cfg"),
            "[gcode_macro NEW]\ngcode:\n    M118 new\n",
        )
        .unwrap();
        fs::remove_file(work.join("macros.cfg")).unwrap();
    });
    let web_tip = remote_tip(&url, "printer-1");

    // The printer changes a different line. The next backup merges both.
    printer.write(
        "printer.cfg",
        "[printer]\nmax_velocity: 250\nmax_accel: 3000\n",
    );
    let output = printer.ok(&["backup"]);
    assert!(
        output.contains(
            "printer.cfg: wrote the changes from the repository. The changes on this printer stay."
        ),
        "{output}"
    );
    assert!(
        output.contains("new.cfg: wrote the changes from the repository."),
        "{output}"
    );
    assert!(
        output.contains("macros.cfg: removed in the repository, and kept on this printer"),
        "{output}"
    );
    assert_eq!(
        printer.read("printer.cfg"),
        "[printer]\nmax_velocity: 250\nmax_accel: 5000\n"
    );
    assert_eq!(
        printer.read("new.cfg"),
        "[gcode_macro NEW]\ngcode:\n    M118 new\n"
    );
    assert!(printer.config().join("macros.cfg").exists());
    let tip = remote_tip(&url, "printer-1");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &web_tip, &tip],
    );
    assert_eq!(
        git(Path::new(&url), &["log", "-1", "--format=%s", "printer-1"]),
        "Changes from the repository: new.cfg, printer.cfg"
    );
}

#[test]
fn unreachable_repository_keeps_backups_for_later() {
    let (dir, url) = repository();
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);
    let log = || fs::read_to_string(printer.dir.path().join("logs/printerkeep.log")).unwrap();

    let moved = dir.path().join("moved.git");
    fs::rename(&url, &moved).unwrap();
    printer.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    let output = printer.ok(&["backup"]);
    assert!(output.contains("Can't send the backups. "), "{output}");
    assert!(
        output.contains("Check the printer's network connection")
            && output.contains("printerkeep tries again at the next run."),
        "{output}"
    );

    // The same problem at the next runs: the terminal shows it, the log does not get it again.
    for _ in 0..2 {
        let output = printer.ok(&["backup"]);
        assert!(output.contains("Can't send the backups. "), "{output}");
    }
    assert_eq!(
        log().matches("Can't send the backups. ").count(),
        1,
        "{}",
        log()
    );
    assert!(
        printer
            .ok(&["status"])
            .contains("Backups not in the repository: 1. The last try to send them didn't work:")
    );

    fs::rename(&moved, &url).unwrap();
    let output = printer.ok(&["backup"]);
    assert!(
        output.contains("Sent 1 backup to the repository."),
        "{output}"
    );
    assert!(
        output.contains("The problem of the last runs is solved."),
        "{output}"
    );
    assert!(log().contains("The problem of the last runs is solved."));
    assert!(!printer.ok(&["status"]).contains("didn't work"));
}

#[test]
fn restore_into_an_empty_config_folder() {
    let (_dir, url) = repository();
    let original = Printer::named("printer-1");
    original.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    original.write("macros/macros.cfg", "[gcode_macro START]\n");
    original.ok(&["add-repository", &url]);

    let new = Printer::empty();
    let output = new.ok(&["restore", &url]);
    assert!(output.contains("Restored 3 files from the backup of printer-1."));
    assert!(output.contains("sudo systemctl restart moonraker klipper"));
    assert_eq!(new.read("printer.cfg"), "[printer]\nkinematics: corexy\n");
    assert_eq!(new.read("macros/macros.cfg"), "[gcode_macro START]\n");

    // The restored printer continues the same history.
    assert_eq!(new.ok(&["backup"]), "No files changed. No backup made.\n");
    new.write("printer.cfg", "[printer]\nkinematics: cartesian\n");
    assert!(
        new.ok(&["backup"])
            .contains("Sent 1 backup to the repository.")
    );
    git(
        Path::new(&url),
        &[
            "merge-base",
            "--is-ancestor",
            &original.tip("printer-1"),
            &remote_tip(&url, "printer-1"),
        ],
    );
}

#[test]
fn restore_replaces_different_files_only_with_overwrite() {
    let (_dir, url) = repository();
    let original = Printer::named("printer-1");
    original.write("printer.cfg", "[printer]\nkinematics: corexy\n");
    original.ok(&["add-repository", &url]);

    // A new install has its own default printer.cfg and an extra file.
    let new = Printer::empty();
    new.write("printer.cfg", "# default\n");
    new.write("extra.cfg", "# keep me\n");

    new.command(&["restore", &url])
        .assert()
        .failure()
        .stderr(contains(
            "These files in the config folder differ from the backup: printer.cfg.",
        ))
        .stderr(contains("add --overwrite"));
    assert_eq!(new.read("printer.cfg"), "# default\n");

    new.ok(&["restore", &url, "--overwrite"]);
    assert_eq!(new.read("printer.cfg"), "[printer]\nkinematics: corexy\n");
    // Restore never removes files.
    assert_eq!(new.read("extra.cfg"), "# keep me\n");
}

#[test]
fn restore_asks_which_printer_when_there_are_more() {
    let (_dir, url) = repository();
    for name in ["printer-1", "printer-2"] {
        let printer = Printer::named(name);
        printer.write("printer.cfg", &format!("# {name}\n"));
        printer.ok(&["add-repository", &url]);
    }

    let new = Printer::empty();
    new.command(&["restore", &url])
        .assert()
        .failure()
        .stderr(contains(
            "backups of several printers: printer-1, printer-2. To select one",
        ));
    new.command(&["restore", &url, "--printer-name", "printer-3"])
        .assert()
        .failure()
        .stderr(contains("no printer named 'printer-3'"));
    new.ok(&["restore", &url, "--printer-name", "printer-2"]);
    assert_eq!(new.read("printer.cfg"), "# printer-2\n");
    assert!(
        new.read("printerkeep.conf")
            .contains("printer_name: printer-2\n")
    );
}

#[test]
fn token_in_the_url_is_refused() {
    let printer = Printer::named("printer-1");
    printer
        .command(&[
            "add-repository",
            "https://me:github_pat_secret@github.com/me/printers.git",
        ])
        .assert()
        .failure()
        .stderr(contains("Give the URL without the token"));
    assert_eq!(
        printer.read("printerkeep.conf"),
        "[printerkeep]\nprinter_name: printer-1\n"
    );
}

#[test]
fn token_is_not_stored_without_a_connection() {
    let printer = Printer::named("printer-1");
    // Nothing listens on port 9.
    printer
        .command(&["add-repository", "https://127.0.0.1:9/me/printers.git"])
        .write_stdin("github_pat_test\n")
        .assert()
        .failure()
        .stderr(contains("Can't reach the repository"));

    let data = printer.dir.path().join("printerkeep");
    assert!(!data.join("credentials").exists());
    assert!(!data.join("credentials.new").exists());
    assert_eq!(
        printer.read("printerkeep.conf"),
        "[printerkeep]\nprinter_name: printer-1\n"
    );
}

#[test]
fn ssh_url_makes_a_key() {
    let printer = Printer::named("printer-1");
    printer
        .command(&["add-repository", "ssh://git@127.0.0.1:9/me/printers.git"])
        .assert()
        .failure();
    let key = printer.dir.path().join("printerkeep/id_ed25519.pub");
    assert!(fs::read_to_string(key).unwrap().starts_with("ssh-ed25519 "));
}

#[test]
fn readme_is_in_the_repository_but_not_in_the_config_folder() {
    let (_dir, url) = repository();
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);

    let readme = git(Path::new(&url), &["show", "printer-1:README.md"]);
    assert!(readme.starts_with("<!-- printerkeep:"), "{readme}");
    assert!(
        readme.contains(&format!(
            "printerkeep restore {url} --printer-name printer-1"
        )),
        "{readme}"
    );
    assert!(!printer.config().join("README.md").exists());
    let status = printer.ok(&["status"]);
    assert!(status.contains("Files in the last backup: 2\n"), "{status}");
    assert!(
        status.contains("Changes since the last backup: none"),
        "{status}"
    );

    // A restore does not write it into the config folder.
    let new_install = Printer::empty();
    new_install.ok(&["restore", &url]);
    assert_eq!(new_install.read("printer.cfg"), "[printer]\n");
    assert!(!new_install.config().join("README.md").exists());
    assert_eq!(
        new_install.ok(&["backup"]),
        "No files changed. No backup made.\n"
    );

    // The user's own README.md wins.
    printer.write("README.md", "# My printer notes\n");
    printer.ok(&["backup"]);
    assert_eq!(
        git(Path::new(&url), &["show", "printer-1:README.md"]),
        "# My printer notes"
    );
}

#[test]
fn rename_moves_the_history_to_a_new_branch() {
    let (_dir, url) = repository();
    let printer = Printer::empty();
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);
    printer.write("printer.cfg", "[printer]\nmax_velocity: 300\n");
    printer.ok(&["backup"]);
    let old_tip = remote_tip(&url, "main");

    let output = printer.ok(&["rename", "printer-1"]);
    assert!(
        output.contains("Renamed the printer from main to printer-1."),
        "{output}"
    );
    assert!(
        output.contains("The repository keeps the branch main."),
        "{output}"
    );
    // The whole history is on the new branch; the README there has the new name.
    let new_tip = remote_tip(&url, "printer-1");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &old_tip, &new_tip],
    );
    assert_eq!(remote_tip(&url, "main"), old_tip);
    assert!(git(Path::new(&url), &["show", "printer-1:README.md"]).contains("# printer-1"));
    let status = printer.ok(&["status"]);
    assert!(status.contains("Printer name: printer-1\n"), "{status}");
    assert!(
        status.contains("All backups are in the repository."),
        "{status}"
    );

    // A name that the repository has already is refused.
    let other = Printer::empty();
    other.write("printer.cfg", "[printer]\n");
    other.write(
        "printerkeep.conf",
        &format!("[printerkeep]\nprinter_name: printer-3\nrepository: {url}\n"),
    );
    other.ok(&["backup"]);
    let error = other
        .command(&["rename", "printer-1"])
        .assert()
        .failure()
        .get_output()
        .stderr
        .clone();
    assert!(
        String::from_utf8(error)
            .unwrap()
            .contains("already has a branch named 'printer-1'")
    );
}

#[test]
fn a_klipper_backup_branch_is_continued() {
    let (_dir, url) = repository();
    // A repository as klipper-backup makes it: its README, and the config folder in printer_data/config.
    web_commit(
        &url,
        "main",
        "New backup from 01/10/26 - 10:00:00",
        |work| {
            fs::create_dir_all(work.join("printer_data/config")).unwrap();
            fs::write(
            work.join("README.md"),
            "# klipper-backup 💾 \nKlipper backup script for manual or automated GitHub backups \n",
        )
        .unwrap();
            fs::write(work.join(".gitignore"), "*.swp\n").unwrap();
            fs::write(
                work.join("printer_data/config/printer.cfg"),
                "[printer]\nmax_velocity: 200\n",
            )
            .unwrap();
        },
    );
    web_commit(
        &url,
        "main",
        "printer.cfg modified - 02/10/26 - 10:00:00",
        |work| {
            fs::write(
                work.join("printer_data/config/printer.cfg"),
                "[printer]\nmax_velocity: 250\n",
            )
            .unwrap();
        },
    );
    let klipper_backup = remote_tip(&url, "main");

    // The printer has the default name main, and its own first backup.
    let printer = Printer::empty();
    printer.write("printer.cfg", "[printer]\nmax_velocity: 250\n");
    printer.ok(&["backup"]);
    let output = printer.ok(&["add-repository", &url, "--continue"]);
    assert!(
        output.contains("The branch main has backups from klipper-backup."),
        "{output}"
    );
    assert!(
        output.contains("Important: stop klipper-backup on this printer"),
        "{output}"
    );

    // The klipper-backup history stays; the branch now has the files of the config folder at the top.
    let tip = remote_tip(&url, "main");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &klipper_backup, &tip],
    );
    let files = git(Path::new(&url), &["ls-tree", "--name-only", "main"]);
    assert_eq!(
        files.lines().collect::<Vec<_>>(),
        ["README.md", "printer.cfg", "printerkeep.conf"]
    );
    assert_eq!(
        git(Path::new(&url), &["show", "main:printer.cfg"]),
        "[printer]\nmax_velocity: 250"
    );
    assert!(git(Path::new(&url), &["show", "main:README.md"]).starts_with("<!-- printerkeep:"));
    let history = "Continued the klipper-backup history\n\
                   printer.cfg modified - 02/10/26 - 10:00:00\n\
                   New backup from 01/10/26 - 10:00:00";
    assert_eq!(
        git(Path::new(&url), &["log", "--format=%s", "main"]),
        history
    );
    // git follows printer.cfg into the klipper-backup history.
    assert_eq!(
        git(
            Path::new(&url),
            &[
                "log",
                "--format=%s",
                "--follow",
                "main",
                "--",
                "printer.cfg"
            ]
        ),
        history
    );
}

#[test]
fn rename_refuses_a_klipper_backup_branch_and_the_documented_way_continues_it() {
    let (_dir, url) = repository();
    web_commit(
        &url,
        "printer-1",
        "New backup from 01/10/26 - 10:00:00",
        |work| {
            fs::create_dir_all(work.join("printer_data/config")).unwrap();
            fs::write(work.join("README.md"), "# klipper-backup 💾 \n").unwrap();
            fs::write(work.join("printer_data/config/printer.cfg"), "[printer]\n").unwrap();
        },
    );
    let klipper_backup = remote_tip(&url, "printer-1");

    // The printer has an old name and is connected; its branch in the repository is not the klipper-backup one.
    let printer = Printer::named("printer-1.example.net");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);

    let error = printer.fails(&["rename", "printer-1"]);
    assert!(
        error.contains("rename never takes over a branch in the repository"),
        "{error}"
    );
    assert!(
        error.contains("--printer-name printer-1 --continue"),
        "{error}"
    );

    // The documented way: add-repository with the name of that branch, and --continue (no terminal).
    let output = printer.ok(&[
        "add-repository",
        &url,
        "--printer-name",
        "printer-1",
        "--continue",
    ]);
    assert!(
        output.contains("This printer's name is now printer-1 (it was printer-1.example.net)."),
        "{output}"
    );
    assert!(
        output.contains("The branch printer-1 has backups from klipper-backup."),
        "{output}"
    );
    let tip = remote_tip(&url, "printer-1");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &klipper_backup, &tip],
    );
    // The old branch in the repository stays as it was.
    assert!(remote_tip(&url, "printer-1.example.net").len() == 40);
}

#[test]
fn the_backup_run_continues_when_the_local_data_was_removed() {
    let (_dir, url) = repository();
    let printer = Printer::named("printer-1");
    printer.write("printer.cfg", "[printer]\n");
    printer.ok(&["add-repository", &url]);
    let old_tip = remote_tip(&url, "printer-1");

    // The real case: printerkeep's data was removed; printerkeep.conf (with the repository) stays.
    fs::remove_dir_all(printer.dir.path().join("printerkeep")).unwrap();
    printer.write("printer.cfg", "[printer]\nmax_velocity: 300\n");
    let output = printer.ok(&["backup"]);
    assert!(
        output
            .contains("printerkeep.conf names this branch, so printerkeep continues its history."),
        "{output}"
    );
    assert!(output.contains("Sent "), "{output}");
    assert!(!output.contains("Cannot send"), "{output}");
    let tip = remote_tip(&url, "printer-1");
    git(
        Path::new(&url),
        &["merge-base", "--is-ancestor", &old_tip, &tip],
    );
    assert_eq!(
        show(&url, "printer-1", "printer.cfg"),
        "[printer]\nmax_velocity: 300\n"
    );
    assert_eq!(
        printer.ok(&["backup"]),
        "No files changed. No backup made.\n"
    );
}
