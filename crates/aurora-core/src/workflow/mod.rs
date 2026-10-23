//! Role-based workflow definitions (schema v1): model, strict parser and deterministic planner.

pub mod describe;
pub mod model;
pub mod parser;
pub mod planner;

pub use describe::{Description, describe};
pub use model::{
    AcceptanceCriterion, FallbackRule, Role, RoleRouting, RoutingPolicy, Stage, StageState, WorkflowDefinition,
    WorkflowValidationError,
};
pub use parser::{parse_workflow_bytes, parse_workflow_str};
pub use planner::{Detection, Plan, PlanError, PlanOptions, PlannedStage, build_plan};
