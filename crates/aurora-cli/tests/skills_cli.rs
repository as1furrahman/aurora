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

fn git(dir: &Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A local mirror laid out as <mirror>/<owner>/<name>, holding one skill with a script.
fn mirror(root: &Path) -> (std::path::PathBuf, String) {
    let repo = root.join("mirror/demo-org/demo-skills");
    fs::create_dir_all(repo.join("skills/demo")).expect("mkdir");
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "uploadpack.allowFilter", "true"]);
    git(&repo, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    fs::write(repo.join("skills/demo/SKILL.md"), "---\nname: demo\ndescription: d\n---\nok\n").expect("write");
    fs::write(repo.join("skills/demo/run.sh"), "echo hi\n").expect("write");
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let rev = git(&repo, &["rev-parse", "HEAD"]);
    (root.join("mirror"), rev)
}

fn aurora_fetch(args: &[&str]) -> (i32, String, String) {
    let out = Command::cargo_bin("aurora")
        .expect("binary builds")
        .env("NO_COLOR", "1")
        .env_remove("AURORA_CACHE_DIR")
        .args(["skills", "fetch"])
        .args(args)
        .output()
        .expect("runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn fetch_ad_hoc_from_a_mirror_scans_and_reports_the_hash() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (mirror, rev) = mirror(tmp.path());
    let cache = tmp.path().join("cache");
    let (m, c) = (mirror.display().to_string(), cache.display().to_string());
    let args = ["--repo", "https://github.com/demo-org/demo-skills", "--rev", &rev, "--path", "skills/demo"];
    let (code, out, err) = aurora_fetch(&[&args[..], &["--mirror", &m, "--cache", &c, "--json"]].concat());
    assert_eq!(code, 4, "a bundled script needs review: {out}{err}");
    let v: Value = serde_json::from_str(&out).expect("json");
    assert_eq!(v["readiness"]["state"], "needs-review");
    assert_eq!(v["sha256"].as_str().expect("hash").len(), 64);
    assert!(v["dir"].as_str().expect("dir").starts_with(&c), "fetched into the given cache");

    let (code, out, _) = aurora_fetch(&[&args[..], &["--mirror", &m, "--cache", &c]].concat());
    assert_eq!(code, 4);
    assert!(out.contains("run.sh") && out.contains("needs review"), "{out}");
}

#[test]
fn fetch_by_entry_uses_the_catalog_pin() {
    let tmp = tempfile::tempdir().expect("tmp");
    let (mirror, _) = mirror(tmp.path());
    let (m, c) = (mirror.display().to_string(), tmp.path().join("cache").display().to_string());
    // The mirror has no obra/superpowers, so the pinned commit cannot be fetched: a clear error.
    let (code, _, err) = aurora_fetch(&["verification-before-completion", "--mirror", &m, "--cache", &c]);
    assert_eq!(code, 1);
    assert!(err.contains("2ce8aa15d05bd57213e6f746f953aafb7ded7a8b") || err.contains("fetch"), "{err}");

    let (code, _, err) = aurora_fetch(&["headroom", "--cache", &c]);
    assert_eq!(code, 1);
    assert!(err.contains("not installed by Aurora"), "{err}");
    let (code, _, err) = aurora_fetch(&["nope", "--cache", &c]);
    assert_eq!(code, 1);
    assert!(err.contains("nope"), "{err}");
    let (code, _, _) = aurora_fetch(&["--repo", "https://github.com/o/r", "--cache", &c]);
    assert_eq!(code, 2, "--repo needs --rev and --path");
}
