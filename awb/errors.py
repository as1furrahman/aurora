"""
Structured error hierarchy for Agent Workbench (awb).
Zero third-party dependencies.
"""

from awb.safety.exceptions import (
    DescriptorValidationError,
    FIFOSubstitutionError,
    GlobalMutationBlockedError,
    LockError,
    LockTimeoutError,
    PathTraversalError,
    SafetyError,
    SecurityPreconditionError,
    StateDriftError,
    SymlinkAncestorError,
)


class AWBError(Exception):
    """Base exception for all Agent Workbench errors."""
    pass


class ConfigError(AWBError):
    """Raised when configuration values are missing, invalid, or inaccessible."""
    pass


class RegistryError(AWBError):
    """Base exception for registry parsing and resolution errors."""
    pass


class RegistryParseError(RegistryError):
    """Raised when registry TOML parsing fails."""
    pass


class ValidationError(RegistryError):
    """Raised when registry schema or skill definition validation fails."""
    pass


class DuplicateSkillError(ValidationError):
    """Raised when duplicate skill IDs are detected in registry."""
    pass


class DependencyError(RegistryError):
    """Raised when skill dependency resolution or cycle detection fails."""
    pass


class AdapterError(AWBError):
    """Base exception for agent adapter errors."""
    pass


class ConformanceError(AdapterError):
    """Raised when an adapter fails non-mutating conformance assertions."""
    pass


class TransactionError(AWBError):
    """Raised when a transactional plan, journal, or state transition fails."""
    pass


class OwnershipError(AWBError):
    """Raised when skill directory ownership or AWB manifest validation fails."""
    pass
