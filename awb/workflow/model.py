"""
Workflow definition model (schema v1). Roles are responsibilities; providers live only
in RoutingPolicy (docs/architecture/workflow-schema.md).
"""

from dataclasses import dataclass, field
from enum import Enum
from typing import Dict, Optional, Tuple

from awb.errors import AWBError

SCHEMA_VERSION = 1
ASSURANCE_LEVELS = ("basic", "standard", "high")
ON_EXHAUSTED = ("escalate", "block")
FALLBACK_TRIGGERS = ("incompatible", "unavailable")
NOT_APPLICABLE_FILE = "NOT_APPLICABLE.md"


class WorkflowError(AWBError):
    """Base exception for workflow definition, planning, and run errors."""
    pass


class WorkflowValidationError(WorkflowError):
    """Raised when a workflow definition violates schema v1. Carries every error found."""

    def __init__(self, errors, source: str = "<string>"):
        self.errors = list(errors)
        self.source = source
        super().__init__(f"Workflow '{source}' is invalid:\n  - " + "\n  - ".join(self.errors))


class StageState(str, Enum):
    """Stage lifecycle states. SKIPPED is reserved; the v1 engine never produces it."""

    PENDING = "PENDING"
    READY = "READY"
    RUNNING = "RUNNING"
    PASSED = "PASSED"
    FAILED = "FAILED"
    BLOCKED = "BLOCKED"
    SKIPPED = "SKIPPED"
    NOT_APPLICABLE = "NOT_APPLICABLE"


@dataclass(frozen=True)
class AcceptanceCriterion:
    id: str
    description: str
    evidence: Tuple[str, ...]


@dataclass(frozen=True)
class Role:
    id: str
    responsibility: str
    required_skills: Tuple[str, ...] = ()
    recommended_skills: Tuple[str, ...] = ()


@dataclass(frozen=True)
class Stage:
    id: str
    role: str
    outputs: Tuple[str, ...]
    acceptance: Tuple[AcceptanceCriterion, ...]
    description: str = ""
    depends_on: Tuple[str, ...] = ()
    inputs: Tuple[str, ...] = ()
    optional: bool = False
    max_attempts: int = 2
    on_exhausted: str = "escalate"
    requires_approval: bool = False
    independent_of: Tuple[str, ...] = ()


@dataclass(frozen=True)
class RoleRouting:
    role: str
    eligible: Tuple[str, ...]
    preferred: str


@dataclass(frozen=True)
class FallbackRule:
    role: str
    from_provider: str
    to_provider: str
    on: Tuple[str, ...]


@dataclass(frozen=True)
class RoutingPolicy:
    allow_override: bool = True
    roles: Dict[str, RoleRouting] = field(default_factory=dict)
    fallback: Tuple[FallbackRule, ...] = ()


@dataclass(frozen=True)
class WorkflowDefinition:
    schema_version: int
    id: str
    version: str
    name: str
    assurance: str
    roles: Dict[str, Role]
    stages: Tuple[Stage, ...]
    routing: RoutingPolicy
    sha256: str
    description: str = ""
    task_types: Tuple[str, ...] = ()
    warnings: Tuple[str, ...] = ()

    def stage(self, stage_id: str) -> Stage:
        for s in self.stages:
            if s.id == stage_id:
                return s
        raise KeyError(f"Unknown stage '{stage_id}' in workflow '{self.id}'")

    def find_stage(self, stage_id: str) -> Optional[Stage]:
        return next((s for s in self.stages if s.id == stage_id), None)
