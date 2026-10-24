//! Evidence-gated run tracking: run layout, acceptance gates, optional stages, approval,
//! completion, resume, integrity, and review regressions.
//! Every run lives in a temporary directory; the boundary policy uses a fake home.

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use aurora_core::providers::PROVIDERS;
use aurora_core::registry::{Registry, parse_registry_str};
use aurora_core::util::sha256_hex;
use aurora_core::workflow::{PlanOptions, StageState};
use aurora_fs::FsError;
use aurora_fs::boundaries::BoundaryPolicy;
use aurora_fs::engine::{CrashPoint, Engine};
use aurora_fs::runs::{RunError, RunStore};
use serde_json::Value;

const FROZEN: &str = "2026-10-23T12:00:00Z";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn example() -> PathBuf {
    repo_root().join("workflows/examples/plan-implement-review.toml")
}

fn load_registry(rel: &str) -> Registry {
    parse_registry_str(&fs::read_to_string(repo_root().join(rel)).expect("registry")).expect("valid")
}

struct T {
    _tmp: tempfile::TempDir,
    base: PathBuf,
    root: PathBuf,
    engine: Engine,
    registry: Registry,
}

fn setup() -> T {
    let tmp = tempfile::tempdir().expect("tempdir");
    let base = tmp.path().canonicalize().expect("canonical");
    let home = base.join("home");
    fs::create_dir(&home).expect("home");
    let root = base.join("runs");
    fs::create_dir(&root).expect("runs");
    let engine = Engine::with_store(RunStore::with_policy(BoundaryPolicy::for_home(&home)));
    T { _tmp: tmp, base, root, engine, registry: load_registry("registry/skills.toml") }
}

impl T {
    fn start_with(&self, definition: &Path, registry: &Registry, run_id: &str, opts: &PlanOptions) -> PathBuf {
        self.engine
            .start_run(definition, &self.root, Some(registry), opts, Some(run_id), Some(FROZEN))
            .expect("start")
            .run_dir
    }

    fn start(&self, run_id: &str) -> PathBuf {
        self.start_with(&example(), &self.registry, run_id, &PlanOptions::default())
    }

    fn minimal(&self, text: &str, opts: &PlanOptions) -> PathBuf {
        let path = self.base.join("mini.toml");
        fs::write(&path, text).expect("write");
        let reg = load_registry("tests/fixtures/parity/fixture-registry.toml");
        self.start_with(&path, &reg, "r1", opts)
    }

    fn produce(&self, run_dir: &Path, stage: &str, name: &str, content: &str) -> PathBuf {
        let path = run_dir.join("artifacts").join(stage).join(name);
        fs::write(&path, content).expect("produce");
        path
    }

    fn states(&self, run_dir: &Path) -> Vec<(String, StageState)> {
        let record = self.engine.store.load_record(run_dir).expect("record");
        record.plan.order.iter().map(|id| (id.clone(), record.stages[id].state)).collect()
    }

    fn state(&self, run_dir: &Path, stage: &str) -> StageState {
        self.engine.store.load_record(run_dir).expect("record").stages[stage].state
    }

    fn submit(&self, run_dir: &Path, stage: &str, met: &[&str]) -> Result<aurora_fs::engine::SubmitResult, RunError> {
        let met: Vec<String> = met.iter().map(|s| (*s).to_string()).collect();
        self.engine.submit(run_dir, stage, &met, false, Some(FROZEN))
    }

    fn not_applicable(&self, run_dir: &Path, stage: &str) -> Result<aurora_fs::engine::SubmitResult, RunError> {
        self.engine.submit(run_dir, stage, &[], true, Some(FROZEN))
    }

    fn pass(&self, run_dir: &Path, stage: &str, outputs: &[&str], met: &[&str]) -> aurora_fs::engine::SubmitResult {
        for out in outputs {
            self.produce(run_dir, stage, out, "evidence\n");
        }
        self.submit(run_dir, stage, met).expect("submit")
    }

    fn advance_to_remediate(&self, run_dir: &Path) {
        self.pass(run_dir, "plan", &["plan.md"], &["plan-written"]);
        self.pass(run_dir, "implement", &["change-summary.md"], &["change-described"]);
        self.pass(run_dir, "test", &["test-report.md"], &["suite-green"]);
        self.pass(run_dir, "review", &["review.md"], &["review-complete"]);
    }
}

const MINIMAL: &str = include_str!("../../../tests/fixtures/parity/minimal.toml");

// --- run layout -------------------------------------------------------------------

#[test]
fn start_creates_versioned_record_and_first_handoff() {
    let t = setup();
    let run_dir = t.start("r1");
    assert_eq!(run_dir, t.root.join("r1"));
    let record = t.engine.store.load_record(&run_dir).expect("record");
    assert_eq!((record.schema_version, record.kind.as_str()), (1, "aurora.workflow-run"));
    assert_eq!(fs::read(run_dir.join("workflow.toml")).expect("pinned"), fs::read(example()).expect("source"));
    assert_eq!(record.workflow.sha256, sha256_hex(&fs::read(example()).expect("source")));
    let states: Vec<(&str, StageState)> = vec![
        ("plan", StageState::Running),
        ("implement", StageState::Pending),
        ("test", StageState::Pending),
        ("review", StageState::Pending),
        ("remediate", StageState::Pending),
        ("final-verify", StageState::Pending),
    ];
    let got = t.states(&run_dir);
    assert_eq!(got.iter().map(|(a, b)| (a.as_str(), *b)).collect::<Vec<_>>(), states);
    assert!(run_dir.join("handoff/plan.md").is_file());
    assert!(!run_dir.join("handoff/implement.md").exists());
    assert!(run_dir.join("artifacts/plan").is_dir());
    assert_eq!(record.plan.mode, "multi");
}

#[test]
fn handoff_bundle_is_role_specific_and_safe() {
    let t = setup();
    let run_dir = t.start("r1");
    let text = fs::read_to_string(run_dir.join("handoff/plan.md")).expect("bundle");
    for needle in [
        "planner",
        "scoped implementation plan",
        "principles",
        "artifacts/plan/plan.md",
        "plan-written",
        "Claude Code",
        "exit code",
    ] {
        assert!(text.contains(needle), "{needle}");
    }
    assert!(
        text.contains(&format!("aurora workflow submit --run {} --stage plan --met plan-written", run_dir.display()))
    );
    for p in &PROVIDERS {
        for flag in p.forbidden_flags {
            assert!(!text.contains(flag));
        }
    }
}

#[test]
fn handoff_bundles_are_deterministic() {
    let t = setup();
    let a = t.start("a");
    let b = t.start("b");
    let norm = |dir: &Path, id: &str| {
        fs::read_to_string(dir.join("handoff/plan.md"))
            .expect("bundle")
            .replace(&dir.display().to_string(), "<RUN>")
            .replace(&format!("Run: {id}"), "Run: <ID>")
    };
    assert_eq!(norm(&a, "a"), norm(&b, "b"));
}

#[test]
fn single_provider_run_records_reduced_independence() {
    let t = setup();
    let run_dir = t.start_with(
        &example(),
        &t.registry,
        "r1",
        &PlanOptions { provider_override: Some("codex"), ..Default::default() },
    );
    let record = t.engine.store.load_record(&run_dir).expect("record");
    assert!(record.stages.values().all(|s| s.provider == "codex"));
    assert_eq!(record.plan.stage("review").expect("review").independence[0].status, "same-provider");
    assert!(fs::read_to_string(run_dir.join("handoff/plan.md")).expect("bundle").contains("Codex"));
}

#[test]
fn plan_blocked_stage_starts_blocked() {
    let t = setup();
    let run_dir = t.minimal(MINIMAL, &PlanOptions { provider_override: Some("antigravity"), ..Default::default() });
    assert_eq!(t.state(&run_dir, "build"), StageState::Running);
    assert_eq!(t.state(&run_dir, "check"), StageState::Blocked);
    assert!(!t.engine.store.load_record(&run_dir).expect("record").stages["check"].blocked_reasons.is_empty());
}

#[test]
fn run_root_must_exist_and_run_id_is_safe() {
    let t = setup();
    let start = |root: &Path, id: Option<&str>| {
        t.engine.start_run(&example(), root, Some(&t.registry), &PlanOptions::default(), id, Some(FROZEN))
    };
    assert!(matches!(start(&t.root.join("missing"), Some("x")), Err(RunError::Run(_))));
    for bad in ["../escape", "a/b", ".", "..", "", ".hidden"] {
        assert!(matches!(start(&t.root, Some(bad)), Err(RunError::Run(_))), "{bad:?}");
    }
    t.start("taken");
    assert!(matches!(start(&t.root, Some("taken")), Err(RunError::Run(_))));
}

#[test]
fn default_run_id_uses_workflow_and_time() {
    let t = setup();
    let started = t
        .engine
        .start_run(&example(), &t.root, Some(&t.registry), &PlanOptions::default(), None, Some(FROZEN))
        .expect("start");
    assert_eq!(started.run_dir.file_name().expect("name"), "plan-implement-review-20261023T120000Z");
}

#[test]
fn run_root_inside_global_agent_dir_is_rejected() {
    let t = setup();
    let global = t.base.join("home/.claude/skills");
    fs::create_dir_all(&global).expect("mkdir");
    let err = t
        .engine
        .start_run(&example(), &global, Some(&t.registry), &PlanOptions::default(), Some("x"), Some(FROZEN))
        .expect_err("global");
    assert!(matches!(err, RunError::Fs(FsError::GlobalMutationBlocked(_))), "{err}");
    assert_eq!(fs::read_dir(&global).expect("dir").count(), 0);
}

// --- acceptance gates -------------------------------------------------------------

#[test]
fn missing_outputs_fail_and_do_not_advance() {
    let t = setup();
    let run_dir = t.start("r1");
    let r = t.submit(&run_dir, "plan", &["plan-written"]).expect("submit");
    assert_eq!(r.state, StageState::Failed);
    assert!(r.missing.contains(&"output missing: artifacts/plan/plan.md".to_string()));
    assert_eq!(t.state(&run_dir, "implement"), StageState::Pending);
    assert_eq!(t.engine.store.load_record(&run_dir).expect("record").stages["plan"].attempts, 1);
}

#[test]
fn outputs_without_claimed_criteria_do_not_pass() {
    let t = setup();
    let run_dir = t.start("r1");
    t.produce(&run_dir, "plan", "plan.md", "x");
    let r = t.submit(&run_dir, "plan", &[]).expect("submit");
    assert_eq!(r.state, StageState::Failed);
    assert!(r.missing.contains(&"criterion 'plan-written' not claimed (use --met plan-written)".to_string()));
}

#[test]
fn empty_and_symlinked_outputs_are_not_evidence() {
    let t = setup();
    let run_dir = t.start("r1");
    t.produce(&run_dir, "plan", "plan.md", "");
    let r = t.submit(&run_dir, "plan", &["plan-written"]).expect("submit");
    assert!(r.missing.contains(&"output empty: artifacts/plan/plan.md".to_string()), "{:?}", r.missing);
    fs::remove_file(run_dir.join("artifacts/plan/plan.md")).expect("rm");
    let real = t.base.join("elsewhere.md");
    fs::write(&real, "x").expect("write");
    symlink(&real, run_dir.join("artifacts/plan/plan.md")).expect("symlink");
    let r = t.submit(&run_dir, "plan", &["plan-written"]).expect("submit");
    assert_eq!(r.state, StageState::Blocked, "second attempt of max 2");
    assert!(r.missing.iter().any(|m| m.contains("not a regular file")));
}

#[test]
fn complete_submission_passes_and_records_evidence() {
    let t = setup();
    let run_dir = t.start("r1");
    let path = t.produce(&run_dir, "plan", "plan.md", "the plan\n");
    let r = t.submit(&run_dir, "plan", &["plan-written"]).expect("submit");
    assert_eq!(r.state, StageState::Passed);
    let record = t.engine.store.load_record(&run_dir).expect("record");
    let st = &record.stages["plan"];
    assert_eq!(st.evidence.get("artifacts/plan/plan.md"), Some(&sha256_hex(&fs::read(path).expect("read"))));
    assert_eq!(st.criteria_met, ["plan-written"]);
    assert_eq!(record.stages["implement"].state, StageState::Running);
    assert!(run_dir.join("handoff/implement.md").is_file());
}

#[test]
fn resubmitting_passed_stage_is_rejected() {
    let t = setup();
    let run_dir = t.start("r1");
    t.pass(&run_dir, "plan", &["plan.md"], &["plan-written"]);
    let err = t.submit(&run_dir, "plan", &["plan-written"]).expect_err("passed");
    assert!(err.to_string().contains("PASSED"));
}

#[test]
fn submit_to_pending_stage_is_rejected() {
    let t = setup();
    let run_dir = t.start("r1");
    assert!(t.submit(&run_dir, "review", &["review-complete"]).is_err());
}

#[test]
fn unknown_criterion_rejected_without_consuming_attempt() {
    let t = setup();
    let run_dir = t.start("r1");
    assert!(t.submit(&run_dir, "plan", &["plan-written", "made-up"]).is_err());
    assert_eq!(t.engine.store.load_record(&run_dir).expect("record").stages["plan"].attempts, 0);
    assert!(t.submit(&run_dir, "nope", &[]).is_err());
}

#[test]
fn exhausted_attempts_block_and_escalate() {
    let t = setup();
    let run_dir = t.start("r1");
    t.submit(&run_dir, "plan", &[]).expect("1");
    let r = t.submit(&run_dir, "plan", &[]).expect("2");
    assert_eq!(r.state, StageState::Blocked);
    assert!(t.engine.store.load_record(&run_dir).expect("record").stages["plan"].escalated);
    let status = t.engine.status(&run_dir).expect("status");
    assert!(!status.complete);
    let implement = status.stages.iter().find(|s| s.id == "implement").expect("implement");
    assert_eq!(implement.waiting_on, ["plan"]);
    assert_eq!(status.escalations, ["plan"]);
}

#[test]
fn on_exhausted_block_does_not_escalate() {
    let t = setup();
    let text =
        MINIMAL.replace("role = \"builder\"\n", "role = \"builder\"\nmax_attempts = 1\non_exhausted = \"block\"\n");
    let run_dir = t.minimal(&text, &PlanOptions::default());
    let r = t.submit(&run_dir, "build", &[]).expect("submit");
    assert_eq!(r.state, StageState::Blocked);
    assert!(!t.engine.store.load_record(&run_dir).expect("record").stages["build"].escalated);
}

#[test]
fn failed_stage_can_retry() {
    let t = setup();
    let run_dir = t.start("r1");
    t.submit(&run_dir, "plan", &[]).expect("fail");
    assert_eq!(t.pass(&run_dir, "plan", &["plan.md"], &["plan-written"]).state, StageState::Passed);
}

// --- optional stages, approval, completion ------------------------------------------

#[test]
fn not_applicable_requires_optional_stage_and_justification() {
    let t = setup();
    let run_dir = t.start("r1");
    assert!(t.not_applicable(&run_dir, "plan").is_err());
    t.advance_to_remediate(&run_dir);
    let err = t.not_applicable(&run_dir, "remediate").expect_err("no justification");
    assert!(err.to_string().contains("NOT_APPLICABLE.md"), "{err}");
    t.produce(&run_dir, "remediate", "NOT_APPLICABLE.md", "Review found no issues.\n");
    assert_eq!(t.not_applicable(&run_dir, "remediate").expect("na").state, StageState::NotApplicable);
    assert_eq!(t.state(&run_dir, "final-verify"), StageState::Running);
}

#[test]
fn approval_gate_and_completion() {
    let t = setup();
    let run_dir = t.start("r1");
    t.advance_to_remediate(&run_dir);
    t.produce(&run_dir, "remediate", "NOT_APPLICABLE.md", "No findings.\n");
    t.not_applicable(&run_dir, "remediate").expect("na");
    let r = t.pass(&run_dir, "final-verify", &["final-report.md"], &["final-suite-green"]);
    assert_eq!((r.state, r.awaiting_approval), (StageState::Running, true));
    assert!(!t.engine.status(&run_dir).expect("status").complete);
    // Resubmitting while awaiting approval re-evaluates the gate; it never skips approval.
    let again = t.submit(&run_dir, "final-verify", &["final-suite-green"]).expect("resubmit");
    assert_eq!((again.state, again.awaiting_approval), (StageState::Running, true));
    t.engine.approve(&run_dir, "final-verify", "reviewer@example", Some(FROZEN)).expect("approve");
    assert!(t.engine.status(&run_dir).expect("status").complete);
    let record = t.engine.store.load_record(&run_dir).expect("record");
    let st = &record.stages["final-verify"];
    assert_eq!(st.state, StageState::Passed);
    let approval = st.approval.as_ref().expect("approval");
    assert_eq!((approval.by.as_str(), approval.at.as_str()), ("reviewer@example", FROZEN));
}

#[test]
fn approve_rejects_drifted_evidence_and_wrong_state() {
    let t = setup();
    let run_dir = t.start("r1");
    assert!(t.engine.approve(&run_dir, "plan", "me", Some(FROZEN)).is_err());
    t.advance_to_remediate(&run_dir);
    t.produce(&run_dir, "remediate", "NOT_APPLICABLE.md", "No findings.\n");
    t.not_applicable(&run_dir, "remediate").expect("na");
    let path = t.produce(&run_dir, "final-verify", "final-report.md", "green\n");
    t.submit(&run_dir, "final-verify", &["final-suite-green"]).expect("submit");
    fs::write(&path, "tampered\n").expect("tamper");
    assert!(matches!(t.engine.approve(&run_dir, "final-verify", "me", Some(FROZEN)), Err(RunError::Integrity(_))));
    assert_eq!(t.state(&run_dir, "final-verify"), StageState::Running);
}

#[test]
fn completion_requires_every_mandatory_stage() {
    let t = setup();
    let run_dir = t.start("r1");
    t.advance_to_remediate(&run_dir);
    assert!(!t.engine.status(&run_dir).expect("status").complete);
}

// --- resume and integrity ---------------------------------------------------------

#[test]
fn resume_reissues_missing_bundle_identically() {
    let t = setup();
    let run_dir = t.start("r1");
    let bundle = run_dir.join("handoff/plan.md");
    let original = fs::read(&bundle).expect("bundle");
    fs::remove_file(&bundle).expect("rm");
    t.engine.resume(&run_dir, Some(FROZEN)).expect("resume");
    assert_eq!(fs::read(&bundle).expect("bundle"), original);
}

#[test]
fn resume_preserves_edited_bundle() {
    let t = setup();
    let run_dir = t.start("r1");
    let bundle = run_dir.join("handoff/plan.md");
    fs::write(&bundle, "my edited prompt\n").expect("edit");
    let report = t.engine.resume(&run_dir, Some(FROZEN)).expect("resume");
    assert_eq!(fs::read_to_string(&bundle).expect("bundle"), "my edited prompt\n");
    assert!(report.warnings.iter().any(|w| w.contains("plan") && w.contains("not overwritten")));
    let handoff =
        t.engine.store.load_record(&run_dir).expect("record").stages["plan"].handoff.clone().expect("handoff");
    assert!(handoff.preserved_existing);
    assert_eq!(handoff.on_disk_sha256.as_deref(), Some(sha256_hex(b"my edited prompt\n").as_str()));
}

#[test]
fn resume_never_reissues_passed_stages() {
    let t = setup();
    let run_dir = t.start("r1");
    t.pass(&run_dir, "plan", &["plan.md"], &["plan-written"]);
    fs::remove_file(run_dir.join("handoff/plan.md")).expect("rm");
    let before = t.engine.store.load_record(&run_dir).expect("record").events.len();
    t.engine.resume(&run_dir, Some(FROZEN)).expect("resume");
    assert!(!run_dir.join("handoff/plan.md").exists());
    let record = t.engine.store.load_record(&run_dir).expect("record");
    assert_eq!(record.stages["plan"].state, StageState::Passed);
    assert!(!record.events[before..].iter().any(|e| e.stage.as_deref() == Some("plan")));
}

#[test]
fn interrupted_submission_is_safely_resumable() {
    let t = setup();
    let run_dir = t.start("r1");
    t.produce(&run_dir, "plan", "plan.md", "x");
    t.engine.inject_crash(CrashPoint::BeforeRecordSave);
    assert!(t.submit(&run_dir, "plan", &["plan-written"]).is_err());
    // The record was never written: plan is still RUNNING and no attempt was consumed.
    let record = t.engine.store.load_record(&run_dir).expect("record");
    assert_eq!((record.stages["plan"].state, record.stages["plan"].attempts), (StageState::Running, 0));
    let bundle = run_dir.join("handoff/implement.md");
    let early = fs::read(&bundle).ok();
    let r = t.submit(&run_dir, "plan", &["plan-written"]).expect("resubmit");
    assert_eq!(r.state, StageState::Passed);
    if let Some(early) = early {
        assert_eq!(fs::read(&bundle).expect("bundle"), early);
    }
    assert!(r.warnings.is_empty(), "{:?}", r.warnings);
    assert_eq!(t.state(&run_dir, "implement"), StageState::Running);
}

#[test]
fn tampered_workflow_copy_is_detected() {
    let t = setup();
    let run_dir = t.start("r1");
    let mut pinned = fs::read_to_string(run_dir.join("workflow.toml")).expect("pinned");
    pinned.push_str("\n# edited\n");
    fs::write(run_dir.join("workflow.toml"), pinned).expect("tamper");
    assert!(matches!(t.engine.status(&run_dir), Err(RunError::Integrity(_))));
    assert!(matches!(t.engine.resume(&run_dir, Some(FROZEN)), Err(RunError::Integrity(_))));
    assert!(matches!(t.submit(&run_dir, "plan", &[]), Err(RunError::Integrity(_))));
}

#[test]
fn status_reports_evidence_drift_without_changing_state() {
    let t = setup();
    let run_dir = t.start("r1");
    let path = t.produce(&run_dir, "plan", "plan.md", "x");
    t.submit(&run_dir, "plan", &["plan-written"]).expect("submit");
    fs::write(&path, "changed after acceptance\n").expect("drift");
    let status = t.engine.status(&run_dir).expect("status");
    assert_eq!(status.drift.len(), 1);
    assert_eq!(
        (status.drift[0].stage.as_str(), status.drift[0].path.as_str(), status.drift[0].problem.as_str()),
        ("plan", "artifacts/plan/plan.md", "hash mismatch")
    );
    assert_eq!(t.state(&run_dir, "plan"), StageState::Passed);
    fs::remove_file(&path).expect("rm");
    assert_eq!(t.engine.status(&run_dir).expect("status").drift[0].problem, "missing");
}

#[test]
fn not_a_run_directory_is_never_locked() {
    let t = setup();
    assert!(matches!(t.engine.status(&t.root), Err(RunError::Record(_))));
    assert!(t.submit(&t.root, "plan", &[]).is_err());
    assert!(!t.root.join(".awb.lock").exists(), "must not lock arbitrary directories");
}

// --- review regressions -----------------------------------------------------------

fn approval_minimal() -> String {
    MINIMAL.replace("independent_of = [\"build\"]\n", "independent_of = [\"build\"]\nrequires_approval = true\n")
}

#[test]
fn gate_satisfied_after_failure_can_be_approved() {
    let t = setup();
    let run_dir = t.minimal(&approval_minimal(), &PlanOptions::default());
    t.pass(&run_dir, "build", &["out.md"], &["built"]);
    t.submit(&run_dir, "check", &[]).expect("fail first");
    let r = t.pass(&run_dir, "check", &["check.md"], &["checked"]);
    assert_eq!((r.state, r.awaiting_approval), (StageState::Running, true));
    assert_eq!(t.engine.approve(&run_dir, "check", "me", Some(FROZEN)).expect("approve").state, StageState::Passed);
}

#[test]
fn drift_while_awaiting_approval_can_be_resubmitted() {
    let t = setup();
    let run_dir = t.minimal(&approval_minimal(), &PlanOptions::default());
    t.pass(&run_dir, "build", &["out.md"], &["built"]);
    let path = t.produce(&run_dir, "check", "check.md", "v1\n");
    t.submit(&run_dir, "check", &["checked"]).expect("submit");
    fs::write(&path, "v2\n").expect("drift");
    assert!(matches!(t.engine.approve(&run_dir, "check", "me", Some(FROZEN)), Err(RunError::Integrity(_))));
    assert!(t.submit(&run_dir, "check", &["checked"]).expect("resubmit").awaiting_approval);
    assert_eq!(t.engine.approve(&run_dir, "check", "me", Some(FROZEN)).expect("approve").state, StageState::Passed);
    let record = t.engine.store.load_record(&run_dir).expect("record");
    assert_eq!(record.stages["check"].evidence.get("artifacts/check/check.md"), Some(&sha256_hex(b"v2\n")));
}

#[test]
fn failed_start_leaves_a_resumable_run() {
    let t = setup();
    t.engine.inject_crash(CrashPoint::BeforeStartAdvance);
    assert!(
        t.engine
            .start_run(&example(), &t.root, Some(&t.registry), &PlanOptions::default(), Some("crash"), Some(FROZEN))
            .is_err()
    );
    let run_dir = t.root.join("crash");
    assert_eq!(t.state(&run_dir, "plan"), StageState::Pending);
    t.engine.resume(&run_dir, Some(FROZEN)).expect("resume");
    assert_eq!(t.state(&run_dir, "plan"), StageState::Running);
    assert!(run_dir.join("handoff/plan.md").is_file());
}

#[test]
fn approver_must_be_named() {
    let t = setup();
    let run_dir = t.start("r1");
    assert!(matches!(t.engine.approve(&run_dir, "plan", "  ", Some(FROZEN)), Err(RunError::Run(_))));
}

#[test]
fn status_json_shape_preserves_version_one_fields() {
    let t = setup();
    let run_dir = t.start("r1");
    let value: Value = serde_json::to_value(t.engine.status(&run_dir).expect("status")).expect("json");
    let mut keys: Vec<&str> = value.as_object().expect("object").keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "awaiting_approval",
            "complete",
            "drift",
            "escalations",
            "mode",
            "reduced_independence",
            "run_dir",
            "run_id",
            "stages",
            "warnings",
            "workflow"
        ]
    );
}
