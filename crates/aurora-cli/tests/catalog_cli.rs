//! CLI tests for `aurora catalog list|show|recommend` (R2 Step 0). Read-only: nothing is fetched
//! or installed.

use std::path::PathBuf;

use assert_cmd::Command;
use serde_json::Value;

fn aurora() -> Command {
    let mut cmd = Command::cargo_bin("aurora").expect("binary builds");
    cmd.env("NO_COLOR", "1");
    cmd
}

fn run(args: &[&str]) -> (i32, String, String) {
    let out = aurora().args(args).output().expect("runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn json(args: &[&str]) -> Value {
    let (code, out, err) = run(args);
    assert_eq!(code, 0, "{args:?} failed: {err}");
    serde_json::from_str(&out).unwrap_or_else(|e| panic!("invalid JSON ({e}):\n{out}"))
}

#[test]
fn list_shows_every_entry_with_kind_tier_and_trust() {
    let (code, out, _) = run(&["catalog", "list"]);
    assert_eq!(code, 0);
    for needle in ["karpathy-guidelines", "rules", "baseline", "code-review", "headroom", "infra", "community"] {
        assert!(out.contains(needle), "missing {needle}:\n{out}");
    }
    let rows = json(&["catalog", "list", "--json"]);
    let rows = rows.as_array().expect("array");
    assert!(rows.len() >= 30);
    let ponytail = rows.iter().find(|r| r["id"] == "ponytail").expect("ponytail");
    assert_eq!(ponytail["trust"], "community");
    assert_eq!(ponytail["rev"].as_str().expect("rev").len(), 40);
}

#[test]
fn show_prints_provenance_and_flags() {
    let (code, out, _) = run(&["catalog", "show", "planning-with-files"]);
    assert_eq!(code, 0);
    for needle in [
        "https://github.com/OthmanAdi/planning-with-files/tree/99402ce1e6a9",
        "skills/planning-with-files",
        "hooks",
        "MIT",
        "opt-in",
    ] {
        assert!(out.contains(needle), "missing {needle}:\n{out}");
    }
    let (code, _, err) = run(&["catalog", "show", "nope"]);
    assert_eq!(code, 1);
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn recommend_a_preset_for_the_default_pair() {
    let rec = json(&["catalog", "recommend", "--preset", "bugfix", "--json"]);
    assert_eq!(rec["profile"], "pair");
    assert_eq!(rec["clis"], serde_json::json!(["claude", "codex"]));
    let review = rec["stages"].as_array().expect("stages").iter().find(|s| s["stage"] == "review").expect("review");
    assert_eq!(review["provider"], "codex");

    let (code, out, _) = run(&["catalog", "recommend", "--preset", "bugfix"]);
    assert_eq!(code, 0);
    for needle in ["review", "codex", "code-review", "karpathy-guidelines", "AGENTS.md", "CLAUDE.md", "budget"] {
        assert!(out.contains(needle), "missing {needle}:\n{out}");
    }
}

#[test]
fn profile_follows_the_number_of_clis() {
    let rec = json(&["catalog", "recommend", "--preset", "feature", "--clis", "codex", "--json"]);
    assert_eq!(rec["profile"], "solo");
    let rec = json(&["catalog", "recommend", "--preset", "feature", "--clis", "claude,codex,antigravity", "--json"]);
    assert_eq!(rec["profile"], "trio");
}

#[test]
fn recommend_a_workflow_file() {
    let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../workflows/presets/docs.toml");
    let rec = json(&["catalog", "recommend", file.to_str().expect("utf-8"), "--json"]);
    assert_eq!(rec["workflow"], "docs");
}

#[test]
fn recommend_errors_are_clear() {
    let (code, _, err) = run(&["catalog", "recommend", "--preset", "waterfall"]);
    assert_eq!(code, 1);
    assert!(err.contains("waterfall"), "{err}");
    let (code, _, err) = run(&["catalog", "recommend", "--preset", "bugfix", "--with", "headroom"]);
    assert_eq!(code, 1);
    assert!(err.contains("not installed by Aurora"), "{err}");
    let (code, _, _) = run(&["catalog", "recommend"]);
    assert_eq!(code, 2, "a workflow file or --preset is required");
}

#[test]
fn warnings_reach_the_user() {
    let (code, out, _) = run(&["catalog", "recommend", "--preset", "bugfix", "--clis", "claude"]);
    assert_eq!(code, 0, "warnings are not errors");
    assert!(out.contains("reduced independence"), "{out}");
}
