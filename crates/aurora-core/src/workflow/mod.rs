//! Role-based workflow definitions (schema v1): model and strict parser.

pub mod model;
pub mod parser;

pub use model::{
    AcceptanceCriterion, FallbackRule, Role, RoleRouting, RoutingPolicy, Stage, StageState, WorkflowDefinition,
    WorkflowValidationError,
};
pub use parser::{parse_workflow_bytes, parse_workflow_str};
