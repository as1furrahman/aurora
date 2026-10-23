//! CLI tests for `aurora workflow validate|show|plan` and `aurora providers` (read-only).
//! Ported from `tests/test_workflow_cli.py`. Provider detection is controlled through a
//! temporary PATH containing empty executable files; nothing is ever executed.

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn example() -> String {
    repo_root().join("workflows/examples/plan-implement-review.toml").display().to_string()
}

fn fixture(name: &str) -> String {
    repo_root().join("tests/fixtures/workflows").join(name).display().to_string()
}

/// `aurora` with a clean environment: no registry override and an empty PATH (no providers).
fn aurora(empty_path: &Path) -> Command {
    let mut cmd = Command::cargo_bin("aurora").expect("binary builds");
    cmd.env_remove("AURORA_REGISTRY_PATH").env_remove("AWB_REGISTRY_PATH").env("PATH", empty_path).env("NO_COLOR", "1");
    cmd
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(cmd: &mut Command) -> Run {
    let out = cmd.output().expect("runs");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("invalid JSON ({e}):\n{text}"))
}

fn fake_bin(dir: &Path, names: &[&str]) {
    for name in names {
        let path = dir.join(name);
        fs::write(&path, "").expect("write fake binary");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
    }
}

const REGISTRY_TOML: &str = r#"
[registry]
version = "1.0.0"
updated_at = "2026-10-23T00:00:00Z"

[[skills]]
id = "alpha"
name = "Alpha"
version = "1.0.0"
description = "a"
compatible_agents = ["universal"]

[[skills]]
id = "beta"
name = "Beta"
version = "1.0.0"
description = "b"
compatible_agents = ["codex-cli", "claude-code"]
"#;

struct Env {
    _tmp: tempfile::TempDir,
    dir: PathBuf,
    path: PathBuf,
    registry: String,
}

fn env() -> Env {
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = tmp.path().to_path_buf();
    let path = dir.join("bin");
    fs::create_dir(&path).expect("bin dir");
    let registry = dir.join("skills.toml");
    fs::write(&registry, REGISTRY_TOML).expect("registry");
    Env { registry: registry.display().to_string(), _tmp: tmp, dir, path }
}

fn minimal(e: &Env) -> String {
    let text = fs::read_to_string(repo_root().join("tests/fixtures/parity/minimal.toml")).expect("minimal fixture");
    let path = e.dir.join("wf.toml");
    fs::write(&path, text).expect("write workflow");
    path.display().to_string()
}

// --- validate ---------------------------------------------------------------

#[test]
fn validate_example() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "validate", &example()]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("VALID") && r.stdout.contains("plan-implement-review"));
}

#[test]
fn validate_invalid_reports_every_error() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "validate", &fixture("provider-in-role.toml")]));
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("INVALID") && r.stdout.contains("role != provider"), "{}", r.stdout);
}

#[test]
fn validate_json() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "validate", &fixture("cycle.toml"), "--json"]));
    assert_eq!(r.code, 1);
    let data = json(&r.stdout);
    assert_eq!(data["valid"], false);
    assert!(data["errors"].as_array().expect("errors").iter().any(|m| m.as_str().unwrap_or("").contains("cycle")));
}

#[test]
fn validate_missing_file_is_an_error() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "validate", "/nonexistent/wf.toml"]));
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("cannot read file") || r.stderr.contains("cannot read file"), "{}{}", r.stdout, r.stderr);
}

// --- show -------------------------------------------------------------------

#[test]
fn show_json_keeps_roles_provider_neutral() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "show", &example(), "--json"]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    let data = json(&r.stdout);
    assert_eq!(data["workflow"]["id"], "plan-implement-review");
    let planner: Vec<&String> = data["roles"]["planner"].as_object().expect("role").keys().collect();
    assert_eq!(planner, ["responsibility", "required_skills", "recommended_skills"]);
    assert_eq!(data["routing"]["roles"]["implementer"]["preferred"], "antigravity");
    assert_eq!(data["stages"][0]["id"], "plan");
    assert_eq!(data["stages"][1]["id"], "implement");
}

#[test]
fn show_text() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "show", &example()]));
    assert_eq!(r.code, 0);
    assert!(r.stdout.contains("review") && r.stdout.contains("independent_of"));
}

// --- plan -------------------------------------------------------------------

#[test]
fn plan_multi_and_single() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "plan", &example(), "--no-detect", "--json"]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    let plan = json(&r.stdout);
    assert_eq!(plan["mode"], "multi");
    assert!(plan["detection"].is_null());

    let r = run(aurora(&e.path).args(["workflow", "plan", &example(), "--no-detect", "--json", "--provider", "codex"]));
    assert_eq!(r.code, 0);
    let plan = json(&r.stdout);
    assert!(plan["stages"].as_array().expect("stages").iter().all(|s| s["provider"] == "codex"));
}

#[test]
fn plan_route() {
    let e = env();
    let r = run(aurora(&e.path).args([
        "workflow",
        "plan",
        &example(),
        "--no-detect",
        "--json",
        "--route",
        "implement=codex",
        "--route",
        "plan=codex",
    ]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    let plan = json(&r.stdout);
    assert_eq!(plan["routes"], serde_json::json!({"implement": "codex", "plan": "codex"}));
    let keys: Vec<&String> = plan["routes"].as_object().expect("routes").keys().collect();
    assert_eq!(keys, ["implement", "plan"], "routes keep the order given");
}

#[test]
fn plan_rejects_malformed_route_and_unknown_provider() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "plan", &example(), "--route", "implement"]));
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("STAGE=PROVIDER"), "{}", r.stderr);
    let r = run(aurora(&e.path).args(["workflow", "plan", &example(), "--provider", "gpt-cli"]));
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("gpt-cli"), "{}", r.stderr);
}

#[test]
fn plan_provider_and_route_are_mutually_exclusive() {
    let e = env();
    let r =
        run(aurora(&e.path).args(["workflow", "plan", &example(), "--provider", "codex", "--route", "plan=claude"]));
    assert_eq!(r.code, 2, "clap usage error");
}

#[test]
fn plan_blocked_exit_code() {
    let e = env();
    let wf = minimal(&e);
    let r = run(aurora(&e.path).args([
        "workflow",
        "plan",
        &wf,
        "--registry",
        &e.registry,
        "--no-detect",
        "--provider",
        "antigravity",
    ]));
    assert_eq!(r.code, 3, "{}", r.stderr);
    assert!(r.stdout.contains("BLOCKED"));
}

#[test]
fn plan_uses_path_detection_by_default() {
    let e = env();
    fake_bin(&e.path, &["claude", "codex"]);
    let r = run(aurora(&e.path).args(["workflow", "plan", &example(), "--json"]));
    assert_eq!(r.code, 0, "{}", r.stderr);
    let plan = json(&r.stdout);
    assert_eq!(
        plan["detection"],
        serde_json::json!({"antigravity": false, "claude": true, "codex": true, "hermes": false})
    );
    let implement = plan["stages"].as_array().expect("stages").iter().find(|s| s["id"] == "implement").expect("stage");
    assert_eq!(implement["fallback_trail"][0]["reason"], "unavailable");
}

#[test]
fn plan_writes_nothing() {
    let e = env();
    let wf = minimal(&e);
    let before = listing(&e.dir);
    run(aurora(&e.path).current_dir(&e.dir).args(["workflow", "plan", &wf, "--registry", &e.registry, "--no-detect"]));
    assert_eq!(listing(&e.dir), before);
}

fn listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> =
        fs::read_dir(dir).expect("dir").map(|e| e.expect("entry").file_name().to_string_lossy().into_owned()).collect();
    names.sort();
    names
}

// --- registry resolution ----------------------------------------------------

#[test]
fn registry_env_var_is_honoured() {
    let e = env();
    let wf = minimal(&e);
    let r = run(aurora(&e.path).env("AURORA_REGISTRY_PATH", &e.registry).args(["workflow", "validate", &wf]));
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    // The built-in registry has no 'alpha'/'beta' skills, so the same file fails without it.
    let r = run(aurora(&e.path).args(["workflow", "validate", &wf]));
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("required skill 'alpha'"), "{}", r.stdout);
}

#[test]
fn builtin_registry_works_outside_the_repo() {
    let e = env();
    let r = run(aurora(&e.path).current_dir(&e.dir).args(["workflow", "validate", &example()]));
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
}

#[test]
fn bad_registry_path_is_an_error() {
    let e = env();
    let r = run(aurora(&e.path).args(["workflow", "validate", &example(), "--registry", "/nonexistent/skills.toml"]));
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("Error:"), "{}", r.stderr);
}

// --- providers --------------------------------------------------------------

#[test]
fn providers_json() {
    let e = env();
    let r = run(aurora(&e.path).args(["providers", "--json"]));
    assert_eq!(r.code, 0);
    let data = json(&r.stdout);
    let rows = data.as_array().expect("rows");
    let ids: Vec<&str> = rows.iter().map(|p| p["id"].as_str().expect("id")).collect();
    assert_eq!(ids, ["antigravity", "claude", "codex", "hermes"]);
    for p in rows {
        assert_eq!(p["installed"], false);
        assert_eq!(p["headless_enabled"], false);
        assert_eq!(p["invocation_modes"], serde_json::json!(["manual-handoff"]));
    }
}

#[test]
fn providers_detects_from_path_without_running_anything() {
    let e = env();
    fake_bin(&e.path, &["agy"]);
    let r = run(aurora(&e.path).args(["providers", "--json"]));
    let data = json(&r.stdout);
    let agy = &data[0];
    assert_eq!((agy["id"].as_str(), agy["installed"].as_bool()), (Some("antigravity"), Some(true)));
    assert!(agy["path"].as_str().expect("path").ends_with("/bin/agy"));
    let r = run(aurora(&e.path).args(["providers"]));
    assert!(r.stdout.contains("[INSTALLED]") && r.stdout.contains("headless: disabled"));
}
