//! R2 Step 5: CLI profiles on `aurora workflow plan|start`, catalog skills in handoffs, and a
//! fix hint when a CLI cannot load a stage's required skills.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use serde_json::Value;

fn aurora(args: &[&str]) -> (i32, String, String) {
    let out = Command::cargo_bin("aurora")
        .expect("binary")
        .env("NO_COLOR", "1")
        .env_remove("AURORA_REGISTRY_PATH")
        .env_remove("AWB_REGISTRY_PATH")
        .args(args)
        .output()
        .expect("runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn preset(dir: &Path, name: &str) -> String {
    let file = dir.join(format!("{name}.toml"));
    let (code, _, err) = aurora(&["workflow", "init", name, "--output", file.to_str().expect("utf-8")]);
    assert_eq!(code, 0, "{err}");
    file.display().to_string()
}

#[test]
fn a_profile_routes_stages_and_attaches_skills() {
    let tmp = tempfile::tempdir().expect("tmp");
    let wf = preset(tmp.path(), "bugfix");
    let (code, out, err) = aurora(&["workflow", "plan", &wf, "--profile", "pair", "--no-detect", "--json"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(&out).expect("json");
    let stage =
        |id: &str| v["stages"].as_array().expect("stages").iter().find(|s| s["id"] == id).expect("stage").clone();
    assert_eq!(stage("fix")["provider"], "claude");
    assert_eq!(stage("review")["provider"], "codex");
    let skills = stage("review")["stage_skills"].as_array().expect("skills").clone();
    assert!(skills.iter().any(|s| s == "code-review"), "{skills:?}");
}

#[test]
fn clis_alone_pick_the_profile_and_any_cli_can_take_any_role() {
    let tmp = tempfile::tempdir().expect("tmp");
    let wf = preset(tmp.path(), "feature");
    let (code, out, err) = aurora(&[
        "workflow",
        "plan",
        &wf,
        "--clis",
        "hermes",
        "--registry",
        &team_registry(tmp.path()),
        "--no-detect",
        "--json",
    ]);
    assert_eq!(code, 0, "{out}{err}");
    let v: Value = serde_json::from_str(&out).expect("json");
    assert!(v["stages"].as_array().expect("stages").iter().all(|s| s["provider"] == "hermes"));
}

/// The shipped registry with Hermes declared as a compatible agent (what a team would do).
fn team_registry(dir: &Path) -> String {
    let shipped =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../registry/skills.toml")).expect("read");
    let team = shipped.replace("compatible_agents = [", "compatible_agents = [\"hermes\", ");
    let path = dir.join("team-registry.toml");
    fs::write(&path, team).expect("write");
    path.display().to_string()
}

#[test]
fn profile_and_explicit_routing_are_exclusive() {
    let tmp = tempfile::tempdir().expect("tmp");
    let wf = preset(tmp.path(), "bugfix");
    let (code, _, _) = aurora(&["workflow", "plan", &wf, "--profile", "pair", "--route", "fix=codex"]);
    assert_eq!(code, 2);
    let (code, _, _) = aurora(&["workflow", "plan", &wf, "--clis", "claude", "--provider", "codex"]);
    assert_eq!(code, 2);
}

#[test]
fn incompatible_stages_get_a_fix_hint() {
    let tmp = tempfile::tempdir().expect("tmp");
    let wf = preset(tmp.path(), "bugfix");
    let (code, out, _) = aurora(&["workflow", "plan", &wf, "--provider", "hermes", "--no-detect"]);
    assert_eq!(code, 3, "blocked: the shipped registry does not declare hermes");
    assert!(out.contains("fix:") && out.contains("--registry") && out.contains("--route"), "{out}");
}

#[test]
fn start_with_a_profile_puts_skills_in_handoffs() {
    let tmp = tempfile::tempdir().expect("tmp");
    let wf = preset(tmp.path(), "bugfix");
    let runs = tmp.path().join("runs");
    fs::create_dir(&runs).expect("mkdir");
    let (code, out, err) = aurora(&[
        "workflow",
        "start",
        &wf,
        "--profile",
        "pair",
        "--no-detect",
        "--run-root",
        runs.to_str().expect("utf-8"),
        "--run-id",
        "r1",
    ]);
    assert_eq!(code, 0, "{out}{err}");
    let handoff = fs::read_to_string(runs.join("r1/handoff/reproduce.md")).expect("handoff");
    assert!(handoff.contains("- Use for this stage: `$verification-before-completion`"), "{handoff}");
    assert!(handoff.contains("`$reproduce-bug`"), "{handoff}");
}
