"""
Phase 5 — Transaction Reliability and Adversarial Hardening Test Suite.
Exclusively operates inside disposable temporary directories.
Evaluates:
1. Failure injection and transactional rollback across all steps
2. Update idempotency, source changes, and repeat operations
3. Concurrency, lock contention, and process interruption
4. Manifest tampering, unexpected files, symlink substitution, and permission changes
"""

import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

from awb.config import Config
from awb.errors import (
    LockTimeoutError,
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
from awb.safety import ProcessLock, SafeFS
from awb.transactions import (
    MutationAction,
    MutationPlan,
    MutationStep,
    TransactionEngine,
    TransactionResult,
)


class TestPhase5Reliability(unittest.TestCase):
    """Phase 5 Transaction Reliability & Adversarial Hardening Tests."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_p5_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.config = Config.load()

    def tearDown(self):
        self.temp_dir.cleanup()

    # =========================================================================
    # 1. FAILURE INJECTION AND ROLLBACK
    # =========================================================================

    def test_failure_injection_at_each_install_step(self):
        """
        Force failures after each individual transaction step during installation.
        Confirm rollback cleans up all created artifacts and restores pre-existing state.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        dest_dir = target_dir / "test_skill"

        step_definitions = [
            MutationStep(MutationAction.CREATE_DIR, dest_dir),
            MutationStep(MutationAction.WRITE_FILE, dest_dir / "file1.txt", content=b"content 1"),
            MutationStep(MutationAction.WRITE_FILE, dest_dir / "file2.txt", content=b"content 2"),
            MutationStep(MutationAction.WRITE_FILE, dest_dir / MANIFEST_FILENAME, content=b"manifest"),
        ]

        # Test failure after step 0, 1, 2, 3
        for fail_idx in range(len(step_definitions)):
            steps = list(step_definitions[:fail_idx + 1])
            # Append an impossible step that violates allowed roots boundary
            steps.append(
                MutationStep(
                    MutationAction.WRITE_FILE,
                    Path("/unauthorized_escape_root/fail.txt"),
                    content=b"fail",
                )
            )

            plan = MutationPlan(
                plan_id=f"plan_fail_at_{fail_idx}",
                target_dir=target_dir,
                steps=steps,
            )

            result = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
            self.assertFalse(result.success, f"Plan should have failed when fault injected at step {fail_idx}")
            self.assertTrue(result.rolled_back)

            # Assert complete rollback: no files or directories leaked
            self.assertFalse(dest_dir.exists(), f"Destination directory leaked after rollback at step {fail_idx}")

    def test_failure_injection_during_update_restores_prior_content(self):
        """
        When updating an existing installed skill, a failure during file write must
        restore prior content and preserve the pre-existing skill directory intact.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        # 1. Install pristine principles skill
        res1, m1 = SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        self.assertTrue(res1.success)

        dest_skill = target_dir / "principles"
        skill_md = dest_skill / "SKILL.md"
        original_content = skill_md.read_bytes()

        # 2. Simulate an update plan where SKILL.md is overwritten, followed by an error
        plan = MutationPlan(
            plan_id="faulted_update_plan",
            target_dir=target_dir,
            steps=[
                MutationStep(
                    MutationAction.WRITE_FILE,
                    skill_md,
                    content=b"# New Tampered Content that should be rolled back",
                ),
                MutationStep(
                    MutationAction.WRITE_FILE,
                    Path("/unauthorized_escape_root/error.txt"),
                    content=b"fail",
                ),
            ],
        )

        result = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(result.success)
        self.assertTrue(result.rolled_back)

        # Verify SKILL.md has been restored to its exact prior content
        self.assertEqual(skill_md.read_bytes(), original_content)
        # Verify destination skill directory was NOT deleted
        self.assertTrue(dest_skill.is_dir())
        # Verify integrity report still passes
        valid, _, _ = verify_skill_integrity(dest_skill, expected_skill_id="principles")
        self.assertTrue(valid)

    def test_failure_injection_during_removal_restores_unlinked_files(self):
        """
        If a failure occurs during skill removal, previously unlinked files must
        be restored with their prior content and permissions.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        res1, m1 = SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        self.assertTrue(res1.success)

        dest_skill = target_dir / "principles"
        skill_md = dest_skill / "SKILL.md"
        orig_content = skill_md.read_bytes()

        # Plan removal with an injected failure after unlinking SKILL.md
        st = os.stat(skill_md)
        plan = MutationPlan(
            plan_id="faulted_removal_plan",
            target_dir=target_dir,
            steps=[
                MutationStep(
                    MutationAction.UNLINK_FILE,
                    skill_md,
                    expected_dev=st.st_dev,
                    expected_ino=st.st_ino,
                    expected_sha=hashlib.sha256(orig_content).hexdigest(),
                ),
                MutationStep(
                    MutationAction.WRITE_FILE,
                    Path("/unauthorized_escape_root/fail.txt"),
                    content=b"fail",
                ),
            ],
        )

        result = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(result.success)
        self.assertTrue(result.rolled_back)

        # Assert SKILL.md was restored
        self.assertTrue(skill_md.is_file())
        self.assertEqual(skill_md.read_bytes(), orig_content)

    def test_rollback_preserves_unrelated_sibling_files(self):
        """
        Rollback of a failed skill operation must never unlink unrelated files
        or sibling directories in the target tree.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        # Create unrelated files and sibling directories
        unrelated_file = target_dir / "unrelated_notes.txt"
        unrelated_file.write_text("critical user data", encoding="utf-8")

        sibling_dir = target_dir / "sibling_tool"
        sibling_dir.mkdir(parents=True, exist_ok=True)
        (sibling_dir / "tool.py").write_text("print('safe')", encoding="utf-8")

        dest_dir = target_dir / "test_failed_skill"
        plan = MutationPlan(
            plan_id="plan_fail_with_unrelated",
            target_dir=target_dir,
            steps=[
                MutationStep(MutationAction.CREATE_DIR, dest_dir),
                MutationStep(MutationAction.WRITE_FILE, dest_dir / "file.txt", content=b"temp"),
                MutationStep(MutationAction.WRITE_FILE, Path("/unauthorized_root/bad.txt"), content=b"bad"),
            ],
        )

        result = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(result.success)
        self.assertTrue(result.rolled_back)

        # Unrelated files and sibling directory must remain 100% untouched
        self.assertTrue(unrelated_file.is_file())
        self.assertEqual(unrelated_file.read_text(encoding="utf-8"), "critical user data")
        self.assertTrue((sibling_dir / "tool.py").is_file())

    # =========================================================================
    # 2. UPDATE AND REPEAT-OPERATION TESTING
    # =========================================================================

    def test_repeat_install_idempotent(self):
        """
        Installing an already-installed skill twice (repeat install) is safe,
        idempotent, preserves skill validity, and updates manifest cleanly.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        # First install
        r1, m1 = SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        self.assertTrue(r1.success)
        self.assertEqual(r1.applied_count, 3)

        # Second install (idempotent repeat operation)
        r2, m2 = SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        self.assertTrue(r2.success)
        self.assertEqual(r2.applied_count, 3)

        dest_skill = target_dir / "principles"
        self.assertTrue((dest_skill / "SKILL.md").is_file())
        self.assertTrue((dest_skill / MANIFEST_FILENAME).is_file())

        # Verify integrity passes cleanly
        valid, manifest, report = verify_skill_integrity(dest_skill, expected_skill_id="principles")
        self.assertTrue(valid)
        self.assertEqual(len(report.errors), 0)

    def test_reinstall_over_unmanaged_directory_fails_closed(self):
        """
        Attempting repeat install or update over an unmanaged directory
        (or directory with unmanaged files) fails closed with OwnershipError.
        """
        target_dir = self.root / "skills"
        dest_skill = target_dir / "principles"
        dest_skill.mkdir(parents=True, exist_ok=True)
        (dest_skill / "unmanaged.py").write_text("print('user')", encoding="utf-8")

        with self.assertRaises(OwnershipError) as ctx:
            SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        self.assertIn("not a valid AWB-managed skill", str(ctx.exception))
        # Ensure user unmanaged file is intact
        self.assertTrue((dest_skill / "unmanaged.py").is_file())

    def test_removal_after_aborted_update(self):
        """
        If an update operation fails and rolls back, the skill remains in a valid
        state and subsequent removal succeeds completely.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        r1, m1 = SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        self.assertTrue(r1.success)

        dest_skill = target_dir / "principles"
        skill_md = dest_skill / "SKILL.md"

        # Abort an update midway
        plan = MutationPlan(
            plan_id="failed_update",
            target_dir=target_dir,
            steps=[
                MutationStep(MutationAction.WRITE_FILE, skill_md, content=b"# Bad update"),
                MutationStep(MutationAction.WRITE_FILE, Path("/outside/fail.txt"), content=b"fail"),
            ],
        )
        res_fail = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(res_fail.success)

        # Removal must still succeed cleanly on the restored skill
        r_rem, m_rem = SkillRemover.execute_remove("principles", target_dir, self.allowed_roots)
        self.assertTrue(r_rem.success)
        self.assertFalse(dest_skill.exists())

    def test_cli_update_command_syntax(self):
        """awb update command is recognized by CLI and executes cleanly."""
        from awb.cli import main
        import io
        from contextlib import redirect_stdout

        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        # 1. Install
        code_inst = main(["install", "principles", "--target", str(target_dir), "--root", str(self.root)])
        self.assertEqual(code_inst, 0)

        # 2. Update via CLI
        f = io.StringIO()
        with redirect_stdout(f):
            code_up = main(["update", "principles", "--target", str(target_dir), "--root", str(self.root)])
        self.assertEqual(code_up, 0)
        self.assertIn("Skill Installation Successful", f.getvalue())

    # =========================================================================
    # 3. CONCURRENCY AND PROCESS INTERRUPTION
    # =========================================================================

    def test_concurrent_lock_contention_raises_timeout(self):
        """
        When one process holds ProcessLock, a competing contender with a short
        timeout raises LockTimeoutError and does not proceed.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        lock_file = target_dir / ".awb.lock"

        lock1 = ProcessLock(lock_file)
        lock2 = ProcessLock(lock_file, timeout=0.1, stale_threshold=1)

        with lock1:
            # Second contender attempts to acquire with 0.1s timeout
            with self.assertRaises(LockTimeoutError) as ctx:
                lock2.acquire()
            self.assertIn("Timed out", str(ctx.exception))

        # After lock1 releases, lock2 acquires cleanly
        with lock2:
            self.assertTrue(lock2.lock_path.is_file())

    def test_lock_inode_never_changes_across_acquisitions(self):
        """
        SEC-01 Invariant: .awb.lock is persistent and its device and inode
        never change across sequential acquire/release cycles.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        lock_file = target_dir / ".awb.lock"

        lock = ProcessLock(lock_file)

        # Cycle 1
        with lock:
            st1 = os.stat(lock_file)
            ino1 = (st1.st_dev, st1.st_ino)

        # Cycle 2
        with lock:
            st2 = os.stat(lock_file)
            ino2 = (st2.st_dev, st2.st_ino)

        # Cycle 3 with new instance
        lock_alt = ProcessLock(lock_file)
        with lock_alt:
            st3 = os.stat(lock_file)
            ino3 = (st3.st_dev, st3.st_ino)

        self.assertEqual(ino1, ino2)
        self.assertEqual(ino2, ino3)

    def test_simulated_process_crash_holding_lock_releases_cleanly(self):
        """
        When a process holding ProcessLock terminates abruptly (crash), the kernel
        automatically releases flock on the persistent lock file descriptor, allowing
        a waiting or subsequent process to acquire without deadlocking.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        lock_file = target_dir / ".awb.lock"

        script = f"""
import sys, time
from pathlib import Path
from awb.safety import ProcessLock

lock = ProcessLock(Path('{lock_file}'), timeout=5)
lock.acquire()
# Print ready to stdout and flush
print("LOCKED", flush=True)
# Keep holding lock until parent kills process
time.sleep(10)
"""
        proc = subprocess.Popen(
            [sys.executable, "-c", script],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            env={**os.environ, "PYTHONPATH": str(self.config.project_root)},
        )

        try:
            line = proc.stdout.readline()
            self.assertEqual(line.strip(), "LOCKED")

            # Verify lock is held from parent perspective
            contender = ProcessLock(lock_file, timeout=0.1)
            with self.assertRaises(LockTimeoutError):
                contender.acquire()

            # Abruptly kill the holding process (simulate crash / SIGKILL)
            proc.kill()
            proc.wait()

            # Now contender must immediately acquire successfully
            t0 = time.time()
            with contender:
                t_acquired = time.time() - t0
                self.assertLess(t_acquired, 1.0)
        finally:
            if proc.stdout:
                proc.stdout.close()
            if proc.stderr:
                proc.stderr.close()
            if proc.poll() is None:
                proc.kill()

    # =========================================================================
    # 4. MANIFEST AND FILESYSTEM ADVERSARIAL CASES
    # =========================================================================

    def test_manifest_missing_mandatory_fields_rejected(self):
        """Manifest missing awb_managed or skill_id fails closed with OwnershipError."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        dest_skill = target_dir / "principles"
        manifest_file = dest_skill / MANIFEST_FILENAME

        # Corrupt: missing awb_managed
        manifest_file.write_text(json.dumps({"skill_id": "principles", "version": "1.0.0"}), encoding="utf-8")

        with self.assertRaises((SafetyError, OwnershipError)) as ctx:
            SkillRemover.execute_remove("principles", target_dir, self.allowed_roots)

    def test_unexpected_unmanaged_file_inside_skill_rejected(self):
        """
        If a user or rogue process drops an unmanaged file into a skill directory,
        SkillRemover detects extraneous files and refuses removal.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        dest_skill = target_dir / "principles"

        # Drop extraneous file
        extraneous = dest_skill / "my_custom_script.sh"
        extraneous.write_text("#!/bin/sh\necho 'custom'", encoding="utf-8")

        with self.assertRaises(OwnershipError) as ctx:
            SkillRemover.execute_remove("principles", target_dir, self.allowed_roots)
        self.assertIn("extraneous unmanaged files", str(ctx.exception))
        # Ensure user file is preserved
        self.assertTrue(extraneous.is_file())

    def test_symlink_substitution_at_leaf_rejected(self):
        """Substituting a skill file with a symlink triggers safety rejection."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        dest_skill = target_dir / "principles"

        # Substitute SKILL.md with symlink to /etc/passwd
        skill_md = dest_skill / "SKILL.md"
        skill_md.unlink()
        skill_md.symlink_to("/etc/passwd")

        with self.assertRaises(SafetyError) as ctx:
            SkillRemover.execute_remove("principles", target_dir, self.allowed_roots)
        self.assertIn("symlinks detected", str(ctx.exception))

    def test_symlink_substitution_at_manifest_rejected(self):
        """Substituting .awb-manifest.json with a symlink triggers safety rejection."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        dest_skill = target_dir / "principles"

        manifest_file = dest_skill / MANIFEST_FILENAME
        manifest_file.unlink()
        manifest_file.symlink_to("/etc/hosts")

        with self.assertRaises((SafetyError, OwnershipError)) as ctx:
            SkillRemover.execute_remove("principles", target_dir, self.allowed_roots)

    def test_symlink_substitution_at_skill_directory_rejected(self):
        """Substituting the skill directory itself with a symlink triggers safety rejection."""
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        real_target = self.root / "real_principles"
        real_target.mkdir(parents=True, exist_ok=True)

        symlink_dir = target_dir / "principles"
        symlink_dir.symlink_to(real_target)

        with self.assertRaises(SafetyError) as ctx:
            SkillInstaller.execute_install("principles", target_dir, self.allowed_roots, config=self.config)
        self.assertIn("unauthorized symlink", str(ctx.exception))

    def test_permission_change_between_plan_and_execute_aborts(self):
        """
        If target parent directory permissions are changed to insecure mode (e.g. 0777)
        between plan capture and lock acquisition, revalidation detects it and aborts.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)

        plan, manifest, snapshot = SkillInstaller.plan_install("principles", target_dir, self.allowed_roots, config=self.config)

        # Insecure permission alteration: change target_dir to 0777 without sticky bit
        os.chmod(target_dir, 0o777)

        # Attempt to execute: revalidation hook must catch insecure permissions
        try:
            with self.assertRaises((TransactionError, SecurityPreconditionError)) as ctx:
                TransactionEngine.execute(
                    plan=plan,
                    allowed_roots=self.allowed_roots,
                    revalidate_fn=lambda: SkillInstaller.check_security_preconditions(target_dir, self.allowed_roots) is not None,
                )
        finally:
            os.chmod(target_dir, 0o755)


if __name__ == "__main__":
    unittest.main()
