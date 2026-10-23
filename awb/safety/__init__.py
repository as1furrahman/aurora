"""
awb.safety
Filesystem safety primitives and deterministic security controls.
"""

from .exceptions import (
    SafetyError,
    PathTraversalError,
    SymlinkAncestorError,
    FIFOSubstitutionError,
    StateDriftError,
    DescriptorValidationError,
    LockError,
    LockTimeoutError,
)
from .filesystem import SafeFS
from .locking import ProcessLock
from .boundaries import (
    validate_security_preconditions,
    PROTECTED_PROJECT_PATHS,
    GLOBAL_AGENT_SKILL_PATHS,
    SENSITIVE_SYSTEM_PATHS,
)

__all__ = [
    "SafetyError",
    "PathTraversalError",
    "SymlinkAncestorError",
    "FIFOSubstitutionError",
    "StateDriftError",
    "DescriptorValidationError",
    "LockError",
    "LockTimeoutError",
    "SafeFS",
    "ProcessLock",
    "validate_security_preconditions",
    "PROTECTED_PROJECT_PATHS",
    "GLOBAL_AGENT_SKILL_PATHS",
    "SENSITIVE_SYSTEM_PATHS",
]
