//! Run record (`run.json`, schema v1) — pure types and strict validation (ADR-0003).
//!
//! Schema-compatible with the Python implementation (`awb/workflow/runs.py`): same keys and
//! semantics, written with sorted keys and 2-space indentation, so either implementation can
//! resume the other's run. Strictness mirrors Python: unknown top-level keys are tolerated,
//! but stage records must have exactly the v1 keys and valid states.

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
        let record: RunRecord =
            serde_json::from_value(value).map_err(|e| RecordError(format!("invalid record shape: {e}")))?;
        let plan_ids: BTreeSet<&str> = record.plan.stages.iter().map(|s| s.id.as_str()).collect();
        let stage_ids: BTreeSet<&str> = record.stages.keys().map(String::as_str).collect();
        if plan_ids != stage_ids {
            return Err(RecordError("stage set does not match the pinned plan".into()));
        }
        Ok(record)
    }

    /// Canonical serialization: sorted keys, 2-space indent, trailing newline (as Python writes).
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
