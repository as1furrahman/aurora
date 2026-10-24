//! Run store (port of `awb/workflow/runs.py`): run-id rules, create-never-reuse, boundary-
//! checked atomic writes, strict record loading, and schema compatibility with records written
//! by the Python implementation (`tests/fixtures/runs/python-run/run.json`).

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use aurora_core::workflow::StageState;
use aurora_core::workflow::record::RunRecord;
use aurora_fs::boundaries::BoundaryPolicy;
use aurora_fs::runs::{RunError, RunStore, validate_run_id};
use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn python_run() -> Vec<u8> {
    fs::read(repo_root().join("tests/fixtures/runs/python-run/run.json")).expect("python golden")
}

struct Sandbox {
    _t: tempfile::TempDir,
    root: PathBuf,
    store: RunStore,
}

fn sandbox() -> Sandbox {
    let t = tempfile::tempdir().expect("tempdir");
    let base = t.path().canonicalize().expect("canonical");
    let home = base.join("home");
    fs::create_dir(&home).expect("home");
    let root = base.join("runs");
    fs::create_dir(&root).expect("runs");
    Sandbox { store: RunStore::with_policy(BoundaryPolicy::for_home(&home)), root, _t: t }
}

#[test]
fn run_ids_are_restricted() {
    for ok in ["r1", "plan-implement-review-20261023T120000Z", "a.b_c-d"] {
        assert!(validate_run_id(ok).is_ok(), "{ok}");
    }
    for bad in ["", ".", "..", ".hidden", "../escape", "a/b", "x\n", "-x", &"a".repeat(129)] {
        assert!(matches!(validate_run_id(bad), Err(RunError::Run(_))), "{bad:?}");
    }
}

#[test]
fn create_run_dir_is_private_and_never_reused() {
    let s = sandbox();
    let dir = s.store.create_run_dir(&s.root, "r1").expect("create");
    assert_eq!(dir, s.root.join("r1"));
    assert_eq!(fs::metadata(&dir).expect("meta").permissions().mode() & 0o777, 0o700);
    assert!(matches!(s.store.create_run_dir(&s.root, "r1"), Err(RunError::Run(m)) if m.contains("refusing to reuse")));
}

#[test]
fn run_root_must_exist() {
    let s = sandbox();
    assert!(matches!(s.store.create_run_dir(&s.root.join("missing"), "r1"), Err(RunError::Run(_))));
}

#[test]
fn run_root_inside_global_agent_dir_is_rejected_without_writing() {
    let t = tempfile::tempdir().expect("tempdir");
    let home = t.path().canonicalize().expect("canonical");
    let global = home.join(".claude/skills");
    fs::create_dir_all(&global).expect("mkdir");
    let store = RunStore::with_policy(BoundaryPolicy::for_home(&home));
    assert!(store.create_run_dir(&global, "x").is_err());
    assert_eq!(fs::read_dir(&global).expect("dir").count(), 0);
}

#[test]
fn writes_stay_inside_the_run_dir() {
    let s = sandbox();
    let dir = s.store.create_run_dir(&s.root, "r1").expect("create");
    s.store.ensure_dir(&dir, "artifacts/plan").expect("ensure");
    s.store.write_file(&dir, "artifacts/plan/notes.md", b"hi").expect("write");
    assert_eq!(s.store.read_file(&dir, "artifacts/plan/notes.md", 1024).expect("read").content, b"hi");
    for escape in ["../outside.md", "artifacts/../../outside.md"] {
        assert!(s.store.write_file(&dir, escape, b"x").is_err(), "{escape}");
    }
    assert!(s.store.ensure_dir(&dir, "../sibling").is_err());
    assert!(!s.root.join("outside.md").exists() && !s.root.join("sibling").exists());
}

#[test]
fn loads_records_written_by_python() {
    let record = RunRecord::from_json(&python_run()).expect("python record is valid v1");
    assert_eq!(record.run_id, "golden");
    assert_eq!(record.stage("plan").expect("plan").state, StageState::Passed);
    assert_eq!(record.stage("plan").expect("plan").attempts, 2);
    assert_eq!(record.stage("implement").expect("implement").state, StageState::Running);
    assert_eq!(record.events.len(), 7);
}

#[test]
fn save_and_load_round_trip_is_semantically_identical_to_python() {
    let s = sandbox();
    let dir = s.store.create_run_dir(&s.root, "golden").expect("create");
    let record = RunRecord::from_json(&python_run()).expect("valid");
    s.store.save_record(&dir, &record).expect("save");
    let reloaded = s.store.load_record(&dir).expect("load");
    assert_eq!(reloaded, record);
    let ours: Value = serde_json::from_slice(&fs::read(dir.join("run.json")).expect("read")).expect("json");
    let theirs: Value = serde_json::from_slice(&python_run()).expect("json");
    assert_eq!(ours, theirs, "same JSON document as the Python writer");
    let text = fs::read_to_string(dir.join("run.json")).expect("read");
    assert!(text.ends_with("}\n") && text.contains("\n  \"events\": ["), "sorted, 2-space indented");
}

type Mutation = Box<dyn Fn(&mut Value)>;

#[test]
fn record_schema_is_strict() {
    let s = sandbox();
    let dir = s.store.create_run_dir(&s.root, "r1").expect("create");
    let good: Value = serde_json::from_slice(&python_run()).expect("json");
    let mutations: Vec<Mutation> = vec![
        Box::new(|r| r["schema_version"] = 2.into()),
        Box::new(|r| r["kind"] = "other".into()),
        Box::new(|r| r["stages"]["plan"]["state"] = "DONE".into()),
        Box::new(|r| {
            r["stages"].as_object_mut().expect("stages").remove("plan");
        }),
        Box::new(|r| r["stages"]["plan"]["surprise"] = true.into()),
        Box::new(|r| {
            r.as_object_mut().expect("root").remove("events");
        }),
    ];
    for (i, mutate) in mutations.iter().enumerate() {
        let mut bad = good.clone();
        mutate(&mut bad);
        fs::write(dir.join("run.json"), serde_json::to_vec(&bad).expect("json")).expect("write");
        assert!(matches!(s.store.load_record(&dir), Err(RunError::Record(_))), "mutation {i} accepted");
    }
    // Unknown *top-level* keys are tolerated (forward compatibility), as in Python.
    let mut extra = good.clone();
    extra["future_field"] = 1.into();
    fs::write(dir.join("run.json"), serde_json::to_vec(&extra).expect("json")).expect("write");
    assert!(s.store.load_record(&dir).is_ok());
}

#[test]
fn not_a_run_directory() {
    let s = sandbox();
    assert!(
        matches!(s.store.load_record(&s.root), Err(RunError::Record(m)) if m.contains("not an Aurora run directory"))
    );
}

#[test]
fn symlinked_record_is_refused() {
    let s = sandbox();
    let dir = s.store.create_run_dir(&s.root, "r1").expect("create");
    let elsewhere = s.root.join("elsewhere.json");
    fs::write(&elsewhere, python_run()).expect("write");
    std::os::unix::fs::symlink(&elsewhere, dir.join("run.json")).expect("symlink");
    assert!(s.store.load_record(&dir).is_err());
}

#[test]
fn lock_is_taken_inside_the_run_dir() {
    let s = sandbox();
    let dir = s.store.create_run_dir(&s.root, "r1").expect("create");
    {
        let _guard = s.store.lock(&dir).expect("lock");
        assert!(dir.join(".awb.lock").is_file());
    }
    assert!(Path::new(&dir.join(".awb.lock")).is_file());
}
