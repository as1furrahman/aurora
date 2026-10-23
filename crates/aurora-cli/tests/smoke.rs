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

#[test]
fn help_to_a_closed_pipe_does_not_panic() {
    // `aurora | head -1` closes the pipe early; printing help must not panic (exit 101).
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_aurora"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn");
    drop(child.stdout.take());
    let out = child.wait_with_output().expect("wait");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_ne!(out.status.code(), Some(101));
}
