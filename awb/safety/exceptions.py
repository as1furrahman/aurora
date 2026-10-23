"""
Custom exceptions for awb.safety security controls.
"""


class SafetyError(Exception):
    """Base exception for all safety violations."""
    pass


class PathTraversalError(SafetyError):
    """Raised when a path escapes permitted root boundaries."""
    pass


class SymlinkAncestorError(SafetyError):
    """Raised when an ancestor directory is an unauthorized symlink."""
    pass


class FIFOSubstitutionError(SafetyError):
    """Raised when a special device, FIFO, or non-regular file is encountered."""
    pass


class StateDriftError(SafetyError):
    """Raised when file identity (inode, device, UID, hash) drifts unexpectedly."""
    pass


class DescriptorValidationError(SafetyError):
    """Raised when opened descriptor attributes fail security assertions."""
    pass


class LockError(SafetyError):
    """Base exception for cross-process locking violations."""
    pass


class LockTimeoutError(LockError):
    """Raised when acquiring a process lock times out."""
    pass


class SecurityPreconditionError(SafetyError):
    """Raised when filesystem security preconditions (permissions, ownership) are violated."""
    pass


class GlobalMutationBlockedError(SafetyError):
    """Raised when mutation of global agent directories is attempted without authorization."""
    pass
