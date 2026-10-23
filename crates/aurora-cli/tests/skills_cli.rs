//! CLI tests for `aurora skills inspect <dir>` (R2 Step 1): read-only SKILL.md checks.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

fn inspect(dir: &Path, json: bool) -> (i32, String, String) {
    let mut cmd = Command::cargo_bin("aurora").expect("binary builds");
    cmd.env("NO_COLOR", "1").args(["skills", "inspect"]).arg(dir);
    if json {
        cmd.arg("--json");
    }
    let out = cmd.output().expect("runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn write_skill(root: &Path, name: &str, text: &str) -> std::path::PathBuf {
    let dir = root.join(name);
    fs::create_dir(&dir).expect("mkdir");
    fs::write(dir.join("SKILL.md"), text).expect("write");
    dir
}

const CLAUDE_HEAVY: &str = "---\nname: heavy\ndescription: Plans on disk. Use for long tasks.\n\
hooks:\n  Stop:\n    - hooks: [{type: command, command: echo}]\n---\nState: !`git status --short`\n";

#[test]
fn inspect_reports_support_per_cli_and_listing_cost() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = write_skill(tmp.path(), "heavy", CLAUDE_HEAVY);
    let (code, out, err) = inspect(&dir, false);
    assert_eq!(code, 0, "{err}");
    for needle in ["heavy", "claude", "native", "codex", "degraded", "hooks", "git status", "listing"] {
        assert!(out.contains(needle), "missing {needle}:\n{out}");
    }

    let (code, out, _) = inspect(&dir, true);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["name"], "heavy");
    assert_eq!(v["support"]["claude"]["support"], "native");
    assert_eq!(v["support"]["codex"]["support"], "degraded");
    assert_eq!(v["injections"][0]["line"], 1);
    assert!(v["listing_chars"]["codex"].as_u64().expect("number") > 0);
}

#[test]
fn spec_errors_exit_one_but_still_report() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = write_skill(tmp.path(), "wrong-dir", "---\nname: right\ndescription: d\n---\n");
    let (code, out, _) = inspect(&dir, false);
    assert_eq!(code, 1);
    assert!(out.contains("does not match its directory"), "{out}");
}

#[test]
fn unreadable_or_unsafe_skills_fail_clearly() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (code, _, err) = inspect(&tmp.path().join("missing"), false);
    assert_eq!(code, 1);
    assert!(err.contains("SKILL.md"), "{err}");

    let dir = tmp.path().join("linked");
    fs::create_dir(&dir).expect("mkdir");
    fs::write(tmp.path().join("elsewhere.md"), CLAUDE_HEAVY).expect("write");
    std::os::unix::fs::symlink(tmp.path().join("elsewhere.md"), dir.join("SKILL.md")).expect("symlink");
    let (code, _, err) = inspect(&dir, false);
    assert_eq!(code, 1, "a symlinked SKILL.md is refused");
    // Refused by the path guard (it resolves outside the skill dir) or by the no-follow open.
    assert!(err.to_lowercase().contains("symlink") || err.contains("outside allowed boundaries"), "{err}");

    let bad = write_skill(tmp.path(), "bad", "no frontmatter here\n");
    let (code, _, err) = inspect(&bad, false);
    assert_eq!(code, 1);
    assert!(err.contains("frontmatter"), "{err}");
}

fn scan(dir: &Path, json: bool) -> (i32, String, String) {
    let mut cmd = Command::cargo_bin("aurora").expect("binary builds");
    cmd.env("NO_COLOR", "1").args(["skills", "scan"]).arg(dir);
    if json {
        cmd.arg("--json");
    }
    let out = cmd.output().expect("runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn scan_exit_codes_follow_the_verdict() {
    let tmp = tempfile::tempdir().expect("tmp");
    let clean = write_skill(tmp.path(), "clean", "---\nname: clean\ndescription: d\n---\nok\n");
    let (code, out, _) = scan(&clean, false);
    assert_eq!(code, 0, "{out}");
    assert!(out.contains("clean") && out.contains("sha256"), "{out}");

    let review = write_skill(tmp.path(), "review", "---\nname: review\ndescription: d\n---\nok\n");
    fs::write(review.join("helper.sh"), "echo hi\n").expect("write");
    let (code, out, _) = scan(&review, false);
    assert_eq!(code, 4, "{out}");
    assert!(out.contains("needs review") && out.contains("helper.sh"), "{out}");

    let blocked = write_skill(tmp.path(), "blocked", "---\nname: blocked\ndescription: d\n---\nok\n");
    fs::write(blocked.join("helper.pyc"), b"\x6f\x0d\x0d\x0a").expect("write");
    let (code, out, _) = scan(&blocked, true);
    assert_eq!(code, 3);
    let v: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["verdict"], "blocked");
    assert_eq!(v["findings"][0]["rule"], "bytecode");
    assert_eq!(v["sha256"].as_str().expect("hash").len(), 64);
}

#[test]
fn scan_refuses_a_symlinked_root() {
    let tmp = tempfile::tempdir().expect("tmp");
    let dir = write_skill(tmp.path(), "real", "---\nname: real\ndescription: d\n---\n");
    std::os::unix::fs::symlink(&dir, tmp.path().join("link")).expect("symlink");
    let (code, _, err) = scan(&tmp.path().join("link"), false);
    assert_eq!(code, 1);
    assert!(err.contains("symlink"), "{err}");
}
