"""
Skill removal engine for Agent Workbench.
Enforces strict ownership verification, pre-mutation snapshots,
content-drift defense, and transactional rollback under ProcessLock.
Zero third-party runtime dependencies.
"""

import hashlib
import os
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union

from awb.config import Config
from awb.errors import (
    GlobalMutationBlockedError,
    OwnershipError,
    SafetyError,
    SecurityPreconditionError,
    StateDriftError,
    TransactionError,
)
from awb.operations.manifest import (
    MANIFEST_FILENAME,
    SkillManifest,
    verify_skill_integrity,
)
from awb.operations.snapshots import PreMutationSnapshot
from awb.safety import validate_security_preconditions
from awb.transactions import (
    MutationAction,
    MutationPlan,
    MutationStep,
    TransactionEngine,
    TransactionResult,
)


class SkillRemover:
    """Orchestrates secure, transactional skill removal."""

    @classmethod
    def check_security_preconditions(
        cls,
        target_dir: Union[str, Path],
        allowed_roots: List[Union[str, Path]],
        allow_global: bool = False,
    ) -> Path:
        """Validate safety boundaries and ownership before removal."""
        return validate_security_preconditions(
            target_dir=target_dir,
            allowed_roots=allowed_roots,
            allow_global=allow_global,
        )

    @classmethod
    def plan_remove(
        cls,
        skill_id: str,
        target_dir: Path,
        allowed_roots: List[Union[str, Path]],
        allow_global: bool = False,
    ) -> Tuple[MutationPlan, SkillManifest, PreMutationSnapshot]:
        """
        Construct a validated, deterministic MutationPlan for skill removal.
        Rejects non-AWB directories, modified files, symlinks, and extraneous files.
        """
        roots = [Path(r).resolve() for r in allowed_roots]
        canon_target = cls.check_security_preconditions(target_dir, roots, allow_global=allow_global)

        dest_skill_dir = canon_target / skill_id
        if not dest_skill_dir.exists():
            raise OwnershipError(f"Skill '{skill_id}' is not installed at '{dest_skill_dir}' (directory does not exist)")

        if not dest_skill_dir.is_dir():
            raise SafetyError(f"Target skill path '{dest_skill_dir}' is not a directory")

        if dest_skill_dir.is_symlink():
            raise SafetyError(f"Target skill directory '{dest_skill_dir}' is an unauthorized symlink")

        # Rigorous ownership and content-drift check
        is_valid, manifest, report = verify_skill_integrity(dest_skill_dir, expected_skill_id=skill_id)
        if not is_valid or manifest is None:
            if report.symlinks_detected:
                raise SafetyError(f"Cannot remove '{skill_id}': symlinks detected in directory: {report.symlinks_detected}")
            if report.modified_files:
                raise StateDriftError(
                    f"Cannot remove '{skill_id}': files have been modified since installation: "
                    f"{report.modified_files}. Aborting to prevent accidental data loss."
                )
            if report.extraneous_files:
                raise OwnershipError(
                    f"Cannot remove '{skill_id}': extraneous unmanaged files detected in directory: "
                    f"{report.extraneous_files}. Aborting to prevent accidental data loss."
                )
            raise OwnershipError(
                f"Cannot remove '{skill_id}': directory is not demonstrably owned by AWB: {'; '.join(report.errors)}"
            )

        snapshot = PreMutationSnapshot.capture(dest_skill_dir)

        # Collect files and subdirectories to remove
        files_to_unlink: List[Path] = []
        subdirs_to_remove: List[Path] = []

        for root, dirs, fnames in os.walk(dest_skill_dir, topdown=False):
            for fname in fnames:
                files_to_unlink.append(Path(root) / fname)
            for dname in dirs:
                subdirs_to_remove.append(Path(root) / dname)

        steps: List[MutationStep] = []

        # 1. Unlink files (including .awb-manifest.json)
        # Manifest is unlinked last among files
        files_sorted = sorted(files_to_unlink, key=lambda p: (p.name == MANIFEST_FILENAME, str(p)))
        for file_p in files_sorted:
            st = os.stat(file_p)
            with open(file_p, "rb") as f:
                sha = hashlib.sha256(f.read()).hexdigest()
            steps.append(
                MutationStep(
                    action=MutationAction.UNLINK_FILE,
                    target_path=file_p,
                    expected_dev=st.st_dev,
                    expected_ino=st.st_ino,
                    expected_sha=sha,
                )
            )

        # 2. Remove subdirectories (deepest first from bottom-up walk)
        for dir_p in subdirs_to_remove:
            st = os.stat(dir_p)
            steps.append(
                MutationStep(
                    action=MutationAction.REMOVE_DIR,
                    target_path=dir_p,
                    expected_dev=st.st_dev,
                    expected_ino=st.st_ino,
                )
            )

        # 3. Remove skill root directory
        st_root = os.stat(dest_skill_dir)
        steps.append(
            MutationStep(
                action=MutationAction.REMOVE_DIR,
                target_path=dest_skill_dir,
                expected_dev=st_root.st_dev,
                expected_ino=st_root.st_ino,
            )
        )

        plan = MutationPlan(
            plan_id=f"remove_{skill_id}_{int(snapshot.timestamp)}",
            target_dir=canon_target,
            steps=steps,
        )

        return plan, manifest, snapshot

    @classmethod
    def execute_remove(
        cls,
        skill_id: str,
        target_dir: Path,
        allowed_roots: List[Union[str, Path]],
        dry_run: bool = False,
        allow_global: bool = False,
    ) -> Tuple[TransactionResult, Optional[SkillManifest]]:
        """
        Execute skill removal with dry-run support and transaction safety.
        """
        plan, manifest, snapshot = cls.plan_remove(
            skill_id=skill_id,
            target_dir=target_dir,
            allowed_roots=allowed_roots,
            allow_global=allow_global,
        )

        if dry_run:
            return (
                TransactionResult(
                    success=True,
                    plan_id=plan.plan_id,
                    applied_count=0,
                    rolled_back=False,
                    error=None,
                ),
                manifest,
            )

        roots = [Path(r).resolve() for r in allowed_roots]

        # Revalidation hook: verifies under active lock that state matches snapshot
        def revalidate() -> bool:
            dest_skill_dir = plan.target_dir / skill_id
            if not dest_skill_dir.is_dir():
                return False
            # Re-verify integrity under active lock
            is_valid, _, report = verify_skill_integrity(dest_skill_dir, expected_skill_id=skill_id)
            if not is_valid:
                return False
            current_snap = PreMutationSnapshot.capture(dest_skill_dir)
            if current_snap.files != snapshot.files:
                return False
            cls.check_security_preconditions(plan.target_dir, roots, allow_global=allow_global)
            return True

        result = TransactionEngine.execute(
            plan=plan,
            allowed_roots=roots,
            revalidate_fn=revalidate,
        )

        return result, manifest if result.success else None
