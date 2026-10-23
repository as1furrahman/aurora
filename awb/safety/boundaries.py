"""
Safety boundary enforcement, protected directory shielding,
and permission precondition validation.
Zero third-party dependencies.
"""

import os
from pathlib import Path
from typing import List, Union

from awb.safety.exceptions import (
    GlobalMutationBlockedError,
    PathTraversalError,
    SafetyError,
    SecurityPreconditionError,
    SymlinkAncestorError,
)
from awb.safety.filesystem import SafeFS

# Protected project directories that must never be targeted, contained, or modified (SEC-04)
PROTECTED_PROJECT_PATHS = [
    Path("/home/asif/projects/token-efficient-workflow"),
    Path("/home/asif/projects/dwl"),
    Path("/home/asif/projects/dwl-native-bar-lab"),
]

# Real global agent skill directories blocked in Phase 4
GLOBAL_AGENT_SKILL_PATHS = [
    Path.home() / ".codex" / "skills",
    Path.home() / ".gemini" / "antigravity-cli" / "skills",
    Path.home() / ".gemini" / "config" / "skills",
]

# Sensitive system locations that must never be targeted
SENSITIVE_SYSTEM_PATHS = [
    Path.home() / ".ssh",
    Path.home() / ".gnupg",
    Path("/etc"),
    Path("/root"),
    Path("/boot"),
    Path("/sys"),
    Path("/proc"),
]

# Overly broad roots that cannot serve as trusted sandbox boundaries
DISALLOWED_BROAD_ROOTS = [
    Path("/"),
    Path("/home"),
]


def validate_security_preconditions(
    target_dir: Union[str, Path],
    allowed_roots: List[Union[str, Path]],
    allow_global: bool = False,
) -> Path:
    """
    Validate filesystem security preconditions before any mutation or planning.
    Enforces genuine boundary containment, rejects protected project targets,
    blocks global agent directories, and enforces safe parent ownership and permissions.
    """
    raw_path = Path(target_dir)
    resolved_target = raw_path.resolve()

    # 1. Strict rejection of .system/
    if ".system" in resolved_target.parts or ".system" in raw_path.parts:
        raise SafetyError(
            f"Target path cannot be or reside within protected '.system/': '{target_dir}'"
        )

    # 2. Strict rejection of protected project directories (SEC-04)
    for prot in PROTECTED_PROJECT_PATHS:
        canon_prot = prot.resolve()
        if (
            resolved_target == canon_prot
            or canon_prot in resolved_target.parents
            or resolved_target in canon_prot.parents
        ):
            raise SafetyError(
                f"Target path '{target_dir}' conflicts with protected project directory '{canon_prot}'. "
                "Target cannot equal, contain, or reside within protected projects."
            )

    # 3. Global directory guard (Phase 4 scope enforcement)
    if not allow_global:
        for glob_dir in GLOBAL_AGENT_SKILL_PATHS:
            canon_glob = glob_dir.resolve()
            if (
                resolved_target == canon_glob
                or canon_glob in resolved_target.parents
                or resolved_target in canon_glob.parents
            ):
                raise GlobalMutationBlockedError(
                    f"Direct mutation of global agent directory '{resolved_target}' is blocked "
                    "pending separate operational authorization. Use an isolated test directory or sandbox."
                )

    # 4. Sensitive system locations
    for sens in SENSITIVE_SYSTEM_PATHS:
        canon_sens = sens.resolve()
        if (
            resolved_target == canon_sens
            or canon_sens in resolved_target.parents
            or resolved_target in canon_sens.parents
        ):
            raise SafetyError(
                f"Target path '{target_dir}' conflicts with sensitive system location '{canon_sens}'."
            )

    # 5. Allowed roots validation (SEC-04)
    if not allowed_roots:
        raise SecurityPreconditionError(
            "No trusted sandbox root provided. Allowed roots list cannot be empty."
        )

    canon_roots: List[Path] = []
    user_home = Path.home().resolve()
    for r in allowed_roots:
        canon_r = Path(r).resolve()
        # Reject overly broad roots
        for broad in DISALLOWED_BROAD_ROOTS:
            if canon_r == broad.resolve():
                raise SafetyError(
                    f"Insecure boundary: allowed root '{canon_r}' is overly broad. "
                    "Roots must be specific sandbox or project directories."
                )
        if canon_r == user_home and not allow_global:
            raise SafetyError(
                f"Insecure boundary: user home '{canon_r}' is overly broad as an allowed root. "
                "Roots must be specific sandbox directories."
            )

        # Ensure allowed root does not conflict with protected project directories
        for prot in PROTECTED_PROJECT_PATHS:
            canon_prot = prot.resolve()
            if canon_r == canon_prot or canon_prot in canon_r.parents or canon_r in canon_prot.parents:
                raise SafetyError(
                    f"Insecure boundary: allowed root '{canon_r}' conflicts with protected project directory '{canon_prot}'."
                )
        canon_roots.append(canon_r)

    # 6. Path validation using SafeFS (checks containment, traversal, and ancestor symlinks)
    canon_target_str = SafeFS.validate_path(target_dir, canon_roots, allow_symlinks=False)
    canon_target = Path(canon_target_str)

    # 7. Parent directory ownership and permissions (SEC-07)
    curr = canon_target
    while not curr.exists() and curr != curr.parent:
        curr = curr.parent

    if curr.exists():
        st = os.stat(curr)
        current_uid = os.getuid()
        if st.st_uid != current_uid:
            raise SecurityPreconditionError(
                f"Insecure ownership: target parent '{curr}' is owned by UID {st.st_uid}, "
                f"expected current user UID {current_uid}"
            )

        # Sticky-bit policy (SEC-07):
        # In POSIX filesystems, write permission on a directory permits creating, unlinking,
        # and renaming entries within that directory regardless of file ownership.
        # - A world-writable directory (0o002) without the sticky bit (0o1000) permits any local user
        #   to unlink or substitute files.
        # - A group-writable directory (0o020) without the sticky bit (0o1000) permits any user in the
        #   same group to unlink or substitute files.
        # Therefore, directories writable by group or others MUST have the sticky bit (0o1000) set.
        is_world_writable = bool(st.st_mode & 0o002)
        is_group_writable = bool(st.st_mode & 0o020)
        has_sticky_bit = bool(st.st_mode & 0o1000)

        if (is_world_writable or is_group_writable) and not has_sticky_bit:
            perm_type = "world-writable" if is_world_writable else "group-writable"
            raise SecurityPreconditionError(
                f"Insecure permissions: target directory '{curr}' is {perm_type} "
                f"without sticky bit (mode {oct(st.st_mode)}). Parent directories must not be writable "
                "by group or others unless the sticky bit (0o1000) is set."
            )

    return canon_target
