"""
awb.workflow - Role-based workflow definitions, deterministic planning, and
evidence-gated run tracking (manual-handoff mode).
"""

from .model import (
    AcceptanceCriterion,
    FallbackRule,
    Role,
    RoleRouting,
    RoutingPolicy,
    Stage,
    StageState,
    WorkflowDefinition,
    WorkflowError,
    WorkflowValidationError,
)
from .parser import load_workflow, parse_workflow_string

__all__ = [
    "AcceptanceCriterion",
    "FallbackRule",
    "Role",
    "RoleRouting",
    "RoutingPolicy",
    "Stage",
    "StageState",
    "WorkflowDefinition",
    "WorkflowError",
    "WorkflowValidationError",
    "load_workflow",
    "parse_workflow_string",
]
