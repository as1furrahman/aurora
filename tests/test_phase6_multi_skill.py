"""
Phase 6 — Multi-Skill Integration and Recovery Validation Test Suite.
Exclusively operates inside disposable temporary directories under /tmp.
Strictly non-mutating on live user directories or protected repositories.

Validates integrated behavior of:
- SEC-09: Obsolete-file pruning across multi-skill lifecycle operations.
- SEC-10A: KeyboardInterrupt rollback at all mutation boundaries.
- SEC-10B: Durable crash recovery, subprocess SIGKILL, and journal edge cases.
- SEC-11: Rollback path confinement and symlink shielding.
"""

import base64
import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from typing import Any, Dict, List

from awb.config import Config
from awb.errors import (
    LockTimeoutError,
    OwnershipError,
    PathTraversalError,
    SafetyError,
    StateDriftError,
    SymlinkAncestorError,
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
from awb.transactions.journal import DurableJournal, JournalStatus, JOURNAL_FILENAME


class TestPhase6MultiSkillLifecycle(unittest.TestCase):
    """Multi-Skill Lifecycle Operations in a shared sandbox root."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_p6_lifecycle_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.config = Config.load()
        self.target_dir = self.root / "skills"
        self.target_dir.mkdir(parents=True, exist_ok=True)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_multi_skill_install_and_integrity_verification(self):
        """
        Sequentially install all 4 canonical skills into a shared sandbox:
        principles, task-router, verification-guard, token-efficient-workflow.
        Verify integrity of each skill individually.
        """
        skill_ids = ["principles", "task-router", "verification-guard", "token-efficient-workflow"]
        for sid in skill_ids:
            res, manifest = SkillInstaller.execute_install(
                sid,
                self.target_dir,
                self.allowed_roots,
                config=self.config,
            )
            self.assertTrue(res.success, f"Install failed for skill '{sid}': {res.error}")
            self.assertIsNotNone(manifest)

            # Verify integrity
            valid, m_disk, report = verify_skill_integrity(self.target_dir / sid, expected_skill_id=sid)
            self.assertTrue(valid, f"Integrity failed for '{sid}': {report.errors}")
            self.assertEqual(len(report.errors), 0)

        # Check all 4 directories exist simultaneously
        for sid in skill_ids:
            skill_path = self.target_dir / sid
            self.assertTrue(skill_path.is_dir())
            self.assertTrue((skill_path / MANIFEST_FILENAME).is_file())
            self.assertTrue((skill_path / "SKILL.md").is_file())

        # Check multi-file skill structure
        self.assertTrue((self.target_dir / "token-efficient-workflow" / "rules" / "efficient-workflow.md").is_file())

    def test_multi_skill_repeat_install_idempotency(self):
        """
        Installing all 4 skills a second time (repeat install) is idempotent,
        preserves all sibling skills, and reports success.
        """
        skill_ids = ["principles", "task-router", "verification-guard", "token-efficient-workflow"]
        # Round 1
        for sid in skill_ids:
            r1, _ = SkillInstaller.execute_install(sid, self.target_dir, self.allowed_roots, config=self.config)
            self.assertTrue(r1.success)

        # Round 2 (repeat install)
        for sid in skill_ids:
            r2, _ = SkillInstaller.execute_install(sid, self.target_dir, self.allowed_roots, config=self.config)
            self.assertTrue(r2.success, f"Repeat install failed for '{sid}': {r2.error}")

            valid, _, report = verify_skill_integrity(self.target_dir / sid, expected_skill_id=sid)
            self.assertTrue(valid)
            self.assertEqual(len(report.errors), 0)

    def test_multi_skill_update_with_sec09_obsolete_pruning_sibling_isolation(self):
        """
        When updating token-efficient-workflow to prune rules/efficient-workflow.md:
        1. Obsolete file and directory are pruned.
        2. Sibling skills (principles, task-router, verification-guard) remain 100% intact.
        """
        skill_ids = ["principles", "task-router", "verification-guard", "token-efficient-workflow"]
        for sid in skill_ids:
            SkillInstaller.execute_install(sid, self.target_dir, self.allowed_roots, config=self.config)

        # Record sibling hashes before update
        sibling_hashes = {}
        for sid in ["principles", "task-router", "verification-guard"]:
            s_file = self.target_dir / sid / "SKILL.md"
            sibling_hashes[sid] = hashlib.sha256(s_file.read_bytes()).hexdigest()

        # Update token-efficient-workflow with custom registry omitting rules/
        custom_reg = self.root / "update_reg.toml"
        custom_reg.write_text("""[registry]
version = "1.0.0"
updated_at = "2026-10-23T20:00:00Z"
description = "Pruned update"

[[skills]]
id = "token-efficient-workflow"
name = "Token-Efficient Hardened Workflow"
version = "1.1.0"
category = "workflow"
description = "Pruned"
dependencies = []
compatible_agents = ["antigravity-cli"]
triggers = ["test"]
files = ["skills/token-efficient-workflow/SKILL.md"]
""", encoding="utf-8")

        r_up, m_up = SkillInstaller.execute_install(
            "token-efficient-workflow",
            self.target_dir,
            self.allowed_roots,
            config=self.config,
            custom_registry=custom_reg,
        )
        self.assertTrue(r_up.success)

        # token-efficient-workflow rules pruned
        tew_dir = self.target_dir / "token-efficient-workflow"
        self.assertFalse((tew_dir / "rules" / "efficient-workflow.md").exists())
        self.assertFalse((tew_dir / "rules").exists())
        self.assertTrue((tew_dir / "SKILL.md").is_file())

        # Sibling skills must be completely unchanged and pristine
        for sid, orig_sha in sibling_hashes.items():
            s_file = self.target_dir / sid / "SKILL.md"
            self.assertTrue(s_file.is_file())
            self.assertEqual(hashlib.sha256(s_file.read_bytes()).hexdigest(), orig_sha)
            valid, _, report = verify_skill_integrity(self.target_dir / sid, expected_skill_id=sid)
            self.assertTrue(valid)

    def test_multi_skill_partial_removal_and_reinstall(self):
        """
        Removing one skill (task-router) leaves all other skills intact.
        Reinstalling task-router succeeds cleanly.
        """
        for sid in ["principles", "task-router", "verification-guard"]:
            SkillInstaller.execute_install(sid, self.target_dir, self.allowed_roots, config=self.config)

        # Remove task-router
        r_rem, _ = SkillRemover.execute_remove("task-router", self.target_dir, self.allowed_roots)
        self.assertTrue(r_rem.success)
        self.assertFalse((self.target_dir / "task-router").exists())

        # Verify other 2 skills intact
        for sid in ["principles", "verification-guard"]:
            valid, _, report = verify_skill_integrity(self.target_dir / sid, expected_skill_id=sid)
            self.assertTrue(valid)

        # Reinstall task-router
        r_reinst, _ = SkillInstaller.execute_install("task-router", self.target_dir, self.allowed_roots, config=self.config)
        self.assertTrue(r_reinst.success)
        valid, _, report = verify_skill_integrity(self.target_dir / "task-router", expected_skill_id="task-router")
        self.assertTrue(valid)


class TestPhase6SEC09AdversarialPruning(unittest.TestCase):
    """Adversarial testing of SEC-09 Obsolete Pruning Invariants."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_p6_sec09_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.config = Config.load()
        self.target_dir = self.root / "skills"
        self.target_dir.mkdir(parents=True, exist_ok=True)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_unowned_foreign_file_in_obsolete_dir_fails_closed(self):
        """
        If an unmanaged foreign file is present in an obsolete directory,
        integrity verification detects it and aborts with OwnershipError.
        The unmanaged file and obsolete files remain intact.
        """
        SkillInstaller.execute_install("token-efficient-workflow", self.target_dir, self.allowed_roots, config=self.config)
        tew_dir = self.target_dir / "token-efficient-workflow"

        # User adds an unmanaged foreign file
        foreign_file = tew_dir / "rules" / "custom_user_rule.md"
        foreign_file.write_text("user custom content", encoding="utf-8")

        custom_reg = self.root / "prune_reg.toml"
        custom_reg.write_text("""[registry]
version = "1.0.0"
updated_at = "2026-10-23T20:00:00Z"
description = "Prune"

[[skills]]
id = "token-efficient-workflow"
name = "TEW"
version = "1.1.0"
category = "workflow"
description = "Prune"
dependencies = []
compatible_agents = ["antigravity-cli"]
triggers = ["test"]
files = ["skills/token-efficient-workflow/SKILL.md"]
""", encoding="utf-8")

        with self.assertRaises(OwnershipError) as ctx:
            SkillInstaller.execute_install(
                "token-efficient-workflow",
                self.target_dir,
                self.allowed_roots,
                config=self.config,
                custom_registry=custom_reg,
            )
        self.assertIn("Extraneous unmanaged files detected", str(ctx.exception))

        # Both the foreign file and original rule must be completely preserved
        self.assertTrue(foreign_file.is_file())
        self.assertTrue((tew_dir / "rules" / "efficient-workflow.md").is_file())

    def test_modified_obsolete_file_in_multi_skill_sandbox_fails_closed(self):
        """
        In a sandbox with multiple skills, modifying an obsolete file fails closed
        with OwnershipError / StateDriftError and corrupts no skills.
        """
        SkillInstaller.execute_install("principles", self.target_dir, self.allowed_roots, config=self.config)
        SkillInstaller.execute_install("token-efficient-workflow", self.target_dir, self.allowed_roots, config=self.config)

        tew_dir = self.target_dir / "token-efficient-workflow"
        obs_file = tew_dir / "rules" / "efficient-workflow.md"
        obs_file.write_text("tampered content", encoding="utf-8")

        custom_reg = self.root / "prune_reg.toml"
        custom_reg.write_text("""[registry]
version = "1.0.0"
updated_at = "2026-10-23T20:00:00Z"
description = "Prune"

[[skills]]
id = "token-efficient-workflow"
name = "TEW"
version = "1.1.0"
category = "workflow"
description = "Prune"
dependencies = []
compatible_agents = ["antigravity-cli"]
triggers = ["test"]
files = ["skills/token-efficient-workflow/SKILL.md"]
""", encoding="utf-8")

        with self.assertRaises((OwnershipError, StateDriftError)):
            SkillInstaller.execute_install(
                "token-efficient-workflow",
                self.target_dir,
                self.allowed_roots,
                config=self.config,
                custom_registry=custom_reg,
            )

        # Principles is still 100% valid
        valid, _, _ = verify_skill_integrity(self.target_dir / "principles", expected_skill_id="principles")
        self.assertTrue(valid)


class TestPhase6SEC10AMutationBoundaries(unittest.TestCase):
    """SEC-10A: KeyboardInterrupt Injection at All Mutation Boundaries."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_p6_sec10a_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.target_dir = self.root / "boundary_test"
        self.target_dir.mkdir(parents=True, exist_ok=True)

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_keyboard_interrupt_during_write_file(self):
        """Inject KeyboardInterrupt during safe_atomic_write on step 1."""
        plan = MutationPlan(
            plan_id="plan_ki_write",
            target_dir=self.target_dir,
            steps=[
                MutationStep(MutationAction.WRITE_FILE, self.target_dir / "file1.txt", content=b"c1"),
                MutationStep(MutationAction.WRITE_FILE, self.target_dir / "file2.txt", content=b"c2"),
            ],
        )

        orig_write = SafeFS.safe_atomic_write

        def interrupt_write(*args, **kwargs):
            raise KeyboardInterrupt("Simulated interrupt during write")

        SafeFS.safe_atomic_write = interrupt_write
        try:
            with self.assertRaises(KeyboardInterrupt):
                TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        finally:
            SafeFS.safe_atomic_write = orig_write

        # Zero artifacts remaining
        self.assertFalse((self.target_dir / "file1.txt").exists())
        self.assertFalse((self.target_dir / JOURNAL_FILENAME).exists())

        # Lock released
        lock = ProcessLock(self.target_dir / TransactionEngine.LOCK_FILENAME, timeout=1.0, allowed_roots=self.allowed_roots)
        with lock:
            pass

    def test_keyboard_interrupt_after_write_before_record_applied(self):
        """Inject KeyboardInterrupt immediately after write completes, before record_step_applied."""
        plan = MutationPlan(
            plan_id="plan_ki_after_write",
            target_dir=self.target_dir,
            steps=[
                MutationStep(MutationAction.WRITE_FILE, self.target_dir / "f1.txt", content=b"content 1"),
                MutationStep(MutationAction.WRITE_FILE, self.target_dir / "f2.txt", content=b"content 2"),
            ],
        )

        orig_record_applied = DurableJournal.record_step_applied

        def interrupt_record(*args, **kwargs):
            raise KeyboardInterrupt("Simulated interrupt between write and record_applied")

        DurableJournal.record_step_applied = interrupt_record
        try:
            with self.assertRaises(KeyboardInterrupt):
                TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        finally:
            DurableJournal.record_step_applied = orig_record_applied

        # f1.txt was written then rolled back by KeyboardInterrupt handler
        self.assertFalse((self.target_dir / "f1.txt").exists())
        self.assertFalse((self.target_dir / JOURNAL_FILENAME).exists())

    def test_keyboard_interrupt_during_unlink_file(self):
        """Inject KeyboardInterrupt during safe_unlink_file."""
        target_file = self.target_dir / "to_delete.txt"
        target_file.write_text("delete me", encoding="utf-8")
        st = os.stat(target_file)
        sha = hashlib.sha256(b"delete me").hexdigest()

        plan = MutationPlan(
            plan_id="plan_ki_unlink",
            target_dir=self.target_dir,
            steps=[
                MutationStep(MutationAction.UNLINK_FILE, target_file, expected_dev=st.st_dev, expected_ino=st.st_ino, expected_sha=sha),
            ],
        )

        orig_unlink = SafeFS.safe_unlink_file

        def interrupt_unlink(*args, **kwargs):
            raise KeyboardInterrupt("Simulated interrupt during unlink")

        SafeFS.safe_unlink_file = interrupt_unlink
        try:
            with self.assertRaises(KeyboardInterrupt):
                TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        finally:
            SafeFS.safe_unlink_file = orig_unlink

        # Target file preserved
        self.assertTrue(target_file.is_file())
        self.assertEqual(target_file.read_text(encoding="utf-8"), "delete me")

    def test_keyboard_interrupt_during_create_dir(self):
        """Inject KeyboardInterrupt during directory creation."""
        new_dir = self.target_dir / "new_folder"
        plan = MutationPlan(
            plan_id="plan_ki_mkdir",
            target_dir=self.target_dir,
            steps=[
                MutationStep(MutationAction.CREATE_DIR, new_dir),
            ],
        )

        orig_validate = SafeFS.validate_path

        def interrupt_before_mkdir(p, roots, allow_symlinks=False):
            if str(p) == str(new_dir):
                raise KeyboardInterrupt("Simulated interrupt before mkdir")
            return orig_validate(p, roots, allow_symlinks)

        SafeFS.validate_path = interrupt_before_mkdir
        try:
            with self.assertRaises(KeyboardInterrupt):
                TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        finally:
            SafeFS.validate_path = orig_validate

        self.assertFalse(new_dir.exists())

    def test_keyboard_interrupt_during_rmdir(self):
        """Inject KeyboardInterrupt during safe_rmdir."""
        del_dir = self.target_dir / "dir_to_remove"
        del_dir.mkdir(parents=True, exist_ok=True)
        st = os.stat(del_dir)

        plan = MutationPlan(
            plan_id="plan_ki_rmdir",
            target_dir=self.target_dir,
            steps=[
                MutationStep(MutationAction.REMOVE_DIR, del_dir, expected_dev=st.st_dev, expected_ino=st.st_ino),
            ],
        )

        orig_rmdir = SafeFS.safe_rmdir

        def interrupt_rmdir(*args, **kwargs):
            raise KeyboardInterrupt("Simulated interrupt during rmdir")

        SafeFS.safe_rmdir = interrupt_rmdir
        try:
            with self.assertRaises(KeyboardInterrupt):
                TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        finally:
            SafeFS.safe_rmdir = orig_rmdir

        self.assertTrue(del_dir.is_dir())


class TestPhase6SEC10BCrashRecoveryBoundaries(unittest.TestCase):
    """SEC-10B: Subprocess SIGKILL and Crash Recovery at Mutation Boundaries."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_p6_sec10b_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.target_dir = self.root / "sigkill_test"
        self.target_dir.mkdir(parents=True, exist_ok=True)
        self.repo_root = Path(__file__).resolve().parent.parent

    def tearDown(self):
        self.temp_dir.cleanup()

    def _run_subprocess_crash_test(self, crash_hook_code: str, steps_code: str):
        """Helper to run a transaction in a subprocess and crash it with SIGKILL."""
        subproc_script = f"""
import os
import signal
import sys
from pathlib import Path

sys.path.insert(0, {repr(str(self.repo_root))})

from awb.safety import SafeFS
from awb.transactions import MutationPlan, MutationStep, MutationAction, TransactionEngine
from awb.transactions.journal import DurableJournal

target_dir = Path({repr(str(self.target_dir))})
roots = [Path({repr(str(self.root))})]

{crash_hook_code}

plan = MutationPlan(
    plan_id="crash_boundary_plan",
    target_dir=target_dir,
    steps={steps_code},
)

TransactionEngine.execute(plan, allowed_roots=roots)
"""
        proc = subprocess.run([sys.executable, "-c", subproc_script], capture_output=True, text=True)
        self.assertEqual(proc.returncode, -signal.SIGKILL, f"Subprocess should have exited with SIGKILL (-9). Output: {proc.stderr}")

    def test_sigkill_immediately_after_write_prepared(self):
        """SIGKILL immediately after write_prepared (0 steps executed, journal PREPARED)."""
        crash_hook = """
orig_write_prepared = DurableJournal.write_prepared
def crash_after_prepared(self, plan):
    orig_write_prepared(self, plan)
    os.kill(os.getpid(), signal.SIGKILL)
DurableJournal.write_prepared = crash_after_prepared
"""
        steps = "[MutationStep(MutationAction.WRITE_FILE, target_dir / 'f.txt', content=b'val')]"
        self._run_subprocess_crash_test(crash_hook, steps)

        journal_file = self.target_dir / JOURNAL_FILENAME
        self.assertTrue(journal_file.exists())
        self.assertFalse((self.target_dir / "f.txt").exists())

        # Recover from fresh process
        recovered, msg = DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)
        self.assertTrue(recovered)
        self.assertFalse(journal_file.exists())

        # Calling recover_if_needed again is safe and idempotent
        r2, m2 = DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)
        self.assertTrue(r2)
        self.assertIsNone(m2)

    def test_sigkill_after_first_step_applied_during_second_step(self):
        """SIGKILL after step 1 applied, during step 2."""
        crash_hook = """
orig_write = SafeFS.safe_atomic_write
step_count = [0]
def crash_on_step2(*args, **kwargs):
    step_count[0] += 1
    if step_count[0] == 2:
        os.kill(os.getpid(), signal.SIGKILL)
    return orig_write(*args, **kwargs)
SafeFS.safe_atomic_write = crash_on_step2
"""
        steps = """[
    MutationStep(MutationAction.WRITE_FILE, target_dir / 'file1.txt', content=b'step1 done'),
    MutationStep(MutationAction.WRITE_FILE, target_dir / 'file2.txt', content=b'step2 data'),
]"""
        self._run_subprocess_crash_test(crash_hook, steps)

        # file1 exists on disk, file2 does not
        self.assertTrue((self.target_dir / "file1.txt").exists())
        self.assertFalse((self.target_dir / "file2.txt").exists())

        # Fresh transaction executes on target_dir, auto-recovering
        clean_file = self.target_dir / "recovered.txt"
        clean_plan = MutationPlan(
            plan_id="clean_p",
            target_dir=self.target_dir,
            steps=[MutationStep(MutationAction.WRITE_FILE, clean_file, content=b"clean")],
        )
        res = TransactionEngine.execute(clean_plan, allowed_roots=self.allowed_roots)
        self.assertTrue(res.success)

        # file1 rolled back, clean_file present
        self.assertFalse((self.target_dir / "file1.txt").exists())
        self.assertTrue(clean_file.is_file())
        self.assertFalse((self.target_dir / JOURNAL_FILENAME).exists())


class TestPhase6JournalEdgeCasesAndContention(unittest.TestCase):
    """Journal Edge Cases, Tampering, Contention, and Silent Failure Prevention."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_p6_journal_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.target_dir = self.root / "journal_test"
        self.target_dir.mkdir(parents=True, exist_ok=True)
        self.journal_file = self.target_dir / JOURNAL_FILENAME

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_corrupted_json_journal_fails_closed(self):
        """Corrupted JSON fails closed with TransactionError."""
        self.journal_file.write_text("NOT_JSON{{{", encoding="utf-8")
        with self.assertRaises(TransactionError) as ctx:
            DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)
        self.assertIn("Malformed transaction journal", str(ctx.exception))

    def test_missing_status_fails_closed(self):
        """Journal missing status field fails closed."""
        self.journal_file.write_text(json.dumps({"target_dir": str(self.target_dir)}), encoding="utf-8")
        with self.assertRaises(TransactionError) as ctx:
            DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)
        self.assertIn("Unrecognized journal status", str(ctx.exception))

    def test_missing_target_dir_fails_closed(self):
        """Journal in PREPARED status missing target_dir field fails closed."""
        self.journal_file.write_text(json.dumps({"status": "PREPARED"}), encoding="utf-8")
        with self.assertRaises(TransactionError) as ctx:
            DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)
        self.assertIn("missing required 'target_dir' field", str(ctx.exception))

    def test_target_dir_mismatch_fails_closed(self):
        """Journal claiming different target directory fails closed with SafetyError."""
        data = {
            "status": "PREPARED",
            "target_dir": "/tmp/different_directory",
            "step_entries": [],
        }
        self.journal_file.write_text(json.dumps(data), encoding="utf-8")
        with self.assertRaises(SafetyError) as ctx:
            DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)
        self.assertIn("Journal target", str(ctx.exception))
        self.assertIn("mismatch", str(ctx.exception))

    def test_symlinked_journal_fails_closed(self):
        """Symlinked .awb.journal.json fails closed with SafetyError."""
        real_j = self.root / "real_j.json"
        real_j.write_text("{}", encoding="utf-8")
        self.journal_file.symlink_to(real_j)

        with self.assertRaises(SafetyError) as ctx:
            DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)
        self.assertIn("symlinked transaction journal", str(ctx.exception))

    def test_failed_recovery_never_silently_reports_success(self):
        """
        If recovery rollback encounters errors (e.g. state drift or failure),
        it must raise TransactionError and NEVER return (True, ...).
        The journal must remain on disk.
        """
        f = self.target_dir / "drifted.txt"
        f.write_text("drifted content", encoding="utf-8")

        data = {
            "status": "PREPARED",
            "target_dir": str(self.target_dir),
            "step_entries": [
                {
                    "action": MutationAction.WRITE_FILE.value,
                    "path": str(f),
                    "new_sha": "different_sha_than_drifted",
                }
            ],
        }
        self.journal_file.write_text(json.dumps(data), encoding="utf-8")

        with self.assertRaises(StateDriftError):
            DurableJournal.recover_if_needed(self.target_dir, self.allowed_roots)

        # Journal is preserved on disk for forensic inspection
        self.assertTrue(self.journal_file.is_file())

    def test_lock_contention_and_timeout(self):
        """
        When process A holds lock on sandbox, process B attempting transaction
        times out with LockTimeoutError.
        """
        lock_path = self.target_dir / TransactionEngine.LOCK_FILENAME
        lock_a = ProcessLock(lock_path, timeout=1.0, allowed_roots=self.allowed_roots)
        lock_a.acquire()
        try:
            lock_b = ProcessLock(lock_path, timeout=0.1, allowed_roots=self.allowed_roots)
            with self.assertRaises(LockTimeoutError):
                lock_b.acquire()
        finally:
            lock_a.release()

        # After A releases, B acquires immediately
        lock_b2 = ProcessLock(lock_path, timeout=1.0, allowed_roots=self.allowed_roots)
        with lock_b2:
            pass


class TestPhase6SEC11RollbackPathConfinement(unittest.TestCase):
    """SEC-11: Rollback Path Confinement and Boundary Shielding."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_p6_sec11_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_unlink_rollback_with_out_of_boundary_target(self):
        """Rollback of UNLINK_FILE targeting path outside allowed roots fails safely."""
        outside = Path("/tmp/outside_boundary_unlink.txt")
        journal = [
            {
                "action": MutationAction.UNLINK_FILE.value,
                "path": str(outside),
                "prior_content": b"forbidden write back",
            }
        ]

        errors = TransactionEngine._rollback(journal, self.allowed_roots)
        self.assertEqual(len(errors), 1)
        self.assertIn("Path traversal rejected", errors[0])
        self.assertFalse(outside.exists())

    def test_create_dir_rollback_with_out_of_boundary_target(self):
        """Rollback of CREATE_DIR targeting path outside allowed roots fails safely."""
        outside_dir = Path("/tmp/outside_boundary_dir")
        journal = [
            {
                "action": MutationAction.CREATE_DIR.value,
                "path": str(outside_dir),
                "created": True,
            }
        ]

        errors = TransactionEngine._rollback(journal, self.allowed_roots)
        self.assertEqual(len(errors), 1)
        self.assertIn("Path traversal rejected", errors[0])

    def test_step_execution_boundary_escape_fails_and_rolls_back(self):
        """
        A plan containing an out-of-boundary step fails at execution time
        under SEC-05, and previously applied steps roll back cleanly under SEC-11.
        """
        target_dir = self.root / "sandbox"
        target_dir.mkdir(parents=True, exist_ok=True)
        f1 = target_dir / "valid.txt"
        outside_f = Path("/tmp/unauthorized_escape.txt")

        plan = MutationPlan(
            plan_id="escape_plan",
            target_dir=target_dir,
            steps=[
                MutationStep(MutationAction.WRITE_FILE, f1, content=b"valid"),
                MutationStep(MutationAction.WRITE_FILE, outside_f, content=b"escape"),
            ],
        )

        res = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(res.success)
        self.assertTrue(res.rolled_back)
        self.assertFalse(f1.exists())
        self.assertFalse(outside_f.exists())


if __name__ == "__main__":
    unittest.main()
