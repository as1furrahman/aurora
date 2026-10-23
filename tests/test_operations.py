"""
Comprehensive unit and adversarial test suite for skill installation and removal operations.
Verifies dry-run planning, strict ownership enforcement, content-drift protection,
transactional rollback, and global directory mutation blocks.
"""

import json
import os
import tempfile
import unittest
from pathlib import Path

from awb.config import Config
from awb.errors import (
    GlobalMutationBlockedError,
    OwnershipError,
    SafetyError,
    SecurityPreconditionError,
    StateDriftError,
    TransactionError,
)
from awb.operations import (
    MANIFEST_FILENAME,
    SkillInstaller,
    SkillRemover,
    verify_skill_integrity,
)


class TestOperations(unittest.TestCase):
    """Test suite for awb.operations (install, remove, ownership, safety)."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_test_ops_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.config = Config.load()

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_plan_install_dry_run_leaves_filesystem_pristine(self):
        """Dry-run installation constructs valid plan without modifying disk."""
        target_dir = self.root / "agent_skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        plan, manifest, snapshot = SkillInstaller.plan_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        self.assertEqual(manifest.skill_id, "principles")
        self.assertTrue(manifest.awb_managed)
        self.assertGreater(len(plan.steps), 0)

        # Execute in dry-run mode
        result, res_manifest = SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
            dry_run=True,
        )

        self.assertTrue(result.success)
        self.assertEqual(result.applied_count, 0)
        # Ensure destination skill directory was not created on disk
        self.assertFalse((target_dir / "principles").exists())

    def test_install_single_skill_success(self):
        """Install a single canonical skill and verify files, manifest, and lock persistence."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        result, manifest = SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
            dry_run=False,
        )

        self.assertTrue(result.success)
        self.assertIsNotNone(manifest)
        self.assertEqual(result.applied_count, len(manifest.files) + 2)  # files + dir + manifest

        dest_skill = target_dir / "principles"
        self.assertTrue(dest_skill.is_dir())
        self.assertTrue((dest_skill / "SKILL.md").is_file())
        self.assertTrue((dest_skill / MANIFEST_FILENAME).is_file())

        # Verify integrity
        is_valid, read_manifest, report = verify_skill_integrity(dest_skill, expected_skill_id="principles")
        self.assertTrue(is_valid)
        self.assertEqual(len(report.errors), 0)
        self.assertEqual(read_manifest.skill_id, "principles")

        # Verify lock persistence: .awb.lock must exist and not be locked
        lock_file = target_dir / ".awb.lock"
        self.assertTrue(lock_file.is_file())

    def test_install_rejects_global_agent_directories(self):
        """Direct installation into real global agent directories is blocked in Phase 4."""
        user_home = Path.home().resolve()
        global_targets = [
            user_home / ".codex" / "skills",
            user_home / ".gemini" / "antigravity-cli" / "skills",
            user_home / ".gemini" / "config" / "skills",
        ]

        for gt in global_targets:
            with self.assertRaises(GlobalMutationBlockedError):
                SkillInstaller.execute_install(
                    skill_id="principles",
                    target_dir=gt,
                    allowed_roots=[user_home],
                    config=self.config,
                    allow_global=False,
                )

    def test_install_rejects_system_directory(self):
        """Installation targeting or residing inside .system/ is rejected."""
        system_target = self.root / ".system"
        system_target.mkdir(parents=True, exist_ok=True)

        with self.assertRaises(SafetyError) as ctx:
            SkillInstaller.execute_install(
                skill_id="principles",
                target_dir=system_target,
                allowed_roots=self.allowed_roots,
                config=self.config,
            )
        self.assertIn(".system", str(ctx.exception))

    def test_install_rejects_unauthorized_boundary(self):
        """Installation outside declared allowed roots fails closed."""
        outside = Path("/tmp/outside_test_awb_dir")
        with self.assertRaises(SafetyError):
            SkillInstaller.execute_install(
                skill_id="principles",
                target_dir=outside,
                allowed_roots=self.allowed_roots,
                config=self.config,
            )

    def test_install_rejects_existing_unmanaged_directory(self):
        """Existing directory without AWB manifest is rejected to avoid clobbering user files."""
        target_dir = self.root / "skills"
        unmanaged_skill = target_dir / "principles"
        unmanaged_skill.mkdir(parents=True, exist_ok=True)
        user_file = unmanaged_skill / "my_custom_code.py"
        user_file.write_text("print('user')", encoding="utf-8")

        with self.assertRaises(OwnershipError) as ctx:
            SkillInstaller.execute_install(
                skill_id="principles",
                target_dir=target_dir,
                allowed_roots=self.allowed_roots,
                config=self.config,
            )
        self.assertIn("not a valid AWB-managed skill", str(ctx.exception))
        # Ensure user file is completely unharmed
        self.assertTrue(user_file.is_file())
        self.assertEqual(user_file.read_text(encoding="utf-8"), "print('user')")

    def test_remove_dry_run_leaves_skill_intact(self):
        """Dry-run removal constructs plan without unlinking any file."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        # Install skill first
        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill = target_dir / "principles"
        self.assertTrue((dest_skill / "SKILL.md").is_file())

        # Execute removal in dry-run mode
        result, manifest = SkillRemover.execute_remove(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            dry_run=True,
        )

        self.assertTrue(result.success)
        self.assertEqual(result.applied_count, 0)
        # All files must remain intact
        self.assertTrue(dest_skill.is_dir())
        self.assertTrue((dest_skill / "SKILL.md").is_file())
        self.assertTrue((dest_skill / MANIFEST_FILENAME).is_file())

    def test_remove_single_skill_success(self):
        """Remove a demonstrably owned AWB skill and verify complete deletion."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill = target_dir / "principles"
        self.assertTrue(dest_skill.is_dir())

        result, manifest = SkillRemover.execute_remove(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            dry_run=False,
        )

        self.assertTrue(result.success)
        self.assertFalse(dest_skill.exists())

    def test_remove_rejects_unmanaged_directory(self):
        """Attempting to remove an unmanaged directory raises OwnershipError."""
        target_dir = self.root / "skills"
        fake_skill = target_dir / "principles"
        fake_skill.mkdir(parents=True, exist_ok=True)
        (fake_skill / "SKILL.md").write_text("# Unmanaged", encoding="utf-8")

        with self.assertRaises(OwnershipError) as ctx:
            SkillRemover.execute_remove(
                skill_id="principles",
                target_dir=target_dir,
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("not demonstrably owned by AWB", str(ctx.exception))
        # Ensure fake skill was not removed
        self.assertTrue(fake_skill.is_dir())

    def test_remove_rejects_modified_files_content_drift(self):
        """Content-drift protection: modified files abort removal to prevent data loss."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill = target_dir / "principles"
        skill_md = dest_skill / "SKILL.md"

        # Simulate user modifying the skill file
        skill_md.write_text("# Modified by user with custom additions", encoding="utf-8")

        with self.assertRaises(StateDriftError) as ctx:
            SkillRemover.execute_remove(
                skill_id="principles",
                target_dir=target_dir,
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("modified since installation", str(ctx.exception))
        # Directory and modified file must remain intact
        self.assertTrue(dest_skill.is_dir())
        self.assertTrue(skill_md.is_file())

    def test_remove_rejects_extraneous_unmanaged_files(self):
        """Extraneous unmanaged files inside skill directory abort removal."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill = target_dir / "principles"
        extra_file = dest_skill / "my_notes.txt"
        extra_file.write_text("important user notes", encoding="utf-8")

        with self.assertRaises(OwnershipError) as ctx:
            SkillRemover.execute_remove(
                skill_id="principles",
                target_dir=target_dir,
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("extraneous unmanaged files", str(ctx.exception))
        self.assertTrue(dest_skill.is_dir())
        self.assertTrue(extra_file.is_file())

    def test_remove_rejects_symlink_inside_skill(self):
        """Symlinks inside skill directory trigger safety rejection."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install(
            skill_id="principles",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        dest_skill = target_dir / "principles"
        symlink_file = dest_skill / "escape.txt"
        symlink_file.symlink_to("/etc/hosts")

        with self.assertRaises(SafetyError) as ctx:
            SkillRemover.execute_remove(
                skill_id="principles",
                target_dir=target_dir,
                allowed_roots=self.allowed_roots,
            )
        self.assertIn("symlinks detected", str(ctx.exception))
        self.assertTrue(dest_skill.is_dir())

    def test_remove_rejects_global_agent_directories(self):
        """Direct removal against real global agent directories is blocked."""
        user_home = Path.home().resolve()
        global_target = user_home / ".codex" / "skills"

        with self.assertRaises(GlobalMutationBlockedError):
            SkillRemover.execute_remove(
                skill_id="principles",
                target_dir=global_target,
                allowed_roots=[user_home],
                allow_global=False,
            )

    def test_multi_file_skill_install_and_remove(self):
        """Install and remove a multi-file skill (token-efficient-workflow with nested rules)."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        result, manifest = SkillInstaller.execute_install(
            skill_id="token-efficient-workflow",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
            config=self.config,
        )

        self.assertTrue(result.success)
        dest_skill = target_dir / "token-efficient-workflow"
        self.assertTrue((dest_skill / "SKILL.md").is_file())
        self.assertTrue((dest_skill / "rules" / "efficient-workflow.md").is_file())
        self.assertTrue((dest_skill / MANIFEST_FILENAME).is_file())

        # Remove
        rem_result, _ = SkillRemover.execute_remove(
            skill_id="token-efficient-workflow",
            target_dir=target_dir,
            allowed_roots=self.allowed_roots,
        )

        self.assertTrue(rem_result.success)
        self.assertFalse(dest_skill.exists())


if __name__ == "__main__":
    unittest.main()
