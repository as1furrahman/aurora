use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn version_reports_workspace_version() {
    Command::cargo_bin("aurora")
        .expect("binary builds")
        .arg("--version")
        .assert()
        .success()
        .stdout(contains(concat!("aurora ", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn help_describes_the_tool() {
    Command::cargo_bin("aurora")
        .expect("binary builds")
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Usage: aurora"));
}
