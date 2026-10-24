//! CLI tests for `aurora workflow start|status|submit|approve|resume`, including exit-code and
//! JSON checks.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use serde_json::Value;

fn example() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../workflows/examples/plan-implement-review.toml")
        .display()
        .to_string()
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

fn aurora(path_dir: &Path, args: &[&str]) -> Run {
    let out = Command::cargo_bin("aurora")
        .expect("binary")
        .env_remove("AURORA_REGISTRY_PATH")
        .env_remove("AWB_REGISTRY_PATH")
        .env("PATH", path_dir)
        .env("NO_COLOR", "1")
        .args(args)
        .output()
        .expect("runs");
    Run {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn json(text: &str) -> Value {
    serde_json::from_str(text).unwrap_or_else(|e| panic!("invalid JSON ({e}):\n{text}"))
}

fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().expect("tempdir");
    let base = t.path().canonicalize().expect("canonical");
    let bin = base.join("bin");
    fs::create_dir(&bin).expect("bin");
    let root = base.join("runs");
    fs::create_dir(&root).expect("runs");
    (t, bin, root)
}

#[test]
fn end_to_end_manual_handoff() {
    let (_t, bin, root) = setup();
    let root_s = root.display().to_string();
    let r = aurora(
        &bin,
        &[
            "workflow",
            "start",
            &example(),
            "--run-root",
            &root_s,
            "--run-id",
            "e2e",
            "--route",
            "implement=claude",
            "--json",
        ],
    );
    assert_eq!(r.code, 0, "{}", r.stderr);
    let run_dir = root.join("e2e");
    let started = json(&r.stdout);
    assert_eq!(started["run_dir"], run_dir.display().to_string());
    assert_eq!(started["issued"], serde_json::json!(["plan"]));
    let run = run_dir.display().to_string();

    let r = aurora(&bin, &["workflow", "submit", "--run", &run, "--stage", "plan", "--met", "plan-written"]);
    assert_eq!(r.code, 1);
    assert!(r.stdout.contains("output missing"), "{}", r.stdout);

    fs::write(run_dir.join("artifacts/plan/plan.md"), "the plan\n").expect("produce");
    let r = aurora(&bin, &["workflow", "submit", "--run", &run, "--stage", "plan", "--met", "plan-written"]);
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("PASSED"));

    let r = aurora(&bin, &["workflow", "status", "--run", &run, "--json"]);
    assert_eq!(r.code, 0);
    let status = json(&r.stdout);
    assert_eq!(status["complete"], false);
    let implement = status["stages"].as_array().expect("stages").iter().find(|s| s["id"] == "implement").expect("impl");
    assert_eq!((implement["state"].as_str(), implement["provider"].as_str()), (Some("RUNNING"), Some("claude")));

    let r = aurora(&bin, &["workflow", "status", "--run", &run]);
    assert!(r.stdout.contains("implement") && r.stdout.contains("RUNNING") && r.stdout.contains("INCOMPLETE"));

    let r = aurora(&bin, &["workflow", "resume", "--run", &run]);
    assert_eq!(r.code, 0);

    let r = aurora(&bin, &["workflow", "approve", "--run", &run, "--stage", "plan", "--by", "me"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("not awaiting approval"), "{}", r.stderr);
}

#[test]
fn submit_json_and_not_applicable() {
    let (_t, bin, root) = setup();
    let root_s = root.display().to_string();
    aurora(&bin, &["workflow", "start", &example(), "--run-root", &root_s, "--run-id", "r", "--no-detect"]);
    let run = root.join("r").display().to_string();
    let r = aurora(&bin, &["workflow", "submit", "--run", &run, "--stage", "plan", "--json"]);
    assert_eq!(r.code, 1);
    let result = json(&r.stdout);
    assert_eq!((result["state"].as_str(), result["attempts"].as_u64()), (Some("FAILED"), Some(1)));
    let r = aurora(&bin, &["workflow", "submit", "--run", &run, "--stage", "plan", "--not-applicable"]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("not optional"), "{}", r.stderr);
}

#[test]
fn start_requires_run_root() {
    let (_t, bin, _root) = setup();
    let r = aurora(&bin, &["workflow", "start", &example()]);
    assert_eq!(r.code, 2, "clap usage error");
}

#[test]
fn task_list_and_archive_keep_task_files() {
    let (_t, bin, _unused_root) = setup();
    let project = bin.parent().expect("project");
    let run_root = project.join(".aurora/runs");
    fs::create_dir_all(&run_root).expect("run root");
    let run_root_s = run_root.display().to_string();
    let started = aurora(
        &bin,
        &["workflow", "start", &example(), "--run-root", &run_root_s, "--run-id", "task-1", "--no-detect"],
    );
    assert_eq!(started.code, 0, "{}{}", started.stdout, started.stderr);

    let project_s = project.display().to_string();
    let listed = aurora(&bin, &["task", "list", "--project", &project_s, "--json"]);
    assert_eq!(listed.code, 0, "{}{}", listed.stdout, listed.stderr);
    assert_eq!(json(&listed.stdout)[0]["id"], "task-1");

    let archived = aurora(&bin, &["task", "archive", "task-1", "--project", &project_s, "--yes"]);
    assert_eq!(archived.code, 0, "{}{}", archived.stdout, archived.stderr);
    let archived_dir = project.join(".aurora/archive/runs/task-1");
    assert!(archived_dir.join("run.json").is_file());
    let listed = aurora(&bin, &["task", "list", "--project", &project_s, "--json"]);
    assert_eq!(listed.code, 0, "{}{}", listed.stdout, listed.stderr);
    assert_eq!(json(&listed.stdout), serde_json::json!([]));

    let archived_list = aurora(&bin, &["task", "list", "--project", &project_s, "--archived", "--json"]);
    assert_eq!(archived_list.code, 0, "{}{}", archived_list.stdout, archived_list.stderr);
    assert_eq!(json(&archived_list.stdout)[0]["id"], "task-1");

    let restored = aurora(&bin, &["task", "restore", "task-1", "--project", &project_s]);
    assert_eq!(restored.code, 0, "{}{}", restored.stdout, restored.stderr);
    assert!(run_root.join("task-1/run.json").is_file());
    let archived_list = aurora(&bin, &["task", "list", "--project", &project_s, "--archived", "--json"]);
    assert_eq!(json(&archived_list.stdout), serde_json::json!([]));
    let listed = aurora(&bin, &["task", "list", "--project", &project_s, "--json"]);
    assert_eq!(json(&listed.stdout)[0]["id"], "task-1");
}

#[test]
fn start_with_blocked_plan_exits_3() {
    let (t, bin, root) = setup();
    let registry = t.path().join("reg.toml");
    fs::copy(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/parity/fixture-registry.toml"),
        &registry,
    )
    .expect("registry");
    let wf = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/parity/minimal.toml");
    let r = aurora(
        &bin,
        &[
            "workflow",
            "start",
            &wf.display().to_string(),
            "--registry",
            &registry.display().to_string(),
            "--run-root",
            &root.display().to_string(),
            "--run-id",
            "b",
            "--no-detect",
            "--provider",
            "antigravity",
        ],
    );
    assert_eq!(r.code, 3, "{}{}", r.stdout, r.stderr);
    assert!(r.stdout.contains("[BLOCKED] check"));
}

#[test]
fn run_commands_report_errors_without_tracebacks() {
    let (_t, bin, root) = setup();
    let r = aurora(&bin, &["workflow", "status", "--run", &root.display().to_string()]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.starts_with("Error:"), "{}", r.stderr);
    assert!(!root.join(".awb.lock").exists());
}

#[test]
fn waive_via_cli_requires_reason_and_records_it() {
    let (_t, bin, root) = setup();
    let root_s = root.display().to_string();
    aurora(&bin, &["workflow", "start", &example(), "--run-root", &root_s, "--run-id", "w", "--no-detect"]);
    let run = root.join("w").display().to_string();
    aurora(&bin, &["workflow", "submit", "--run", &run, "--stage", "plan"]);
    let r = aurora(&bin, &["workflow", "waive", "--run", &run, "--stage", "plan", "--by", "lead"]);
    assert_eq!(r.code, 2, "--reason is required");
    let r = aurora(
        &bin,
        &["workflow", "waive", "--run", &run, "--stage", "plan", "--reason", "spike only", "--by", "lead"],
    );
    assert_eq!(r.code, 0, "{}{}", r.stdout, r.stderr);
    let status = json(&aurora(&bin, &["workflow", "status", "--run", &run, "--json"]).stdout);
    assert_eq!(status["waived"], serde_json::json!(["plan"]));
    let text = aurora(&bin, &["workflow", "status", "--run", &run]).stdout;
    assert!(text.contains("[waived] accepted by recorded waiver: plan"), "{text}");
}

#[test]
fn presets_list_and_init() {
    let (t, bin, _root) = setup();
    let r = aurora(&bin, &["workflow", "presets", "--json"]);
    assert_eq!(r.code, 0);
    let list = json(&r.stdout);
    assert_eq!(list.as_array().expect("array").len(), 6);
    assert_eq!(list[0]["name"], "feature");
    assert!(list[0]["stages"].as_array().expect("stages").len() >= 5);

    let out = t.path().join("my-feature.toml");
    let out_s = out.display().to_string();
    let r = aurora(&bin, &["workflow", "init", "feature", "--output", &out_s]);
    assert_eq!(r.code, 0, "{}", r.stderr);
    assert!(r.stdout.contains("aurora workflow plan"), "{}", r.stdout);
    let r = aurora(&bin, &["workflow", "validate", &out_s]);
    assert_eq!(r.code, 0, "{}", r.stdout);
    let r = aurora(&bin, &["workflow", "init", "feature", "--output", &out_s]);
    assert_eq!(r.code, 1, "never overwrites");
    assert!(r.stderr.contains("exists"), "{}", r.stderr);
    let r =
        aurora(&bin, &["workflow", "init", "waterfall", "--output", &t.path().join("x.toml").display().to_string()]);
    assert_eq!(r.code, 1);
    assert!(r.stderr.contains("feature"), "lists known presets: {}", r.stderr);
}

#[test]
fn closed_stdout_does_not_panic() {
    use std::process::{Command as Std, Stdio};
    let (_t, bin, _root) = setup();
    for args in [vec!["providers", "--json"], vec!["workflow", "presets"]] {
        let mut child = Std::new(assert_cmd::cargo::cargo_bin("aurora"))
            .args(&args)
            .env("PATH", &bin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn");
        drop(child.stdout.take()); // reader goes away before the CLI writes
        let out = child.wait_with_output().expect("wait");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stderr.contains("panicked"), "{args:?}: {stderr}");
    }
}
