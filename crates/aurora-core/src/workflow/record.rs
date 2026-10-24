//! Run record (`run.json`, schema v1) — pure types and strict validation (ADR-0003).
//!
//! Compatible with existing schema-v1 run records: stable keys and semantics, written with sorted
//! keys and 2-space indentation. Unknown top-level keys are tolerated, but stage records must
//! have exactly the v1 keys and valid states.

use std::collections::{BTreeMap, BTreeSet};

use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::model::StageState;
use super::planner::Plan;

pub const RUN_SCHEMA_VERSION: i64 = 1;
pub const RUN_KIND: &str = "aurora.workflow-run";
pub const RECORD_FILE: &str = "run.json";
pub const WORKFLOW_FILE: &str = "workflow.toml";

const STAGE_KEYS: [&str; 12] = [
    "state",
    "provider",
    "attempts",
    "awaiting_approval",
    "escalated",
    "handoff",
    "evidence",
    "criteria_met",
    "missing",
    "blocked_reasons",
    "approval",
    "not_applicable_reason",
];

const RECORD_KEYS: [&str; 9] =
    ["schema_version", "kind", "run_id", "created_at", "updated_at", "workflow", "plan", "stages", "events"];

impl Serialize for StageState {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for StageState {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        StageState::parse(&raw).ok_or_else(|| serde::de::Error::custom(format!("invalid state {raw:?}")))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunRecord {
    pub schema_version: i64,
    pub kind: String,
    pub run_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub workflow: WorkflowRef,
    pub plan: Plan,
    pub stages: IndexMap<String, StageRecord>,
    pub events: Vec<Event>,
    /// Gate outcomes beyond plain pass (structured verdicts, waivers). A top-level map keeps
    /// stage records at exactly the v1 keys, preserving compatibility with older readers.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub gates: BTreeMap<String, GateOutcome>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowRef {
    pub id: String,
    pub version: String,
    pub sha256: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageRecord {
    pub state: StageState,
    pub provider: String,
    pub attempts: u32,
    pub awaiting_approval: bool,
    pub escalated: bool,
    pub handoff: Option<Handoff>,
    pub evidence: BTreeMap<String, String>,
    pub criteria_met: Vec<String>,
    pub missing: Vec<String>,
    pub blocked_reasons: Vec<String>,
    pub approval: Option<Approval>,
    pub not_applicable_reason: Option<String>,
}

impl StageRecord {
    pub fn new(provider: &str) -> Self {
        StageRecord {
            state: StageState::Pending,
            provider: provider.to_string(),
            attempts: 0,
            awaiting_approval: false,
            escalated: false,
            handoff: None,
            evidence: BTreeMap::new(),
            criteria_met: Vec::new(),
            missing: Vec::new(),
            blocked_reasons: Vec::new(),
            approval: None,
            not_applicable_reason: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handoff {
    pub path: String,
    pub sha256: String,
    #[serde(default)]
    pub preserved_existing: bool,
    #[serde(default)]
    pub on_disk_sha256: Option<String>,
}

/// Outcome of a stage gate: `pass` or `concerns` (from a structured verdict) or `waived`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateOutcome {
    pub outcome: String,
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiver: Option<Waiver>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waiver {
    pub by: String,
    pub at: String,
    pub reason: String,
}

/// Parse a structured verdict: `{"outcome": "pass"|"concerns"|"fail", "findings": [{"severity", "summary"}]}`.
pub fn parse_verdict(bytes: &[u8]) -> Result<(String, Vec<Finding>), String> {
    let value: Value = serde_json::from_slice(bytes).map_err(|e| format!("not valid JSON: {e}"))?;
    let outcome = value.get("outcome").and_then(Value::as_str).unwrap_or_default();
    if !["pass", "concerns", "fail"].contains(&outcome) {
        return Err("'outcome' must be one of 'pass', 'concerns', 'fail'".into());
    }
    let findings = value
        .get("findings")
        .and_then(Value::as_array)
        .ok_or_else(|| "'findings' must be an array".to_string())?
        .iter()
        .map(|f| match (f.get("severity").and_then(Value::as_str), f.get("summary").and_then(Value::as_str)) {
            (Some(severity), Some(summary)) => Ok(Finding { severity: severity.into(), summary: summary.into() }),
            _ => Err("each finding needs string 'severity' and 'summary'".to_string()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((outcome.to_string(), findings))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Approval {
    pub by: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub seq: u64,
    pub at: String,
    pub stage: Option<String>,
    pub event: String,
    pub detail: Value,
}

/// `run.json` is missing required parts or violates the v1 record schema.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("run.json: {0}")]
pub struct RecordError(pub String);

impl RunRecord {
    /// Parse and strictly validate `run.json` bytes.
    pub fn from_json(bytes: &[u8]) -> Result<RunRecord, RecordError> {
        let value: Value = serde_json::from_slice(bytes).map_err(|e| RecordError(format!("not valid JSON: {e}")))?;
        let Some(obj) = value.as_object() else {
            return Err(RecordError("root must be an object".into()));
        };
        if obj.get("schema_version").and_then(Value::as_i64) != Some(RUN_SCHEMA_VERSION)
            || obj.get("schema_version").is_some_and(Value::is_boolean)
        {
            return Err(RecordError(format!(
                "unsupported schema_version {}; expected {RUN_SCHEMA_VERSION}",
                obj.get("schema_version").map_or("None".to_string(), Value::to_string)
            )));
        }
        if obj.get("kind").and_then(Value::as_str) != Some(RUN_KIND) {
            return Err(RecordError(format!("kind must be '{RUN_KIND}'")));
        }
        let missing: Vec<&str> = RECORD_KEYS.iter().copied().filter(|k| !obj.contains_key(*k)).collect();
        if !missing.is_empty() {
            return Err(RecordError(format!("missing keys {missing:?}")));
        }
        // Stage records must carry exactly the v1 keys (serde alone would accept a
        // missing optional key as null).
        if let Some(stages) = obj.get("stages").and_then(Value::as_object) {
            for (sid, st) in stages {
                let keys: BTreeSet<&str> =
                    st.as_object().map(|o| o.keys().map(String::as_str).collect()).unwrap_or_default();
                if keys != STAGE_KEYS.into_iter().collect::<BTreeSet<_>>() {
                    return Err(RecordError(format!("stage '{sid}' has an invalid shape")));
                }
            }
        }
        let record: RunRecord =
            serde_json::from_value(value).map_err(|e| RecordError(format!("invalid record shape: {e}")))?;
        let is_sha = |s: &str| s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !is_sha(&record.workflow.sha256) {
            return Err(RecordError("workflow.sha256 must be 64 lowercase hex characters".into()));
        }
        let plan_ids: BTreeSet<&str> = record.plan.stages.iter().map(|s| s.id.as_str()).collect();
        let stage_ids: BTreeSet<&str> = record.stages.keys().map(String::as_str).collect();
        if plan_ids != stage_ids {
            return Err(RecordError("stage set does not match the pinned plan".into()));
        }
        let order: BTreeSet<&str> = record.plan.order.iter().map(String::as_str).collect();
        if order != stage_ids || record.plan.order.len() != stage_ids.len() {
            return Err(RecordError("plan.order must list every stage exactly once".into()));
        }
        Ok(record)
    }

    /// Canonical serialization: sorted keys, 2-space indent, trailing newline.
    pub fn to_json(&self) -> String {
        let value = serde_json::to_value(self).unwrap_or(Value::Null);
        let mut text = serde_json::to_string_pretty(&sort_keys(value)).unwrap_or_default();
        text.push('\n');
        text
    }

    pub fn stage(&self, id: &str) -> Option<&StageRecord> {
        self.stages.get(id)
    }

    pub fn add_event(&mut self, now: &str, stage: Option<&str>, event: &str, detail: Value) {
        let seq = self.events.len() as u64 + 1;
        self.events.push(Event {
            seq,
            at: now.to_string(),
            stage: stage.map(str::to_string),
            event: event.to_string(),
            detail: if detail.is_null() { Value::Object(Map::new()) } else { detail },
        });
        self.updated_at = now.to_string();
    }
}

/// Recursively rebuild every object with keys in sorted order (independent of serde_json's
/// `preserve_order` feature, which other crates in the build may enable).
pub fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: BTreeMap<String, Value> = map.into_iter().map(|(k, v)| (k, sort_keys(v))).collect();
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        other => other,
    }
}
