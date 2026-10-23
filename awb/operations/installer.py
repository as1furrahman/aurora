"""
Skill installation engine for Agent Workbench.
Enforces strict ownership, pre-mutation snapshots, dry-run planning,
and transactional execution under ProcessLock.
Zero third-party runtime dependencies.
"""

import hashlib
import os
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple, Union

from awb.config import Config
from awb.errors import (
    GlobalMutationBlockedError,
    OwnershipError,
    RegistryError,
    SafetyError,
    SecurityPreconditionError,
    StateDriftError,
    TransactionError,
    ValidationError,
)
from awb.operations.manifest import (
    MANIFEST_FILENAME,
    MANIFEST_VERSION,
    SkillManifest,
    verify_skill_integrity,
)
from awb.operations.snapshots import PreMutationSnapshot
from awb.registry.parser import load_registry
from awb.registry.schema import RegistryManifest, SkillDefinition
from awb.safety import SafeFS, validate_security_preconditions
from awb.transactions import (
    MutationAction,
    MutationPlan,
    MutationStep,
    TransactionEngine,
    TransactionResult,
)


class SkillInstaller:
    """Orchestrates secure, transactional skill installation."""

    @classmethod
    def check_security_preconditions(
        cls,
        target_dir: Union[str, Path],
        allowed_roots: List[Union[str, Path]],
        allow_global: bool = False,
    ) -> Path:
        """
        Validate filesystem security preconditions before any operation.
        Enforces permissions, ownership, boundary containment, protected project shielding,
        and global directory blocks.
        """
        return validate_security_preconditions(
            target_dir=target_dir,
            allowed_roots=allowed_roots,
            allow_global=allow_global,
        )

    @classmethod
    def plan_install(
        cls,
        skill_id: str,
        target_dir: Path,
        allowed_roots: List[Union[str, Path]],
        config: Optional[Config] = None,
        custom_registry: Optional[Path] = None,
        allow_global: bool = False,
    ) -> Tuple[MutationPlan, SkillManifest, PreMutationSnapshot]:
        """
        Construct a validated, deterministic MutationPlan for skill installation.
        Does not acquire locks or perform mutations.
        """
        cfg = config or Config.load()
        reg_path = custom_registry or cfg.registry_path
        manifest = load_registry(reg_path)

        if skill_id not in manifest.skills:
            raise RegistryError(f"Skill '{skill_id}' is not defined in registry '{reg_path}'")

        skill: SkillDefinition = manifest.skills[skill_id]
        roots = [Path(r).resolve() for r in allowed_roots]
        canon_target = cls.check_security_preconditions(target_dir, roots, allow_global=allow_global)

        dest_skill_dir = canon_target / skill.id

        # Check existing destination
        existing_manifest: Optional[SkillManifest] = None
        if dest_skill_dir.exists():
            if not dest_skill_dir.is_dir():
                raise SafetyError(f"Destination '{dest_skill_dir}' exists and is not a directory")
            if dest_skill_dir.is_symlink():
                raise SafetyError(f"Destination '{dest_skill_dir}' is an unauthorized symlink")

            # Check if already managed by AWB
            is_valid, existing_manifest, report = verify_skill_integrity(dest_skill_dir, expected_skill_id=skill.id)
            if not is_valid:
                raise OwnershipError(
                    f"Destination '{dest_skill_dir}' exists but is not a valid AWB-managed skill: "
                    f"{'; '.join(report.errors)}. Aborting to prevent data loss."
                )

        snapshot = PreMutationSnapshot.capture(dest_skill_dir)

        # Locate and read source files from project root
        source_files: Dict[str, bytes] = {}
        file_metadata: Dict[str, Dict[str, Any]] = {}

        for rel_file in skill.files:
            source_path = (cfg.project_root / rel_file).resolve()
            if not source_path.is_file():
                raise RegistryError(f"Source file for skill '{skill.id}' not found at '{source_path}'")
            if source_path.is_symlink():
                raise SafetyError(f"Source file '{source_path}' cannot be a symlink")

            content = source_path.read_bytes()
            sha256_hash = hashlib.sha256(content).hexdigest()

            # Target relative path inside skill directory
            # If rel_file is 'skills/<id>/SKILL.md', target inside dest_skill_dir is 'SKILL.md'
            parts = Path(rel_file).parts
            if len(parts) >= 3 and parts[0] == "skills" and parts[1] == skill.id:
                sub_rel = str(Path(*parts[2:]))
            else:
                sub_rel = Path(rel_file).name

            source_files[sub_rel] = content
            file_metadata[sub_rel] = {
                "sha256": sha256_hash,
                "size": len(content),
                "mode": "0o100644",
            }

        installed_at = datetime.now(timezone.utc).isoformat()
        awb_manifest = SkillManifest(
            manifest_version=MANIFEST_VERSION,
            awb_managed=True,
            skill_id=skill.id,
            version=skill.version,
            installed_at=installed_at,
            source_registry=str(reg_path),
            files=file_metadata,
        )

        manifest_bytes = awb_manifest.to_json().encode("utf-8")

        # Construct mutation steps
        steps: List[MutationStep] = []

        # 1. Ensure target skill directory exists
        steps.append(
            MutationStep(
                action=MutationAction.CREATE_DIR,
                target_path=dest_skill_dir,
            )
        )

        # SEC-09: Plan transactional pruning of obsolete managed files
        if existing_manifest is not None:
            obsolete_files = set(existing_manifest.files.keys()) - set(source_files.keys())
            obsolete_subdirs_to_prune = set()

            if obsolete_files:
                for obs_rel in sorted(obsolete_files):
                    obs_path = dest_skill_dir / obs_rel
                    # Confirm obsolete file exists and is regular file
                    if not obs_path.exists() or obs_path.is_symlink() or not obs_path.is_file():
                        raise SafetyError(f"Cannot prune obsolete file '{obs_path}': invalid file or unauthorized symlink")

                    # Verify recorded integrity state (prevent deleting modified obsolete files)
                    with open(obs_path, "rb") as f:
                        obs_content = f.read()
                    obs_sha = hashlib.sha256(obs_content).hexdigest()
                    expected_sha = existing_manifest.files[obs_rel].get("sha256")
                    if expected_sha and obs_sha != expected_sha:
                        raise StateDriftError(
                            f"Cannot prune obsolete file '{obs_rel}': content has been modified since installation"
                        )

                    obs_st = os.stat(obs_path)
                    steps.append(
                        MutationStep(
                            action=MutationAction.UNLINK_FILE,
                            target_path=obs_path,
                            expected_dev=obs_st.st_dev,
                            expected_ino=obs_st.st_ino,
                            expected_sha=obs_sha,
                        )
                    )

                    # Track parent directories of obsolete files
                    curr_parent = obs_path.parent
                    while curr_parent != dest_skill_dir and dest_skill_dir in curr_parent.parents:
                        obsolete_subdirs_to_prune.add(curr_parent)
                        curr_parent = curr_parent.parent

                # Determine which obsolete subdirs will be completely empty
                new_subdirs = {(dest_skill_dir / sf).parent for sf in source_files.keys()}
                needed_subdirs = set()
                for sd in new_subdirs:
                    curr = sd
                    while curr != dest_skill_dir and dest_skill_dir in curr.parents:
                        needed_subdirs.add(curr)
                        curr = curr.parent

                # Empty obsolete subdirectories to remove, deepest-first
                removable_subdirs = sorted(
                    [d for d in obsolete_subdirs_to_prune if d not in needed_subdirs],
                    key=lambda p: len(p.parts),
                    reverse=True,
                )

                for empty_dir in removable_subdirs:
                    st_d = os.stat(empty_dir)
                    steps.append(
                        MutationStep(
                            action=MutationAction.REMOVE_DIR,
                            target_path=empty_dir,
                            expected_dev=st_d.st_dev,
                            expected_ino=st_d.st_ino,
                        )
                    )

        # 2. Ensure subdirectories exist
        created_subdirs = set()
        for sub_rel in sorted(source_files.keys()):
            sub_dir = (dest_skill_dir / sub_rel).parent
            if sub_dir != dest_skill_dir and sub_dir not in created_subdirs:
                steps.append(
                    MutationStep(
                        action=MutationAction.CREATE_DIR,
                        target_path=sub_dir,
                    )
                )
                created_subdirs.add(sub_dir)

        # 3. Write skill files
        for sub_rel in sorted(source_files.keys()):
            target_file = dest_skill_dir / sub_rel
            steps.append(
                MutationStep(
                    action=MutationAction.WRITE_FILE,
                    target_path=target_file,
                    content=source_files[sub_rel],
                    mode=0o644,
                )
            )

        # 4. Write .awb-manifest.json
        manifest_path = dest_skill_dir / MANIFEST_FILENAME
        steps.append(
            MutationStep(
                action=MutationAction.WRITE_FILE,
                target_path=manifest_path,
                content=manifest_bytes,
                mode=0o644,
            )
        )

        plan = MutationPlan(
            plan_id=f"install_{skill.id}_{int(snapshot.timestamp)}",
            target_dir=canon_target,
            steps=steps,
        )

        return plan, awb_manifest, snapshot

    @classmethod
    def execute_install(
        cls,
        skill_id: str,
        target_dir: Path,
        allowed_roots: List[Union[str, Path]],
        config: Optional[Config] = None,
        custom_registry: Optional[Path] = None,
        dry_run: bool = False,
        allow_global: bool = False,
    ) -> Tuple[TransactionResult, Optional[SkillManifest]]:
        """
        Execute skill installation with dry-run support and transaction safety.
        """
        plan, manifest, snapshot = cls.plan_install(
            skill_id=skill_id,
            target_dir=target_dir,
            allowed_roots=allowed_roots,
            config=config,
            custom_registry=custom_registry,
            allow_global=allow_global,
        )

        if dry_run:
            # Non-mutating plan inspection
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

        # Revalidation hook: verifies target state did not drift before lock acquisition
        def revalidate() -> bool:
            # Target snapshot must still match
            dest_skill_dir = plan.target_dir / skill_id
            current_snap = PreMutationSnapshot.capture(dest_skill_dir)
            if current_snap.exists != snapshot.exists:
                return False
            if current_snap.files != snapshot.files:
                return False
            # Parent directory must remain secure
            cls.check_security_preconditions(plan.target_dir, roots, allow_global=allow_global)
            return True

        result = TransactionEngine.execute(
            plan=plan,
            allowed_roots=roots,
            revalidate_fn=revalidate,
        )

        return result, manifest if result.success else None
