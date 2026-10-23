//! Workflow definition model (schema v1). Roles are responsibilities; providers live only
//! in [`RoutingPolicy`] (docs/architecture/workflow-schema.md). Port of `awb/workflow/model.py`.

use std::fmt;

pub const SCHEMA_VERSION: i64 = 1;
pub const ASSURANCE_LEVELS: [&str; 3] = ["basic", "standard", "high"];
pub const ON_EXHAUSTED: [&str; 2] = ["escalate", "block"];
pub const FALLBACK_TRIGGERS: [&str; 2] = ["incompatible", "unavailable"];
pub const NOT_APPLICABLE_FILE: &str = "NOT_APPLICABLE.md";

/// Raised when a definition violates schema v1. Carries every error found.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Workflow '{source_name}' is invalid:\n  - {}", .errors.join("\n  - "))]
pub struct WorkflowValidationError {
    pub errors: Vec<String>,
    pub source_name: String,
}

/// Stage lifecycle states. `SKIPPED` is reserved; the v1 engine never produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StageState {
    Pending,
    Ready,
    Running,
    Passed,
    Failed,
    Blocked,
    Skipped,
    NotApplicable,
}

impl StageState {
    pub const ALL: [StageState; 8] = [
        StageState::Pending,
        StageState::Ready,
        StageState::Running,
        StageState::Passed,
        StageState::Failed,
        StageState::Blocked,
        StageState::Skipped,
        StageState::NotApplicable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            StageState::Pending => "PENDING",
            StageState::Ready => "READY",
            StageState::Running => "RUNNING",
            StageState::Passed => "PASSED",
            StageState::Failed => "FAILED",
            StageState::Blocked => "BLOCKED",
            StageState::Skipped => "SKIPPED",
            StageState::NotApplicable => "NOT_APPLICABLE",
        }
    }

    pub fn parse(s: &str) -> Option<StageState> {
        StageState::ALL.into_iter().find(|state| state.as_str() == s)
    }
}

impl fmt::Display for StageState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub id: String,
    pub responsibility: String,
    pub required_skills: Vec<String>,
    pub recommended_skills: Vec<String>,
    /// Capabilities the role needs enforced (e.g. `read_only` for an independent reviewer).
    pub requires: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stage {
    pub id: String,
    pub role: String,
    pub outputs: Vec<String>,
    pub acceptance: Vec<AcceptanceCriterion>,
    pub description: String,
    pub depends_on: Vec<String>,
    pub inputs: Vec<String>,
    pub optional: bool,
    pub max_attempts: u32,
    pub on_exhausted: String,
    pub requires_approval: bool,
    pub independent_of: Vec<String>,
    /// Optional structured verdict output (`<name>.json`): pass | concerns | fail + findings.
    pub verdict: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleRouting {
    pub role: String,
    pub eligible: Vec<String>,
    pub preferred: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackRule {
    pub role: String,
    pub from_provider: String,
    pub to_provider: String,
    pub on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingPolicy {
    pub allow_override: bool,
    /// One entry per declared role, in role declaration order.
    pub roles: Vec<RoleRouting>,
    pub fallback: Vec<FallbackRule>,
}

impl RoutingPolicy {
    pub fn role(&self, role_id: &str) -> Option<&RoleRouting> {
        self.roles.iter().find(|r| r.role == role_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkflowDefinition {
    pub schema_version: i64,
    pub id: String,
    pub version: String,
    pub name: String,
    pub assurance: String,
    /// Roles in declaration order.
    pub roles: Vec<Role>,
    /// Stages in declaration order.
    pub stages: Vec<Stage>,
    pub routing: RoutingPolicy,
    /// Lowercase hex SHA-256 of the exact definition bytes.
    pub sha256: String,
    pub description: String,
    pub task_types: Vec<String>,
    pub warnings: Vec<String>,
}

impl WorkflowDefinition {
    pub fn stage(&self, stage_id: &str) -> Option<&Stage> {
        self.stages.iter().find(|s| s.id == stage_id)
    }

    pub fn role(&self, role_id: &str) -> Option<&Role> {
        self.roles.iter().find(|r| r.id == role_id)
    }
}
