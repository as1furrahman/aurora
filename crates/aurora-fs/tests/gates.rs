//! Step 6.1 — gate outcomes (additive to schema v1): structured verdicts (pass | concerns |
//! fail), concerns carried into downstream handoffs, and recorded waivers. Records stay
//! loadable by the Python implementation (new data lives in the top-level `gates` map).

use std::fs;
use std::path::{Path, PathBuf};

use aurora_core::registry::parse_registry_str;
use aurora_core::workflow::{PlanOptions, StageState};
use aurora_fs::boundaries::BoundaryPolicy;
use aurora_fs::engine::Engine;
use aurora_fs::runs::{RunError, RunStore};

const FROZEN: &str = "2026-10-23T12:00:00Z";
const MINIMAL: &str = include_str!("../../../tests/fixtures/parity/minimal.toml");
const REGISTRY: &str = include_str!("../../../tests/fixtures/parity/fixture-registry.toml");

/// build -> check (check emits a structured verdict) -> ship.
fn definition() -> String {
    MINIMAL
        .replace("outputs = [\"check.md\"]", "outputs = [\"check.md\", \"check.json\"]")
        .replace("independent_of = [\"build\"]\n", "independent_of = [\"build\"]\nverdict = \"check.json\"\n")
        + r#"
[[stages]]
id = "ship"
role = "builder"
depends_on = ["check"]
outputs = ["ship.md"]

  [[stages.acceptance]]
  id = "shipped"
  description = "ship.md exists."
  evidence = ["ship.md"]
"#
}

struct T {
    _t: tempfile::TempDir,
    engine: Engine,
    run_dir: PathBuf,
}

fn setup() -> T {
    let t = tempfile::tempdir().expect("tempdir");
    let base = t.path().canonicalize().expect("canonical");
    fs::create_dir(base.join("home")).expect("home");
    fs::create_dir(base.join("runs")).expect("runs");
    fs::write(base.join("wf.toml"), definition()).expect("wf");
    let engine = Engine::with_store(RunStore::with_policy(BoundaryPolicy::for_home(&base.join("home"))));
    let reg = parse_registry_str(REGISTRY).expect("registry");
    let run_dir = engine
        .start_run(
            &base.join("wf.toml"),
            &base.join("runs"),
            Some(&reg),
            &PlanOptions::default(),
            Some("g"),
            Some(FROZEN),
        )
        .expect("start")
        .run_dir;
    T { _t: t, engine, run_dir }
}

impl T {
    fn produce(&self, stage: &str, name: &str, content: &str) {
        fs::write(self.run_dir.join("artifacts").join(stage).join(name), content).expect("produce");
    }

    fn submit(&self, stage: &str, met: &[&str]) -> aurora_fs::engine::SubmitResult {
        let met: Vec<String> = met.iter().map(|s| (*s).to_string()).collect();
        self.engine.submit(&self.run_dir, stage, &met, false, Some(FROZEN)).expect("submit")
    }

    fn pass_build(&self) {
        self.produce("build", "out.md", "built\n");
        assert_eq!(self.submit("build", &["built"]).state, StageState::Passed);
    }

    fn check_with(&self, verdict: &str) -> aurora_fs::engine::SubmitResult {
        self.produce("check", "check.md", "checked\n");
        self.produce("check", "check.json", verdict);
        self.submit("check", &["checked"])
    }

    fn state(&self, stage: &str) -> StageState {
        self.engine.store.load_record(&self.run_dir).expect("record").stages[stage].state
    }

    fn bundle(&self, stage: &str) -> String {
        fs::read_to_string(self.run_dir.join("handoff").join(format!("{stage}.md"))).unwrap_or_default()
    }
}

#[test]
fn pass_verdict_passes_and_is_recorded() {
    let t = setup();
    t.pass_build();
    assert_eq!(t.check_with(r#"{"outcome": "pass", "findings": []}"#).state, StageState::Passed);
    let record = t.engine.store.load_record(&t.run_dir).expect("record");
    assert_eq!(record.gates["check"].outcome, "pass");
    assert!(!t.bundle("ship").contains("Open concerns"));
}

#[test]
fn concerns_pass_and_reach_the_next_handoff() {
    let t = setup();
    t.pass_build();
    let verdict = r#"{"outcome": "concerns", "findings": [{"severity": "major", "summary": "Error path untested"}]}"#;
    assert_eq!(t.check_with(verdict).state, StageState::Passed);
    let bundle = t.bundle("ship");
    assert!(bundle.contains("## Open concerns from upstream stages"), "{bundle}");
    assert!(bundle.contains("`check` (major): Error path untested"), "{bundle}");
    let status = t.engine.status(&t.run_dir).expect("status");
    assert_eq!(status.concerns, ["check"]);
}

#[test]
fn fail_verdict_fails_the_stage_even_with_all_criteria_claimed() {
    let t = setup();
    t.pass_build();
    let r = t.check_with(r#"{"outcome": "fail", "findings": [{"severity": "critical", "summary": "Data loss"}]}"#);
    assert_eq!(r.state, StageState::Failed);
    assert!(
        r.missing.iter().any(|m| m.contains("verdict outcome is fail") && m.contains("1 finding")),
        "{:?}",
        r.missing
    );
    assert_eq!(t.state("ship"), StageState::Pending);
}

#[test]
fn malformed_verdict_is_missing_evidence() {
    for bad in [
        "not json",
        r#"{"outcome": "maybe", "findings": []}"#,
        r#"{"outcome": "pass"}"#,
        r#"{"outcome": "pass", "findings": [{"severity": 1}]}"#,
    ] {
        let t = setup();
        t.pass_build();
        let r = t.check_with(bad);
        assert_eq!(r.state, StageState::Failed, "{bad}");
        assert!(r.missing.iter().any(|m| m.contains("verdict invalid")), "{bad}: {:?}", r.missing);
    }
}

#[test]
fn waiver_requires_reason_and_approver_and_unblocks_downstream() {
    let t = setup();
    t.pass_build();
    t.submit("check", &[]);
    t.submit("check", &[]);
    assert_eq!(t.state("check"), StageState::Blocked);
    let waive = |reason: &str, by: &str| t.engine.waive(&t.run_dir, "check", reason, by, Some(FROZEN));
    assert!(matches!(waive("", "lead"), Err(RunError::Run(_))));
    assert!(matches!(waive("accepted risk", " "), Err(RunError::Run(_))));
    assert!(
        matches!(t.engine.waive(&t.run_dir, "ship", "x", "lead", Some(FROZEN)), Err(RunError::Run(_))),
        "PENDING cannot be waived"
    );
    waive("Known flaky check; tracked in #42", "lead").expect("waive");
    assert_eq!(t.state("check"), StageState::Passed);
    assert_eq!(t.state("ship"), StageState::Running);
    let record = t.engine.store.load_record(&t.run_dir).expect("record");
    let waiver = record.gates["check"].waiver.as_ref().expect("waiver");
    assert_eq!((waiver.by.as_str(), waiver.reason.as_str()), ("lead", "Known flaky check; tracked in #42"));
    assert!(t.bundle("ship").contains("WAIVED by lead: Known flaky check; tracked in #42"));
    let status = t.engine.status(&t.run_dir).expect("status");
    assert_eq!(status.waived, ["check"]);
}

#[test]
fn passed_stages_cannot_be_waived() {
    let t = setup();
    t.pass_build();
    assert!(t.engine.waive(&t.run_dir, "build", "why", "lead", Some(FROZEN)).is_err());
}

#[test]
fn records_with_gates_keep_v1_stage_shape() {
    let t = setup();
    t.pass_build();
    t.check_with(r#"{"outcome": "concerns", "findings": [{"severity": "minor", "summary": "Naming"}]}"#);
    let raw: serde_json::Value =
        serde_json::from_slice(&fs::read(Path::new(&t.run_dir).join("run.json")).expect("read")).expect("json");
    let stage_keys: Vec<&String> = raw["stages"]["check"].as_object().expect("stage").keys().collect();
    assert_eq!(stage_keys.len(), 12, "stage records keep exactly the v1 keys: {stage_keys:?}");
    assert_eq!(raw["gates"]["check"]["outcome"], "concerns");
}
