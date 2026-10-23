//! CLI tests for `aurora skills plan` (R2 Step 3b). Offline: a custom catalog points at a local
//! mirror; the project directory must be left exactly as it was.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

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

fn aurora(args: &[&str]) -> (i32, String, String) {
    let out = Command::cargo_bin("aurora").expect("binary").env("NO_COLOR", "1").args(args).output().expect("runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn hash_of(dir: &Path) -> String {
    let (_, out, _) = aurora(&["skills", "scan", dir.to_str().expect("utf-8"), "--json"]);
    let v: Value = serde_json::from_str(&out).expect("scan json");
    v["sha256"].as_str().expect("hash").to_string()
}

struct Fixture {
    _tmp: tempfile::TempDir,
    catalog: String,
    mirror: String,
    cache: String,
    project: PathBuf,
}

/// Mirror repo demo-org/team-skills with a baseline skill, a reviewer skill that pre-approves
/// Bash, and a rules entry; plus a catalog pinning them.
fn fixture() -> Fixture {
    let tmp = tempfile::tempdir().expect("tmp");
    let repo = tmp.path().join("mirror/demo-org/team-skills");
    for (path, text) in [
        ("skills/demo/SKILL.md", "---\nname: demo\ndescription: Evidence before claims.\n---\nVerify first.\n"),
        (
            "skills/checker/SKILL.md",
            "---\nname: checker\ndescription: Review diffs.\nallowed-tools: Read Bash\n---\nReview.\n",
        ),
        ("skills/house-rules/SKILL.md", "---\nname: house-rules\ndescription: Team rules.\n---\nKeep changes small.\n"),
    ] {
        let p = repo.join(path);
        fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        fs::write(p, text).expect("write");
    }
    git(&repo, &["init", "-q", "-b", "main"]);
    git(&repo, &["config", "uploadpack.allowFilter", "true"]);
    git(&repo, &["config", "uploadpack.allowAnySHA1InWant", "true"]);
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let rev = git(&repo, &["rev-parse", "HEAD"]);
    let h = |name: &str| hash_of(&repo.join("skills").join(name));
    let catalog = format!(
        r#"schema_version = 1
verified_on = "2026-10-24"

[targets.claude]
listing_chars = 8000
per_skill_chars = 1536
rules_file = "CLAUDE.md"
evidence = "test"

[targets.codex]
listing_chars = 8000
rules_file = "AGENTS.md"
evidence = "test"

[targets.antigravity]
listing_chars = 8000
rules_file = "AGENTS.md"
evidence = "test"

[targets.hermes]
listing_chars = 12000
rules_file = "AGENTS.md"
evidence = "test"

[profiles.pair]
description = "two"
default_clis = ["claude", "codex"]
roles = {{ analyst = 1, architect = 1, implementer = 1, tester = 2, reviewer = 2, security = 1, documenter = 1 }}

[sources.team]
repo = "https://github.com/demo-org/team-skills"
rev = "{rev}"
license = "MIT"
trust = "curated"

[[entries]]
id = "house-rules"
kind = "rules"
source = "team"
path = "skills/house-rules"
tier = "baseline"
description_chars = 11
sha256 = "{rules}"
why = "x"

[[entries]]
id = "demo"
kind = "skill"
source = "team"
path = "skills/demo"
tier = "baseline"
description_chars = 22
sha256 = "{demo}"
why = "x"

[[entries]]
id = "checker"
kind = "skill"
source = "team"
path = "skills/checker"
tier = "stage"
roles = ["reviewer"]
description_chars = 13
sha256 = "{checker}"
why = "x"
"#,
        rules = h("house-rules"),
        demo = h("demo"),
        checker = h("checker"),
    );
    let catalog_path = tmp.path().join("catalog.toml");
    fs::write(&catalog_path, catalog).expect("write catalog");
    let project = tmp.path().join("project");
    fs::create_dir(&project).expect("mkdir project");
    fs::write(project.join("AGENTS.md"), "# Project\n").expect("write");
    Fixture {
        catalog: catalog_path.display().to_string(),
        mirror: tmp.path().join("mirror").display().to_string(),
        cache: tmp.path().join("cache").display().to_string(),
        project,
        _tmp: tmp,
    }
}

fn plan(f: &Fixture, extra: &[&str]) -> (i32, String, String) {
    let project = f.project.display().to_string();
    let mut args = vec![
        "skills",
        "plan",
        "--preset",
        "bugfix",
        "--catalog",
        &f.catalog,
        "--mirror",
        &f.mirror,
        "--cache",
        &f.cache,
        "--project",
        &project,
    ];
    args.extend_from_slice(extra);
    aurora(&args)
}

fn snapshot(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in walk(dir) {
        let rel = entry.strip_prefix(dir).expect("rel").display().to_string();
        out.push((rel, fs::read(&entry).unwrap_or_default()));
    }
    out.sort();
    out
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for e in fs::read_dir(dir).expect("read_dir").flatten() {
        let p = e.path();
        if p.is_dir() { found.extend(walk(&p)) } else { found.push(p) }
    }
    found
}

#[test]
fn plan_lists_actions_overlays_and_rules_and_writes_nothing() {
    let f = fixture();
    let before = snapshot(&f.project);
    let (code, out, err) = plan(&f, &["--json"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).expect("json");
    let actions = v["actions"].as_array().expect("actions");
    let kind = |target: &str| {
        actions.iter().find(|a| a["target"] == target).map(|a| a["kind"].as_str().unwrap_or("").to_string())
    };
    assert_eq!(kind(".claude/skills/demo").as_deref(), Some("create"));
    assert_eq!(kind(".agents/skills/demo").as_deref(), Some("create"));
    assert_eq!(kind(".agents/skills/checker").as_deref(), Some("create"));
    assert_eq!(kind("CLAUDE.md").as_deref(), Some("create"));
    assert_eq!(kind("AGENTS.md").as_deref(), Some("update"));
    assert_eq!(v["lock"]["skills"][".agents/skills/checker"]["overlays"], serde_json::json!(["allowed-tools"]));
    assert_eq!(snapshot(&f.project), before, "plan must not write to the project");

    let (_, text, _) = plan(&f, &[]);
    for needle in ["create", ".agents/skills/checker", "allowed-tools removed", "AGENTS.md", "nothing was written"] {
        assert!(text.contains(needle), "missing {needle}:\n{text}");
    }
}

#[test]
fn foreign_skills_in_the_project_are_refused() {
    let f = fixture();
    let foreign = f.project.join(".claude/skills/demo");
    fs::create_dir_all(&foreign).expect("mkdir");
    fs::write(foreign.join("SKILL.md"), "---\nname: demo\ndescription: mine\n---\n").expect("write");
    let (code, out, _) = plan(&f, &["--json"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).expect("json");
    let a = v["actions"]
        .as_array()
        .expect("actions")
        .iter()
        .find(|a| a["target"] == ".claude/skills/demo")
        .expect("action");
    assert_eq!(a["kind"], "refuse");
    assert!(a["detail"].as_str().expect("detail").contains("not installed by Aurora"));
}

#[test]
fn a_symlinked_rules_file_is_not_written_through() {
    let f = fixture();
    fs::remove_file(f.project.join("AGENTS.md")).expect("rm");
    fs::write(f.project.join("CLAUDE.md"), "# Shared\n").expect("write");
    std::os::unix::fs::symlink("CLAUDE.md", f.project.join("AGENTS.md")).expect("symlink");
    let (code, out, _) = plan(&f, &["--json"]);
    assert_eq!(code, 0);
    let v: Value = serde_json::from_str(&out).expect("json");
    assert!(v["actions"].as_array().expect("actions").iter().all(|a| a["target"] != "AGENTS.md"));
    let warnings = v["warnings"].to_string();
    assert!(warnings.contains("AGENTS.md") && warnings.contains("symlink"), "{warnings}");
}

#[test]
fn a_corrupt_lock_is_an_error() {
    let f = fixture();
    fs::write(f.project.join("aurora.lock"), "{not json").expect("write");
    let (code, _, err) = plan(&f, &[]);
    assert_eq!(code, 1);
    assert!(err.contains("aurora.lock"), "{err}");
}
