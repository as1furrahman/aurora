//! Evidence-gated stage engine for manual-handoff runs (docs/architecture/workflow-schema.md
//! §4–5). A stage never passes
//! on an exit code: only declared outputs plus explicitly claimed, evidenced criteria (and
//! approval, when required) move it to PASSED.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use aurora_core::registry::Registry;
use aurora_core::util::{format_utc, list_repr, sha256_hex};
use aurora_core::workflow::handoff::{UpstreamNote, render_handoff_with_notes};
use aurora_core::workflow::model::NOT_APPLICABLE_FILE;
use aurora_core::workflow::record::{
    Approval, GateOutcome, Handoff, RUN_KIND, RUN_SCHEMA_VERSION, RunRecord, StageRecord, WORKFLOW_FILE, Waiver,
    WorkflowRef, parse_verdict,
};
use aurora_core::workflow::{PlanOptions, StageState, WorkflowDefinition, build_plan, parse_workflow_bytes};
use serde::Serialize;
use serde_json::json;

use crate::runs::{EVIDENCE_MAX_BYTES, RECORD_MAX_BYTES, RunError, RunStore};

/// Test seam for deterministic crash-path coverage. Not part of the stable API.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    /// Fail the next `submit` after handoffs are issued but before `run.json` is saved.
    BeforeRecordSave,
    /// Fail `start_run` after the initial record is saved but before any handoff is issued.
    BeforeStartAdvance,
    /// Fail `start_run` right after the run directory is created (before any record exists).
    AfterRunDirCreated,
}

#[derive(Debug)]
pub struct Engine {
    pub store: RunStore,
    crash: Cell<Option<CrashPoint>>,
}

#[derive(Debug)]
pub struct Started {
    pub run_dir: PathBuf,
    pub record: RunRecord,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubmitResult {
    pub stage: String,
    pub state: StageState,
    pub attempts: u32,
    pub missing: Vec<String>,
    pub awaiting_approval: bool,
    pub escalated: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApproveResult {
    pub stage: String,
    pub state: StageState,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub run_id: String,
    pub run_dir: String,
    pub workflow: WorkflowRef,
    pub mode: String,
    pub complete: bool,
    pub stages: Vec<StageStatus>,
    pub escalations: Vec<String>,
    pub awaiting_approval: Vec<String>,
    pub drift: Vec<Drift>,
    pub reduced_independence: Vec<String>,
    /// Stages that passed with concerns (omitted when empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub concerns: Vec<String>,
    /// Stages accepted by recorded waiver (omitted when empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub waived: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StageStatus {
    pub id: String,
    pub role: String,
    pub provider: String,
    pub state: StageState,
    pub attempts: u32,
    pub max_attempts: u32,
    pub optional: bool,
    pub awaiting_approval: bool,
    pub escalated: bool,
    pub missing: Vec<String>,
    pub blocked_reasons: Vec<String>,
    pub waiting_on: Vec<String>,
    pub handoff: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Drift {
    pub stage: String,
    pub path: String,
    pub problem: String,
}

fn is_done(state: StageState) -> bool {
    matches!(state, StageState::Passed | StageState::NotApplicable)
}

fn now_or(now: Option<&str>) -> String {
    now.map(str::to_string).unwrap_or_else(|| {
        let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
        format_utc(secs)
    })
}

/// Human-readable representation of drift entries used in error messages.
fn drift_repr(drift: &[Drift]) -> String {
    let items: Vec<String> = drift
        .iter()
        .map(|d| format!("{{'stage': '{}', 'path': '{}', 'problem': '{}'}}", d.stage, d.path, d.problem))
        .collect();
    format!("[{}]", items.join(", "))
}

impl Engine {
    pub fn host() -> Self {
        Engine::with_store(RunStore::host())
    }

    pub fn with_store(store: RunStore) -> Self {
        Engine { store, crash: Cell::new(None) }
    }

    #[doc(hidden)]
    pub fn inject_crash(&self, point: CrashPoint) {
        self.crash.set(Some(point));
    }

    fn crash_at(&self, point: CrashPoint) -> Result<(), RunError> {
        if self.crash.get() == Some(point) {
            self.crash.set(None);
            return Err(RunError::Run(format!("injected crash ({point:?})")));
        }
        Ok(())
    }

    /// Validate, plan, create the run directory, pin the definition, and issue ready handoffs.
    pub fn start_run(
        &self,
        definition: &Path,
        run_root: &Path,
        registry: Option<&Registry>,
        opts: &PlanOptions,
        run_id: Option<&str>,
        now: Option<&str>,
    ) -> Result<Started, RunError> {
        let now = now_or(now);
        let raw = std::fs::read(definition)
            .map_err(|e| RunError::Run(format!("Cannot read workflow definition '{}': {e}", definition.display())))?;
        // Validate, plan and pin the same bytes, so the pinned copy always matches the hash.
        let wf = parse_workflow_bytes(&raw, registry, &definition.display().to_string())?;
        let plan = build_plan(&wf, registry, opts)?;
        let run_id = match run_id {
            Some(id) => id.to_string(),
            None => format!("{}-{}", wf.id, now.replace(['-', ':'], "")),
        };

        let run_dir = self.store.create_run_dir(run_root, &run_id)?;
        // Until the first record is saved the directory is not a run; on failure remove what we
        // created (only our own pinned file, then the directory if it is empty) so the id is reusable.
        let pinned = match self
            .crash_at(CrashPoint::AfterRunDirCreated)
            .and_then(|()| self.store.write_file(&run_dir, WORKFLOW_FILE, &raw))
            .and_then(|()| self.load_pinned(&run_dir, &wf.sha256))
        {
            Ok(pinned) => pinned,
            Err(e) => {
                let _ = std::fs::remove_file(run_dir.join(WORKFLOW_FILE));
                let _ = std::fs::remove_dir(&run_dir);
                return Err(e);
            }
        };

        let mut record = RunRecord {
            schema_version: RUN_SCHEMA_VERSION,
            kind: RUN_KIND.to_string(),
            run_id: run_id.clone(),
            created_at: now.clone(),
            updated_at: now.clone(),
            workflow: WorkflowRef {
                id: wf.id.clone(),
                version: wf.version.clone(),
                sha256: wf.sha256.clone(),
                path: WORKFLOW_FILE.to_string(),
            },
            stages: plan.stages.iter().map(|s| (s.id.clone(), StageRecord::new(&s.provider))).collect(),
            events: Vec::new(),
            gates: BTreeMap::new(),
            plan,
        };
        let created = json!({
            "mode": record.plan.mode,
            "provider_override": opts.provider_override.filter(|p| !p.is_empty()),
            "routes": record.plan.routes,
        });
        record.add_event(&now, None, "run-created", created);
        let blocked: Vec<(String, Vec<String>)> = record
            .plan
            .stages
            .iter()
            .filter(|s| s.is_blocked())
            .map(|s| (s.id.clone(), s.blocked_reasons.clone()))
            .collect();
        for (sid, reasons) in blocked {
            if let Some(st) = record.stages.get_mut(&sid) {
                st.state = StageState::Blocked;
                st.blocked_reasons = reasons.clone();
            }
            record.add_event(&now, Some(&sid), "blocked-by-plan", json!({"reasons": reasons}));
        }

        let _lock = self.store.lock(&run_dir)?;
        // Persist the record before issuing any handoff: an interruption leaves a resumable run.
        self.store.save_record(&run_dir, &record)?;
        self.crash_at(CrashPoint::BeforeStartAdvance)?;
        let warnings = self.advance(&pinned, &mut record, &run_dir, &now)?;
        self.store.save_record(&run_dir, &record)?;
        Ok(Started { run_dir, record, warnings })
    }

    fn load_pinned(&self, run_dir: &Path, expected_sha: &str) -> Result<WorkflowDefinition, RunError> {
        let info = self.store.read_file(run_dir, WORKFLOW_FILE, RECORD_MAX_BYTES)?;
        if info.sha256 != expected_sha {
            return Err(RunError::Integrity(format!(
                "{WORKFLOW_FILE} in '{}' no longer matches the pinned sha256 ({} != {}); refusing to continue",
                run_dir.display(),
                &info.sha256[..12],
                expected_sha.chars().take(12).collect::<String>()
            )));
        }
        Ok(parse_workflow_bytes(&info.content, None, &run_dir.join(WORKFLOW_FILE).display().to_string())?)
    }

    fn open(&self, run_dir: &Path) -> Result<(PathBuf, RunRecord, WorkflowDefinition), RunError> {
        let run_dir = run_dir.canonicalize().unwrap_or_else(|_| run_dir.to_path_buf());
        let record = self.store.load_record(&run_dir)?;
        let wf = self.load_pinned(&run_dir, &record.workflow.sha256)?;
        Ok((run_dir, record, wf))
    }

    /// Validate that `run_dir` holds a run record before taking its lock, so arbitrary
    /// directories are never locked.
    fn require_run_dir(&self, run_dir: &Path) -> Result<PathBuf, RunError> {
        let run_dir = run_dir.canonicalize().unwrap_or_else(|_| run_dir.to_path_buf());
        self.store.load_record(&run_dir)?;
        Ok(run_dir)
    }

    /// Promote PENDING stages whose dependencies are done; issue (or re-issue) their handoffs.
    fn advance(
        &self,
        wf: &WorkflowDefinition,
        record: &mut RunRecord,
        run_dir: &Path,
        now: &str,
    ) -> Result<Vec<String>, RunError> {
        let mut warnings = Vec::new();
        for sid in record.plan.order.clone() {
            let Some(stage) = wf.stage(&sid) else { continue };
            let deps_done = stage.depends_on.iter().all(|d| record.stages.get(d).is_some_and(|s| is_done(s.state)));
            let Some(state) = record.stages.get(&sid).map(|s| s.state) else { continue };
            if state == StageState::Pending && deps_done {
                if let Some(st) = record.stages.get_mut(&sid) {
                    st.state = StageState::Ready;
                }
                record.add_event(now, Some(&sid), "ready", json!({}));
            }
            let state = record.stages.get(&sid).map_or(state, |s| s.state);
            if matches!(state, StageState::Ready | StageState::Running | StageState::Failed) {
                if let Some(warning) = self.issue_handoff(wf, record, run_dir, &sid, now)? {
                    warnings.push(warning);
                }
                if let Some(st) = record.stages.get_mut(&sid)
                    && st.state == StageState::Ready
                {
                    st.state = StageState::Running;
                }
            }
        }
        Ok(warnings)
    }

    /// Write the bundle if absent. Never overwrite a bundle whose content differs.
    fn issue_handoff(
        &self,
        wf: &WorkflowDefinition,
        record: &mut RunRecord,
        run_dir: &Path,
        sid: &str,
        now: &str,
    ) -> Result<Option<String>, RunError> {
        let Some(stage_plan) = record.plan.stage(sid).cloned() else { return Ok(None) };
        let notes = upstream_notes(wf, record, sid);
        let content = render_handoff_with_notes(wf, &stage_plan, run_dir, &record.run_id, &notes);
        let expected = sha256_hex(content.as_bytes());
        let rel = stage_plan.handoff_path.clone();
        self.store.ensure_dir(run_dir, "handoff")?;
        self.store.ensure_dir(run_dir, &format!("artifacts/{sid}"))?;

        let fresh = |preserved: bool, on_disk: &str| Handoff {
            path: rel.clone(),
            sha256: expected.clone(),
            preserved_existing: preserved,
            on_disk_sha256: Some(on_disk.to_string()),
        };
        let had_handoff = record.stages.get(sid).and_then(|s| s.handoff.clone());

        if run_dir.join(&rel).symlink_metadata().is_ok() {
            let existing = match self.store.read_file(run_dir, &rel, RECORD_MAX_BYTES) {
                Ok(info) => info.sha256,
                Err(e) => format!("unreadable: {e}"),
            };
            if existing != expected {
                let warning =
                    format!("handoff bundle for stage '{sid}' differs from the generated one; not overwritten");
                if !had_handoff.as_ref().is_some_and(|h| h.preserved_existing) {
                    record.add_event(
                        now,
                        Some(sid),
                        "handoff-preserved",
                        json!({"path": rel, "on_disk_sha256": existing}),
                    );
                }
                if let Some(st) = record.stages.get_mut(sid) {
                    st.handoff = Some(fresh(true, &existing));
                }
                return Ok(Some(warning));
            }
            if had_handoff.is_none() {
                record.add_event(now, Some(sid), "handoff-issued", json!({"path": rel, "sha256": expected}));
            }
            if let Some(st) = record.stages.get_mut(sid) {
                st.handoff = Some(fresh(false, &expected));
            }
            return Ok(None);
        }

        self.store.write_file(run_dir, &rel, content.as_bytes())?;
        let event = if had_handoff.is_some() { "handoff-reissued" } else { "handoff-issued" };
        record.add_event(now, Some(sid), event, json!({"path": rel, "sha256": expected}));
        if let Some(st) = record.stages.get_mut(sid) {
            st.handoff = Some(fresh(false, &expected));
        }
        Ok(None)
    }

    /// `(sha256, None)` for valid evidence, else `(None, problem)`.
    fn inspect_artifact(&self, run_dir: &Path, rel: &str) -> (Option<String>, Option<String>) {
        let meta = match run_dir.join(rel).symlink_metadata() {
            Ok(meta) => meta,
            Err(_) => return (None, Some(format!("output missing: {rel}"))),
        };
        if !meta.file_type().is_file() {
            return (None, Some(format!("output not a regular file (symlink or special file): {rel}")));
        }
        match self.store.read_file(run_dir, rel, EVIDENCE_MAX_BYTES) {
            Err(e) => (None, Some(format!("output unreadable: {rel} ({e})"))),
            // Size is taken from the bytes actually hashed, not from the earlier lstat.
            Ok(info) if info.size == 0 => (None, Some(format!("output empty: {rel}"))),
            Ok(info) => (Some(info.sha256), None),
        }
    }

    /// Ingest evidence for one stage and evaluate its acceptance gate.
    pub fn submit(
        &self,
        run_dir: &Path,
        stage_id: &str,
        met: &[String],
        not_applicable: bool,
        now: Option<&str>,
    ) -> Result<SubmitResult, RunError> {
        let now = now_or(now);
        let run_dir = self.require_run_dir(run_dir)?;
        let _lock = self.store.lock(&run_dir)?;
        let (run_dir, mut record, wf) = self.open(&run_dir)?;
        if !record.stages.contains_key(stage_id) {
            return Err(RunError::Run(format!(
                "Unknown stage '{stage_id}'. Stages: {}",
                list_repr(&record.plan.order)
            )));
        }
        let Some(stage) = wf.stage(stage_id).cloned() else {
            return Err(RunError::Run(format!("Unknown stage '{stage_id}'")));
        };
        let state = record.stages[stage_id].state;
        if !matches!(state, StageState::Running | StageState::Failed) {
            return Err(RunError::Run(format!(
                "Stage '{stage_id}' is {state}; submissions are accepted only for RUNNING or FAILED stages"
            )));
        }
        let mut known: Vec<&str> = stage.acceptance.iter().map(|c| c.id.as_str()).collect();
        known.sort_unstable();
        let mut unknown: Vec<&str> = met.iter().map(String::as_str).filter(|m| !known.contains(m)).collect();
        unknown.sort_unstable();
        unknown.dedup();
        if !unknown.is_empty() {
            return Err(RunError::Run(format!(
                "Unknown criteria for stage '{stage_id}': {}. Known: {}",
                list_repr(&unknown),
                list_repr(&known)
            )));
        }

        if not_applicable {
            if !stage.optional {
                return Err(RunError::Run(format!(
                    "Stage '{stage_id}' is not optional and cannot be marked NOT_APPLICABLE"
                )));
            }
            let rel = format!("artifacts/{stage_id}/{NOT_APPLICABLE_FILE}");
            let (sha, problem) = self.inspect_artifact(&run_dir, &rel);
            if let Some(problem) = problem {
                return Err(RunError::Run(format!(
                    "--not-applicable requires a justification: {}",
                    problem.replacen("output", "file", 1)
                )));
            }
            let sha = sha.unwrap_or_default();
            if let Some(st) = record.stages.get_mut(stage_id) {
                st.state = StageState::NotApplicable;
                st.evidence = BTreeMap::from([(rel.clone(), sha.clone())]);
                st.missing = Vec::new();
                st.not_applicable_reason = Some(rel.clone());
            }
            record.add_event(&now, Some(stage_id), "not-applicable", json!({"justification": rel, "sha256": sha}));
        } else {
            let mut evidence = BTreeMap::new();
            let mut missing = Vec::new();
            for out in &stage.outputs {
                let rel = format!("artifacts/{stage_id}/{out}");
                match self.inspect_artifact(&run_dir, &rel) {
                    (_, Some(problem)) => missing.push(problem),
                    (Some(sha), None) => {
                        evidence.insert(rel, sha);
                    }
                    (None, None) => missing.push(format!("output unreadable: {rel}")),
                }
            }
            for crit in &stage.acceptance {
                if !met.contains(&crit.id) {
                    missing.push(format!("criterion '{}' not claimed (use --met {})", crit.id, crit.id));
                    continue;
                }
                let absent: Vec<&str> = crit
                    .evidence
                    .iter()
                    .map(String::as_str)
                    .filter(|e| !evidence.contains_key(&format!("artifacts/{stage_id}/{e}")))
                    .collect();
                if !absent.is_empty() {
                    missing.push(format!("criterion '{}' lacks evidence: {}", crit.id, list_repr(&absent)));
                }
            }

            // Structured verdict (Step 6): `fail` fails the gate; `pass`/`concerns` are recorded.
            let mut gate: Option<GateOutcome> = None;
            if let Some(name) = &stage.verdict {
                let rel = format!("artifacts/{stage_id}/{name}");
                if evidence.contains_key(&rel) {
                    let parsed = self
                        .store
                        .read_file(&run_dir, &rel, EVIDENCE_MAX_BYTES)
                        .map_err(|e| e.to_string())
                        .and_then(|info| parse_verdict(&info.content));
                    match parsed {
                        Err(reason) => missing.push(format!("verdict invalid: {rel} ({reason})")),
                        Ok((outcome, findings)) if outcome == "fail" => {
                            missing.push(format!("verdict outcome is fail ({} finding(s)): {rel}", findings.len()));
                        }
                        Ok((outcome, findings)) => gate = Some(GateOutcome { outcome, findings, waiver: None }),
                    }
                }
            }

            let Some(st) = record.stages.get_mut(stage_id) else { return Err(RunError::Run("stage vanished".into())) };
            // Resubmitting while awaiting approval re-evaluates the evidence (e.g. after drift).
            st.awaiting_approval = false;
            st.attempts += 1;
            st.missing = missing.clone();
            let attempt = st.attempts;
            let (event, detail) = if missing.is_empty() {
                st.evidence = evidence.clone();
                st.criteria_met = stage.acceptance.iter().map(|c| c.id.clone()).collect();
                if stage.requires_approval {
                    st.state = StageState::Running;
                    st.awaiting_approval = true;
                    ("gate-satisfied-awaiting-approval", json!({"attempt": attempt, "evidence": evidence}))
                } else {
                    st.state = StageState::Passed;
                    ("passed", json!({"attempt": attempt, "evidence": evidence}))
                }
            } else if attempt >= stage.max_attempts {
                st.evidence.clear();
                st.criteria_met.clear();
                st.state = StageState::Blocked;
                st.escalated = stage.on_exhausted == "escalate";
                st.blocked_reasons = vec![format!("max_attempts ({}) exhausted", stage.max_attempts)];
                (if st.escalated { "escalated" } else { "blocked" }, json!({"attempt": attempt, "missing": missing}))
            } else {
                st.evidence.clear();
                st.criteria_met.clear();
                st.state = StageState::Failed;
                ("failed", json!({"attempt": attempt, "missing": missing}))
            };
            record.add_event(&now, Some(stage_id), event, detail);
            match gate.filter(|_| missing.is_empty()) {
                Some(gate) => {
                    record.gates.insert(stage_id.to_string(), gate);
                }
                None => {
                    record.gates.remove(stage_id);
                }
            }
        }

        let warnings = self.advance(&wf, &mut record, &run_dir, &now)?;
        self.crash_at(CrashPoint::BeforeRecordSave)?;
        self.store.save_record(&run_dir, &record)?;
        let st = &record.stages[stage_id];
        Ok(SubmitResult {
            stage: stage_id.to_string(),
            state: st.state,
            attempts: st.attempts,
            missing: st.missing.clone(),
            awaiting_approval: st.awaiting_approval,
            escalated: st.escalated,
            warnings,
        })
    }

    fn drift(&self, run_dir: &Path, st: &StageRecord, stage_id: &str) -> Vec<Drift> {
        st.evidence
            .iter()
            .filter_map(|(rel, sha)| {
                let problem = match self.inspect_artifact(run_dir, rel) {
                    (_, Some(_)) => "missing",
                    (Some(current), None) if &current != sha => "hash mismatch",
                    _ => return None,
                };
                Some(Drift { stage: stage_id.to_string(), path: rel.clone(), problem: problem.to_string() })
            })
            .collect()
    }

    /// Record human approval for a stage whose gate is satisfied; evidence must be unchanged.
    pub fn approve(
        &self,
        run_dir: &Path,
        stage_id: &str,
        by: &str,
        now: Option<&str>,
    ) -> Result<ApproveResult, RunError> {
        let now = now_or(now);
        if by.trim().is_empty() {
            return Err(RunError::Run("--by must name the approver".into()));
        }
        let run_dir = self.require_run_dir(run_dir)?;
        let _lock = self.store.lock(&run_dir)?;
        let (run_dir, mut record, wf) = self.open(&run_dir)?;
        let Some(st) = record.stages.get(stage_id).cloned() else {
            return Err(RunError::Run(format!("Unknown stage '{stage_id}'")));
        };
        if !st.awaiting_approval || st.state != StageState::Running {
            return Err(RunError::Run(format!("Stage '{stage_id}' is not awaiting approval (state {})", st.state)));
        }
        let drift = self.drift(&run_dir, &st, stage_id);
        if !drift.is_empty() {
            return Err(RunError::Integrity(format!(
                "Evidence for stage '{stage_id}' changed since submission: {}; resubmission is required before approval",
                drift_repr(&drift)
            )));
        }
        if let Some(st) = record.stages.get_mut(stage_id) {
            st.awaiting_approval = false;
            st.state = StageState::Passed;
            st.approval = Some(Approval { by: by.to_string(), at: now.clone() });
        }
        record.add_event(&now, Some(stage_id), "approved", json!({"by": by}));
        let warnings = self.advance(&wf, &mut record, &run_dir, &now)?;
        self.store.save_record(&run_dir, &record)?;
        Ok(ApproveResult { stage: stage_id.to_string(), state: record.stages[stage_id].state, warnings })
    }

    /// Accept a FAILED or BLOCKED stage by recorded human decision (gate outcome `waived`).
    /// The reason and approver are stored and shown to every downstream role.
    pub fn waive(
        &self,
        run_dir: &Path,
        stage_id: &str,
        reason: &str,
        by: &str,
        now: Option<&str>,
    ) -> Result<ApproveResult, RunError> {
        let now = now_or(now);
        if reason.trim().is_empty() {
            return Err(RunError::Run("--reason must explain why the gate is waived".into()));
        }
        if by.trim().is_empty() {
            return Err(RunError::Run("--by must name the person waiving the gate".into()));
        }
        let run_dir = self.require_run_dir(run_dir)?;
        let _lock = self.store.lock(&run_dir)?;
        let (run_dir, mut record, wf) = self.open(&run_dir)?;
        let Some(st) = record.stages.get_mut(stage_id) else {
            return Err(RunError::Run(format!("Unknown stage '{stage_id}'")));
        };
        if !matches!(st.state, StageState::Failed | StageState::Blocked) {
            return Err(RunError::Run(format!(
                "Stage '{stage_id}' is {}; only FAILED or BLOCKED stages can be waived",
                st.state
            )));
        }
        let pending: Vec<&str> = wf
            .stage(stage_id)
            .map(|s| s.depends_on.iter().map(String::as_str).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .filter(|d| !record.stages.get(*d).is_some_and(|s| is_done(s.state)))
            .collect();
        if !pending.is_empty() {
            return Err(RunError::Run(format!(
                "Stage '{stage_id}' cannot be waived before its dependencies are done: {}",
                list_repr(&pending)
            )));
        }
        let Some(st) = record.stages.get_mut(stage_id) else {
            return Err(RunError::Run(format!("Unknown stage '{stage_id}'")));
        };
        st.evidence.clear();
        st.criteria_met.clear();
        st.state = StageState::Passed;
        st.escalated = false;
        st.awaiting_approval = false;
        let waiver = Waiver { by: by.to_string(), at: now.clone(), reason: reason.to_string() };
        record.gates.insert(
            stage_id.to_string(),
            GateOutcome { outcome: "waived".into(), findings: Vec::new(), waiver: Some(waiver) },
        );
        record.add_event(&now, Some(stage_id), "waived", json!({"by": by, "reason": reason}));
        let warnings = self.advance(&wf, &mut record, &run_dir, &now)?;
        self.store.save_record(&run_dir, &record)?;
        Ok(ApproveResult { stage: stage_id.to_string(), state: record.stages[stage_id].state, warnings })
    }

    /// Re-verify integrity and re-issue missing handoffs for unfinished stages. Never touches
    /// PASSED stages.
    pub fn resume(&self, run_dir: &Path, now: Option<&str>) -> Result<StatusReport, RunError> {
        let now = now_or(now);
        let run_dir = self.require_run_dir(run_dir)?;
        let warnings = {
            let _lock = self.store.lock(&run_dir)?;
            let (run_dir, mut record, wf) = self.open(&run_dir)?;
            let warnings = self.advance(&wf, &mut record, &run_dir, &now)?;
            self.store.save_record(&run_dir, &record)?;
            warnings
        };
        let mut report = self.status(&run_dir)?;
        report.warnings = warnings.into_iter().chain(report.warnings).collect();
        Ok(report)
    }

    /// Read-only run summary with completion, waiting/escalation info, and evidence drift.
    pub fn status(&self, run_dir: &Path) -> Result<StatusReport, RunError> {
        let (run_dir, record, wf) = self.open(run_dir)?;
        let mut stages = Vec::new();
        let mut drift = Vec::new();
        for sid in &record.plan.order {
            let (Some(st), Some(stage)) = (record.stages.get(sid), wf.stage(sid)) else { continue };
            if is_done(st.state) || st.awaiting_approval {
                drift.extend(self.drift(&run_dir, st, sid));
            }
            let waiting_on = if st.state == StageState::Pending {
                stage
                    .depends_on
                    .iter()
                    .filter(|d| !record.stages.get(*d).is_some_and(|s| is_done(s.state)))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            stages.push(StageStatus {
                id: sid.clone(),
                role: stage.role.clone(),
                provider: st.provider.clone(),
                state: st.state,
                attempts: st.attempts,
                max_attempts: stage.max_attempts,
                optional: stage.optional,
                awaiting_approval: st.awaiting_approval,
                escalated: st.escalated,
                missing: st.missing.clone(),
                blocked_reasons: st.blocked_reasons.clone(),
                waiting_on,
                handoff: st.handoff.as_ref().map(|h| h.path.clone()),
            });
        }
        let complete = stages
            .iter()
            .all(|s| s.state == StageState::Passed || (s.optional && s.state == StageState::NotApplicable))
            && drift.is_empty();
        let mut reduced: Vec<String> = record
            .plan
            .stages
            .iter()
            .filter(|s| s.independence.iter().any(|e| e.status == "same-provider"))
            .map(|s| s.id.clone())
            .collect();
        reduced.sort();
        reduced.dedup();
        Ok(StatusReport {
            run_id: record.run_id.clone(),
            run_dir: run_dir.display().to_string(),
            workflow: record.workflow.clone(),
            mode: record.plan.mode.clone(),
            complete,
            escalations: stages.iter().filter(|s| s.escalated).map(|s| s.id.clone()).collect(),
            awaiting_approval: stages.iter().filter(|s| s.awaiting_approval).map(|s| s.id.clone()).collect(),
            stages,
            drift,
            reduced_independence: reduced,
            concerns: gate_stages(&record, "concerns"),
            waived: gate_stages(&record, "waived"),
            warnings: record.plan.warnings.clone(),
        })
    }
}

/// Stage ids (in plan order) whose recorded gate outcome is `outcome`.
fn gate_stages(record: &RunRecord, outcome: &str) -> Vec<String> {
    record.plan.order.iter().filter(|id| record.gates.get(*id).is_some_and(|g| g.outcome == outcome)).cloned().collect()
}

/// Concerns and waivers from every transitive dependency of `sid`, in plan order.
fn upstream_notes(wf: &WorkflowDefinition, record: &RunRecord, sid: &str) -> Vec<UpstreamNote> {
    let mut ancestors = std::collections::HashSet::new();
    let mut stack: Vec<&str> =
        wf.stage(sid).map(|s| s.depends_on.iter().map(String::as_str).collect()).unwrap_or_default();
    while let Some(dep) = stack.pop() {
        if ancestors.insert(dep.to_string())
            && let Some(stage) = wf.stage(dep)
        {
            stack.extend(stage.depends_on.iter().map(String::as_str));
        }
    }
    let mut notes = Vec::new();
    for id in record.plan.order.iter().filter(|id| ancestors.contains(*id)) {
        let Some(gate) = record.gates.get(id) else { continue };
        match (&gate.waiver, gate.outcome.as_str()) {
            (Some(w), _) => {
                notes.push(UpstreamNote::Waived { stage: id.clone(), by: w.by.clone(), reason: w.reason.clone() })
            }
            (None, "concerns") => notes.extend(gate.findings.iter().map(|f| UpstreamNote::Concern {
                stage: id.clone(),
                severity: f.severity.clone(),
                summary: f.summary.clone(),
            })),
            _ => {}
        }
    }
    notes
}
