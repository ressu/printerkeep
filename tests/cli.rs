use assert_cmd::Command;

#[test]
fn version_flag_prints_name_and_version() {
    Command::cargo_bin("printerkeep")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(format!("printerkeep {}\n", env!("CARGO_PKG_VERSION")));
}
