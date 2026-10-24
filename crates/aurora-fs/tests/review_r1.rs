//! Regressions for the independent R1 code review (2026-10-24).

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use aurora_core::registry::parse_registry_str;
use aurora_core::workflow::{PlanOptions, StageState};
use aurora_fs::boundaries::BoundaryPolicy;
use aurora_fs::engine::{CrashPoint, Engine};
use aurora_fs::paths::open_dir_nofollow;
use aurora_fs::runs::{RunError, RunStore};
use serde_json::Value;

const FROZEN: &str = "2026-10-23T12:00:00Z";
const MINIMAL: &str = include_str!("../../../tests/fixtures/parity/minimal.toml");
const REGISTRY: &str = include_str!("../../../tests/fixtures/parity/fixture-registry.toml");

struct T {
    _t: tempfile::TempDir,
    base: PathBuf,
    engine: Engine,
}

fn setup() -> T {
    let t = tempfile::tempdir().expect("tempdir");
    let base = t.path().canonicalize().expect("canonical");
    fs::create_dir(base.join("home")).expect("home");
    fs::create_dir(base.join("runs")).expect("runs");
    let engine = Engine::with_store(RunStore::with_policy(BoundaryPolicy::for_home(&base.join("home"))));
    T { _t: t, base, engine }
}

impl T {
    fn start(&self, text: &str, opts: &PlanOptions, id: &str) -> Result<PathBuf, RunError> {
        let wf = self.base.join(format!("{id}.toml"));
        fs::write(&wf, text).expect("wf");
        let reg = parse_registry_str(REGISTRY).expect("registry");
        self.engine.start_run(&wf, &self.base.join("runs"), Some(&reg), opts, Some(id), Some(FROZEN)).map(|s| s.run_dir)
    }

    fn submit(&self, run: &Path, stage: &str, met: &[&str]) -> aurora_fs::engine::SubmitResult {
        let met: Vec<String> = met.iter().map(|s| (*s).to_string()).collect();
        self.engine.submit(run, stage, &met, false, Some(FROZEN)).expect("submit")
    }
}

#[test]
fn failed_resubmit_clears_stale_evidence_so_a_waiver_can_complete() {
    let t = setup();
    let text =
        MINIMAL.replace("independent_of = [\"build\"]\n", "independent_of = [\"build\"]\nrequires_approval = true\n");
    let run = t.start(&text, &PlanOptions::default(), "r").expect("start");
    fs::write(run.join("artifacts/build/out.md"), "built\n").expect("out");
    t.submit(&run, "build", &["built"]);
    fs::write(run.join("artifacts/check/check.md"), "v1\n").expect("check");
    assert!(t.submit(&run, "check", &["checked"]).awaiting_approval);
    fs::write(run.join("artifacts/check/check.md"), "").expect("emptied");
    // The gate-satisfied submit was attempt 1 of 2, so this failed resubmit exhausts the stage.
    assert_eq!(t.submit(&run, "check", &["checked"]).state, StageState::Blocked);
    let st = t.engine.store.load_record(&run).expect("record").stages["check"].clone();
    assert!(st.evidence.is_empty() && st.criteria_met.is_empty(), "stale evidence kept: {st:?}");
    t.engine.waive(&run, "check", "accepted for release", "lead", Some(FROZEN)).expect("waive");
    let status = t.engine.status(&run).expect("status");
    assert!(status.drift.is_empty() && status.complete, "{:?}", status.drift);
}

#[test]
fn waive_requires_dependencies_to_be_done() {
    let t = setup();
    let run = t
        .start(MINIMAL, &PlanOptions { provider_override: Some("antigravity"), ..Default::default() }, "r")
        .expect("start");
    // `check` is BLOCKED by the plan while `build` is still RUNNING.
    let err = t.engine.waive(&run, "check", "skip it", "lead", Some(FROZEN)).expect_err("deps not done");
    assert!(err.to_string().contains("build"), "{err}");
}

#[test]
fn failed_start_before_the_first_record_leaves_no_run_directory() {
    let t = setup();
    t.engine.inject_crash(CrashPoint::AfterRunDirCreated);
    assert!(t.start(MINIMAL, &PlanOptions::default(), "bad").is_err());
    assert!(!t.base.join("runs/bad").exists(), "half-created run directory left behind");
    t.start(MINIMAL, &PlanOptions::default(), "bad").expect("id reusable after a failed start");
}

type Mutation = Box<dyn Fn(&mut Value)>;

fn tamper(run: &Path, mutate: impl Fn(&mut Value)) -> Result<aurora_fs::engine::StatusReport, RunError> {
    let path = run.join("run.json");
    let mut record: Value = serde_json::from_slice(&fs::read(&path).expect("read")).expect("json");
    mutate(&mut record);
    fs::write(&path, serde_json::to_vec(&record).expect("json")).expect("write");
    Engine::host().status(run)
}

#[test]
fn hand_edited_records_are_rejected_not_panicking() {
    let t = setup();
    let run = t.start(MINIMAL, &PlanOptions::default(), "r").expect("start");
    let original = fs::read(run.join("run.json")).expect("read");
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "missing optional stage key",
            Box::new(|r| {
                r["stages"]["build"].as_object_mut().expect("stage").remove("approval");
            }),
        ),
        ("order lists an unknown stage", Box::new(|r| r["plan"]["order"] = serde_json::json!(["build", "ghost"]))),
        ("order misses a stage", Box::new(|r| r["plan"]["order"] = serde_json::json!(["build"]))),
        ("non-hex workflow sha", Box::new(|r| r["workflow"]["sha256"] = "zzzzzzzzzzzzé".into())),
    ];
    for (label, mutate) in cases {
        fs::write(run.join("run.json"), &original).expect("restore");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tamper(&run, &mutate)));
        let outcome = result.unwrap_or_else(|_| panic!("{label}: panicked"));
        assert!(matches!(outcome, Err(RunError::Record(_))), "{label}: {outcome:?}");
    }
}

#[test]
fn directory_walk_refuses_symlinked_intermediate_components() {
    let t = setup();
    fs::create_dir_all(t.base.join("real/inner")).expect("mkdir");
    symlink(t.base.join("real"), t.base.join("link")).expect("symlink");
    assert!(open_dir_nofollow(&t.base.join("real/inner")).is_ok());
    assert!(open_dir_nofollow(&t.base.join("link/inner")).is_err(), "intermediate symlink must be refused");
    assert!(open_dir_nofollow(&t.base.join("link")).is_err(), "final symlink must be refused");
}
