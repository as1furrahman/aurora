"""
Adversarial security audit test suite for Agent Workbench Phase 4.
Constructed by Independent Security Auditor to evaluate:
- Inode and state-drift revalidation races between snapshot and lock
- Malformed and forged manifest handling
- Rollback execution on injected mutation step failure
- Boundary escaping and tautological CLI root containment
"""

import json
import os
import tempfile
import unittest
from pathlib import Path

from awb.config import Config
from awb.errors import (
    OwnershipError,
    SafetyError,
    StateDriftError,
    TransactionError,
)
from awb.operations import (
    MANIFEST_FILENAME,
    SkillInstaller,
    SkillRemover,
)
from awb.transactions import (
    MutationAction,
    MutationPlan,
    MutationStep,
    TransactionEngine,
)


class TestAuditPhase4(unittest.TestCase):
    """Adversarial security test suite created by independent auditor."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_audit_p4_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.config = Config.load()

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_adversarial_revalidation_aborts_on_concurrent_skill_creation(self):
        """
        Verify that if an external process creates the target skill directory
        between plan_install (snapshot capture) and lock acquisition,
        revalidation detects the state change and aborts without mutating.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        plan, manifest, snapshot = SkillInstaller.plan_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill_dir = target_dir / "principles"
        self.assertFalse(dest_skill_dir.exists())

        # Simulate concurrent process creating the destination directory before lock
        dest_skill_dir.mkdir(parents=True, exist_ok=True)
        (dest_skill_dir / "rogue.txt").write_text("concurrent content", encoding="utf-8")

        # Revalidation must catch the difference and TransactionEngine must fail closed
        def revalidate() -> bool:
            from awb.operations.snapshots import PreMutationSnapshot
            current_snap = PreMutationSnapshot.capture(dest_skill_dir)
            return current_snap.files == snapshot.files and current_snap.exists == snapshot.exists

        with self.assertRaises(TransactionError) as ctx:
            TransactionEngine.execute(
                plan=plan,
                allowed_roots=self.allowed_roots,
                revalidate_fn=revalidate,
            )
        self.assertIn("Post-acquire target state re-validation failed", str(ctx.exception))
        # Ensure rogue file was not overwritten
        self.assertTrue((dest_skill_dir / "rogue.txt").is_file())
        self.assertEqual((dest_skill_dir / "rogue.txt").read_text(encoding="utf-8"), "concurrent content")

    def test_adversarial_revalidation_aborts_on_concurrent_file_modification(self):
        """
        Verify that if a managed file is modified between plan_remove and lock acquisition,
        revalidation aborts and does not unlink the user's modified file.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        # Install skill first
        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill_dir = target_dir / "principles"
        plan, manifest, snapshot = SkillRemover.plan_remove(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
        )

        # Concurrently modify SKILL.md before lock acquisition
        (dest_skill_dir / "SKILL.md").write_text("# Concurrently edited", encoding="utf-8")

        # Attempt execute with revalidation
        def revalidate() -> bool:
            from awb.operations.snapshots import PreMutationSnapshot
            current_snap = PreMutationSnapshot.capture(dest_skill_dir)
            return current_snap.files == snapshot.files

        with self.assertRaises(TransactionError) as ctx:
            TransactionEngine.execute(
                plan=plan,
                allowed_roots=self.allowed_roots,
                revalidate_fn=revalidate,
            )
        self.assertIn("Post-acquire target state re-validation failed", str(ctx.exception))
        # File must remain intact
        self.assertTrue((dest_skill_dir / "SKILL.md").is_file())
        self.assertEqual((dest_skill_dir / "SKILL.md").read_text(encoding="utf-8"), "# Concurrently edited")

    def test_adversarial_remover_rejects_malformed_json_manifest(self):
        """Verify SkillRemover fails closed with OwnershipError on corrupted JSON manifest."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill_dir = target_dir / "principles"
        manifest_file = dest_skill_dir / MANIFEST_FILENAME
        manifest_file.write_text("{corrupted: json [", encoding="utf-8")

        with self.assertRaises(OwnershipError) as ctx:
            SkillRemover.execute_remove(
                skill_id="principles",
                target_dir=target_dir,
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("not demonstrably owned by AWB", str(ctx.exception))
        self.assertTrue((dest_skill_dir / "SKILL.md").is_file())

    def test_adversarial_remover_rejects_altered_hash_manifest(self):
        """Verify SkillRemover detects when manifest hash has been tampered with or desynchronized."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill_dir = target_dir / "principles"
        manifest_file = dest_skill_dir / MANIFEST_FILENAME
        data = json.loads(manifest_file.read_text(encoding="utf-8"))
        # Alter expected hash to bogus value
        data["files"]["SKILL.md"]["sha256"] = "0000000000000000000000000000000000000000000000000000000000000000"
        manifest_file.write_text(json.dumps(data), encoding="utf-8")

        with self.assertRaises(StateDriftError) as ctx:
            SkillRemover.execute_remove(
                skill_id="principles",
                target_dir=target_dir,
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("modified since installation", str(ctx.exception))
        self.assertTrue((dest_skill_dir / "SKILL.md").is_file())

    def test_adversarial_install_rollback_on_step_failure(self):
        """Verify that failure during installation steps triggers automatic reverse rollback."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        dest_skill_dir = target_dir / "principles"

        # Construct a plan with a valid step followed by an impossible/invalid step outside boundary
        valid_file = dest_skill_dir / "step1.txt"
        invalid_outside_file = Path("/unauthorized_system_boundary/rogue.txt")

        plan = MutationPlan(
            plan_id="test_adversarial_rollback",
            target_dir=target_dir,
            steps=[
                MutationStep(action=MutationAction.CREATE_DIR, target_path=dest_skill_dir),
                MutationStep(action=MutationAction.WRITE_FILE, target_path=valid_file, content=b"temp"),
                MutationStep(action=MutationAction.WRITE_FILE, target_path=invalid_outside_file, content=b"fail"),
            ],
        )

        result = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(result.success)
        self.assertTrue(result.rolled_back)
        self.assertIn("failed", result.error or "")

        # Assert step 1 and step 2 were rolled back completely
        self.assertFalse(valid_file.exists())
        self.assertFalse(dest_skill_dir.exists())

    def test_sec04_rejects_protected_project_exact_targets(self):
        """SEC-04: Targets that equal protected project paths must be rejected unconditionally."""
        protected_paths = [
            Path("/home/asif/projects/token-efficient-workflow"),
            Path("/home/asif/projects/dwl"),
            Path("/home/asif/projects/dwl-native-bar-lab"),
        ]
        for prot in protected_paths:
            with self.assertRaises(SafetyError) as ctx:
                SkillInstaller.check_security_preconditions(
                    target_dir=prot,
                    allowed_roots=self.allowed_roots,
                )
            self.assertIn("conflicts with protected project directory", str(ctx.exception))

    def test_sec04_rejects_target_residing_within_protected_project(self):
        """SEC-04: Targets nested inside protected project directories must be rejected."""
        nested_target = Path("/home/asif/projects/dwl/nested/skills")
        with self.assertRaises(SafetyError) as ctx:
            SkillInstaller.check_security_preconditions(
                target_dir=nested_target,
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("conflicts with protected project directory", str(ctx.exception))

    def test_sec04_rejects_target_enclosing_protected_project(self):
        """SEC-04: Target directories that enclose protected projects must be rejected."""
        enclosing = Path("/home/asif/projects")
        with self.assertRaises(SafetyError) as ctx:
            SkillInstaller.check_security_preconditions(
                target_dir=enclosing,
                allowed_roots=[enclosing],
            )
        self.assertIn("conflicts with protected project directory", str(ctx.exception))

    def test_sec04_rejects_symlink_aliases_to_protected_projects(self):
        """SEC-04: Symlink aliases resolving to protected directories must be rejected."""
        symlink_target = self.root / "alias_to_dwl"
        symlink_target.symlink_to("/home/asif/projects/dwl")

        with self.assertRaises(SafetyError) as ctx:
            SkillInstaller.check_security_preconditions(
                target_dir=symlink_target,
                allowed_roots=self.allowed_roots,
            )
        # Rejection should occur either from SymlinkAncestorError or conflict
        self.assertTrue(
            "symlink" in str(ctx.exception).lower() or
            "conflicts with protected project" in str(ctx.exception)
        )

    def test_sec04_rejects_overly_broad_allowed_roots(self):
        """SEC-04: Overly broad allowed roots (/, /home, user home) must be rejected."""
        broad_roots = [Path("/"), Path("/home"), Path.home()]
        for broad in broad_roots:
            with self.assertRaises(SafetyError) as ctx:
                SkillInstaller.check_security_preconditions(
                    target_dir=self.root / "skills",
                    allowed_roots=[broad],
                )
            self.assertIn("overly broad", str(ctx.exception))

    def test_sec04_accepts_legitimate_disposable_sandbox(self):
        """SEC-04: Legitimate disposable sandboxes inside allowed_roots pass validation."""
        sandbox_target = self.root / "disposable_skills"
        canon = SkillInstaller.check_security_preconditions(
            target_dir=sandbox_target,
            allowed_roots=self.allowed_roots,
        )
        self.assertEqual(canon, sandbox_target.resolve())

    def test_sec05_create_dir_boundary_escape_rejected_at_execution_time(self):
        """SEC-05: In TransactionEngine, CREATE_DIR targeting out-of-root path fails closed."""
        outside_dir = Path("/tmp/rogue_dir_outside_allowed_boundary")

        plan = MutationPlan(
            plan_id="test_sec05_create_dir_escape",
            target_dir=self.root,
            steps=[
                MutationStep(action=MutationAction.CREATE_DIR, target_path=outside_dir),
            ],
        )

        result = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(result.success)
        self.assertTrue(result.rolled_back)
        self.assertIn("Path traversal rejected", result.error or "")
        self.assertFalse(outside_dir.exists())

    def test_sec05_create_dir_symlink_ancestor_rejected_at_execution_time(self):
        """SEC-05: In TransactionEngine, CREATE_DIR traversing a symlinked ancestor fails closed."""
        real_target_dir = self.root / "real_dir"
        real_target_dir.mkdir(parents=True, exist_ok=True)

        symlink_ancestor = self.root / "sym_ancestor"
        symlink_ancestor.symlink_to(real_target_dir)

        escaped_step = symlink_ancestor / "new_sub"

        plan = MutationPlan(
            plan_id="test_sec05_symlink_create_dir",
            target_dir=self.root,
            steps=[
                MutationStep(action=MutationAction.CREATE_DIR, target_path=escaped_step),
            ],
        )

        result = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(result.success)
        self.assertTrue(result.rolled_back)
        self.assertIn("unauthorized symlink", (result.error or "").lower())
        self.assertFalse((real_target_dir / "new_sub").exists())

    def test_sec07_rejects_group_writable_directory_without_sticky_bit(self):
        """SEC-07: Insecure group-writable directory without sticky bit raises SecurityPreconditionError."""
        group_dir = self.root / "group_shared"
        group_dir.mkdir(parents=True, exist_ok=True)
        # Set mode to 0775 (group-writable without sticky bit)
        os.chmod(group_dir, 0o775)

        from awb.errors import SecurityPreconditionError
        with self.assertRaises(SecurityPreconditionError) as ctx:
            SkillInstaller.check_security_preconditions(
                target_dir=group_dir / "skills",
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("group-writable without sticky bit", str(ctx.exception))

    def test_sec07_rejects_world_writable_directory_without_sticky_bit(self):
        """SEC-07: Insecure world-writable directory without sticky bit raises SecurityPreconditionError."""
        world_dir = self.root / "world_shared"
        world_dir.mkdir(parents=True, exist_ok=True)
        # Set mode to 0777 (world-writable without sticky bit)
        os.chmod(world_dir, 0o777)

        from awb.errors import SecurityPreconditionError
        with self.assertRaises(SecurityPreconditionError) as ctx:
            SkillInstaller.check_security_preconditions(
                target_dir=world_dir / "skills",
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("world-writable without sticky bit", str(ctx.exception))

    def test_sec07_accepts_group_writable_directory_with_sticky_bit(self):
        """SEC-07: Group-writable directory WITH sticky bit (e.g. 01775) is accepted."""
        sticky_dir = self.root / "sticky_shared"
        sticky_dir.mkdir(parents=True, exist_ok=True)
        # Set mode to 01775 (group-writable WITH sticky bit)
        os.chmod(sticky_dir, 0o1775)

        canon = SkillInstaller.check_security_preconditions(
            target_dir=sticky_dir / "skills",
            allowed_roots=self.allowed_roots,
        )
        self.assertEqual(canon, (sticky_dir / "skills").resolve())


if __name__ == "__main__":
    unittest.main()
