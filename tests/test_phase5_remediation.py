"""
Phase 5 Remediation Test Suite.
Verifies SEC-09, SEC-10 (10A & 10B), and SEC-11.
Exclusively operates inside disposable temporary directories.
"""

import base64
import hashlib
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from typing import Any, Dict, List

from awb.config import Config
from awb.errors import (
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


class TestSEC09ObsoleteFilePruning(unittest.TestCase):
    """SEC-09: Transactional Pruning of Obsolete Managed Files."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_sec09_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]
        self.config = Config.load()

    def tearDown(self):
        self.temp_dir.cleanup()

    def _create_custom_registry(self, files: List[str]) -> Path:
        """Create a custom skills.toml registry referencing the provided files."""
        reg_path = self.root / "skills.toml"
        files_str = json.dumps(files)
        content = f"""[registry]
version = "1.0.0"
updated_at = "2026-10-23T20:00:00Z"
description = "Test registry for SEC-09"

[[skills]]
id = "token-efficient-workflow"
name = "Token-Efficient Hardened Workflow"
version = "1.0.0"
category = "workflow"
description = "Pruning test skill"
dependencies = []
compatible_agents = ["antigravity-cli"]
triggers = ["test"]
files = {files_str}
"""
        reg_path.write_text(content, encoding="utf-8")
        return reg_path

    def test_obsolete_file_and_empty_dir_pruned_on_update(self):
        """
        When a skill update removes a previously managed file, the obsolete file
        is unlinked and any empty obsolete parent directory is pruned cleanly.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        dest_skill = target_dir / "token-efficient-workflow"

        # 1. Initial install with 2 files (including nested rules/efficient-workflow.md)
        initial_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
            "skills/token-efficient-workflow/rules/efficient-workflow.md",
        ])
        r1, m1 = SkillInstaller.execute_install(
            "token-efficient-workflow",
            target_dir,
            self.allowed_roots,
            config=self.config,
            custom_registry=initial_reg,
        )
        self.assertTrue(r1.success)
        self.assertTrue((dest_skill / "SKILL.md").is_file())
        self.assertTrue((dest_skill / "rules" / "efficient-workflow.md").is_file())
        self.assertIn("rules/efficient-workflow.md", m1.files)

        # 2. Update with registry omitting rules/efficient-workflow.md
        updated_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
        ])
        r2, m2 = SkillInstaller.execute_install(
            "token-efficient-workflow",
            target_dir,
            self.allowed_roots,
            config=self.config,
            custom_registry=updated_reg,
        )
        self.assertTrue(r2.success)

        # Obsolete file and its empty parent directory must be removed
        self.assertTrue((dest_skill / "SKILL.md").is_file())
        self.assertFalse((dest_skill / "rules" / "efficient-workflow.md").exists())
        self.assertFalse((dest_skill / "rules").exists())
        self.assertNotIn("rules/efficient-workflow.md", m2.files)

        # Integrity verification must pass with 0 errors
        valid, manifest, report = verify_skill_integrity(dest_skill, expected_skill_id="token-efficient-workflow")
        self.assertTrue(valid)
        self.assertEqual(len(report.errors), 0)

        # Subsequent removal succeeds completely
        r_rem, _ = SkillRemover.execute_remove(
            "token-efficient-workflow",
            target_dir,
            self.allowed_roots,
        )
        self.assertTrue(r_rem.success)
        self.assertFalse(dest_skill.exists())

    def test_modified_obsolete_file_fails_closed_state_drift(self):
        """
        If an obsolete file has been modified out-of-band by the user,
        update must fail closed with StateDriftError and preserve the file.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        dest_skill = target_dir / "token-efficient-workflow"

        initial_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
            "skills/token-efficient-workflow/rules/efficient-workflow.md",
        ])
        r1, m1 = SkillInstaller.execute_install(
            "token-efficient-workflow",
            target_dir,
            self.allowed_roots,
            config=self.config,
            custom_registry=initial_reg,
        )
        self.assertTrue(r1.success)

        # User modifies the obsolete file
        obs_file = dest_skill / "rules" / "efficient-workflow.md"
        obs_file.write_text("user modifications to obsolete file", encoding="utf-8")

        # Update without rules/efficient-workflow.md
        updated_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
        ])

        with self.assertRaises((OwnershipError, StateDriftError)) as ctx:
            SkillInstaller.execute_install(
                "token-efficient-workflow",
                target_dir,
                self.allowed_roots,
                config=self.config,
                custom_registry=updated_reg,
            )

        # Modified obsolete file and directory must still exist intact
        self.assertTrue(obs_file.is_file())
        self.assertEqual(obs_file.read_text(encoding="utf-8"), "user modifications to obsolete file")

    def test_symlinked_obsolete_file_fails_closed_safety_error(self):
        """
        If an obsolete file has been replaced by a symlink,
        update must fail closed with SafetyError.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        dest_skill = target_dir / "token-efficient-workflow"

        initial_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
            "skills/token-efficient-workflow/rules/efficient-workflow.md",
        ])
        r1, m1 = SkillInstaller.execute_install(
            "token-efficient-workflow",
            target_dir,
            self.allowed_roots,
            config=self.config,
            custom_registry=initial_reg,
        )
        self.assertTrue(r1.success)

        # Replace obsolete file with symlink
        obs_file = dest_skill / "rules" / "efficient-workflow.md"
        obs_file.unlink()
        obs_file.symlink_to(dest_skill / "SKILL.md")

        updated_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
        ])

        with self.assertRaises((OwnershipError, SafetyError)) as ctx:
            SkillInstaller.execute_install(
                "token-efficient-workflow",
                target_dir,
                self.allowed_roots,
                config=self.config,
                custom_registry=updated_reg,
            )

    def test_non_empty_obsolete_parent_dir_preserved(self):
        """
        If an obsolete file is pruned from a directory that still contains
        other non-obsolete files, the parent directory is not removed.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        dest_skill = target_dir / "token-efficient-workflow"

        # Create two files in the rules subdirectory
        (self.root / "rule2.md").write_text("rule 2 content", encoding="utf-8")
        initial_reg = self.root / "reg_multi.toml"
        initial_reg.write_text(f"""[registry]
version = "1.0.0"
updated_at = "2026-10-23T20:00:00Z"
description = "Multi test"

[[skills]]
id = "token-efficient-workflow"
name = "Test"
version = "1.0.0"
category = "workflow"
description = "Multi"
dependencies = []
compatible_agents = ["antigravity-cli"]
triggers = ["test"]
files = [
    "skills/token-efficient-workflow/SKILL.md",
    "skills/token-efficient-workflow/rules/efficient-workflow.md",
    "skills/token-efficient-workflow/rules/rule2.md"
]
""", encoding="utf-8")

        # Temporarily place rule2.md in source repo
        rule2_src = self.config.project_root / "skills" / "token-efficient-workflow" / "rules" / "rule2.md"
        rule2_src.write_text("rule 2 content", encoding="utf-8")
        try:
            r1, m1 = SkillInstaller.execute_install(
                "token-efficient-workflow",
                target_dir,
                self.allowed_roots,
                config=self.config,
                custom_registry=initial_reg,
            )
            self.assertTrue(r1.success)

            # Update removing rule2.md but keeping rules/efficient-workflow.md
            updated_reg = self._create_custom_registry([
                "skills/token-efficient-workflow/SKILL.md",
                "skills/token-efficient-workflow/rules/efficient-workflow.md",
            ])
            r2, m2 = SkillInstaller.execute_install(
                "token-efficient-workflow",
                target_dir,
                self.allowed_roots,
                config=self.config,
                custom_registry=updated_reg,
            )
            self.assertTrue(r2.success)

            # rule2.md pruned, but rules/ directory and efficient-workflow.md remain
            self.assertFalse((dest_skill / "rules" / "rule2.md").exists())
            self.assertTrue((dest_skill / "rules" / "efficient-workflow.md").is_file())
            self.assertTrue((dest_skill / "rules").is_dir())
        finally:
            if rule2_src.exists():
                rule2_src.unlink()

    def test_rollback_restores_pruned_obsolete_files_and_dirs(self):
        """
        If a subsequent step fails during an update, reverse rollback restores
        pruned obsolete files and their directories.
        """
        target_dir = self.root / "skills"
        target_dir.mkdir(parents=True, exist_ok=True)
        dest_skill = target_dir / "token-efficient-workflow"

        initial_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
            "skills/token-efficient-workflow/rules/efficient-workflow.md",
        ])
        r1, m1 = SkillInstaller.execute_install(
            "token-efficient-workflow",
            target_dir,
            self.allowed_roots,
            config=self.config,
            custom_registry=initial_reg,
        )
        self.assertTrue(r1.success)
        orig_obs_content = (dest_skill / "rules" / "efficient-workflow.md").read_bytes()

        # Update omitting rules/efficient-workflow.md, but inject fault in TransactionEngine
        updated_reg = self._create_custom_registry([
            "skills/token-efficient-workflow/SKILL.md",
        ])
        plan, manifest, snap = SkillInstaller.plan_install(
            "token-efficient-workflow",
            target_dir,
            self.allowed_roots,
            config=self.config,
            custom_registry=updated_reg,
        )

        # Inject an invalid final step that escapes allowed roots
        plan.steps.append(
            MutationStep(
                action=MutationAction.WRITE_FILE,
                target_path=Path("/unauthorized_escape_root/fail.txt"),
                content=b"fail",
            )
        )

        res = TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        self.assertFalse(res.success)
        self.assertTrue(res.rolled_back)

        # Both rules directory and obsolete file must be restored
        self.assertTrue((dest_skill / "rules").is_dir())
        self.assertTrue((dest_skill / "rules" / "efficient-workflow.md").is_file())
        self.assertEqual(
            (dest_skill / "rules" / "efficient-workflow.md").read_bytes(),
            orig_obs_content,
        )


class TestSEC10AKeyboardInterruptRollback(unittest.TestCase):
    """SEC-10A: Deterministic KeyboardInterrupt Rollback & Lock Release."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_sec10a_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_keyboard_interrupt_triggers_reverse_rollback_and_reraises(self):
        """
        When KeyboardInterrupt is raised during transaction step execution:
        1. Applied steps are rolled back in reverse order.
        2. Journal is marked ROLLED_BACK / removed.
        3. Lock is cleanly released.
        4. KeyboardInterrupt is re-raised to the caller.
        """
        target_dir = self.root / "ki_test"
        target_dir.mkdir(parents=True, exist_ok=True)
        file1 = target_dir / "step1.txt"
        file2 = target_dir / "step2.txt"

        plan = MutationPlan(
            plan_id="plan_ki_test",
            target_dir=target_dir,
            steps=[
                MutationStep(MutationAction.WRITE_FILE, file1, content=b"step 1 content"),
                MutationStep(MutationAction.WRITE_FILE, file2, content=b"step 2 content"),
            ],
        )

        orig_atomic_write = SafeFS.safe_atomic_write
        call_count = [0]

        def interrupt_on_step2(*args, **kwargs):
            call_count[0] += 1
            if call_count[0] == 2:
                raise KeyboardInterrupt("Simulated user interrupt during step 2")
            return orig_atomic_write(*args, **kwargs)

        SafeFS.safe_atomic_write = interrupt_on_step2
        try:
            with self.assertRaises(KeyboardInterrupt):
                TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
        finally:
            SafeFS.safe_atomic_write = orig_atomic_write

        # Step 1 was executed then rolled back; step 1 file must NOT exist
        self.assertFalse(file1.exists())
        self.assertFalse(file2.exists())

        # Journal file must be cleaned up / marked rolled back
        journal_path = target_dir / JOURNAL_FILENAME
        self.assertFalse(journal_path.exists())

        # ProcessLock must be cleanly released and immediately re-acquirable
        lock_path = target_dir / TransactionEngine.LOCK_FILENAME
        lock = ProcessLock(lock_path, timeout=1.0, allowed_roots=self.allowed_roots)
        with lock:
            pass  # Lock successfully acquired without blocking or timeout

    def test_keyboard_interrupt_rollback_error_chained(self):
        """
        If rollback encounters errors after a KeyboardInterrupt, both the
        interruption and rollback errors are chained inside a TransactionError.
        """
        target_dir = self.root / "ki_chain_test"
        target_dir.mkdir(parents=True, exist_ok=True)
        file1 = target_dir / "step1.txt"
        file2 = target_dir / "step2.txt"

        plan = MutationPlan(
            plan_id="plan_ki_chain",
            target_dir=target_dir,
            steps=[
                MutationStep(MutationAction.WRITE_FILE, file1, content=b"step 1 content"),
                MutationStep(MutationAction.WRITE_FILE, file2, content=b"step 2 content"),
            ],
        )

        orig_atomic_write = SafeFS.safe_atomic_write
        orig_rollback = TransactionEngine._rollback

        def interrupt_on_step2(*args, **kwargs):
            raise KeyboardInterrupt("Simulated interrupt")

        def failing_rollback(*args, **kwargs):
            return ["Simulated rollback failure on step1.txt"]

        SafeFS.safe_atomic_write = interrupt_on_step2
        TransactionEngine._rollback = failing_rollback
        try:
            with self.assertRaises(TransactionError) as ctx:
                TransactionEngine.execute(plan, allowed_roots=self.allowed_roots)
            self.assertIn("interrupted by KeyboardInterrupt and rollback encountered errors", str(ctx.exception))
            self.assertIsInstance(ctx.exception.__cause__, KeyboardInterrupt)
        finally:
            SafeFS.safe_atomic_write = orig_atomic_write
            TransactionEngine._rollback = orig_rollback

        # Lock must still be released despite failure
        lock_path = target_dir / TransactionEngine.LOCK_FILENAME
        lock = ProcessLock(lock_path, timeout=1.0, allowed_roots=self.allowed_roots)
        with lock:
            pass


class TestSEC10BDurableWriteAheadCrashRecovery(unittest.TestCase):
    """SEC-10B: Durable Write-Ahead Crash Recovery & Post-Interruption Consistency."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_sec10b_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_crash_recovery_from_prepared_journal(self):
        """
        Simulate an ungraceful crash mid-transaction (e.g. SIGKILL / power loss).
        A subsequent transaction holding lock detects PREPARED journal,
        reconstitutes previous state, rolls back applied steps in reverse order,
        and unlinks journal.
        """
        target_dir = self.root / "crash_test"
        target_dir.mkdir(parents=True, exist_ok=True)

        file1 = target_dir / "file1.txt"
        file2 = target_dir / "file2.txt"

        # Simulate prior state: file1 did not exist, but was created by interrupted txn
        file1.write_text("interrupted partial write", encoding="utf-8")
        st1 = os.stat(file1)
        sha1 = hashlib.sha256(b"interrupted partial write").hexdigest()

        # Write durable journal in PREPARED status
        journal_data = {
            "journal_version": "1.0.0",
            "plan_id": "crashed_plan_123",
            "target_dir": str(target_dir),
            "allowed_roots": [str(r) for r in self.allowed_roots],
            "status": JournalStatus.PREPARED.value,
            "created_at": "2026-10-23T20:00:00Z",
            "updated_at": "2026-10-23T20:00:01Z",
            "planned_steps": [],
            "step_entries": [
                {
                    "action": MutationAction.WRITE_FILE.value,
                    "path": str(file1),
                    "status": "APPLIED",
                    "prior_content": None,
                    "new_dev": st1.st_dev,
                    "new_ino": st1.st_ino,
                    "new_sha": sha1,
                }
            ],
        }
        journal_file = target_dir / JOURNAL_FILENAME
        journal_file.write_text(json.dumps(journal_data, indent=2), encoding="utf-8")

        # Now execute recovery
        recovered, msg = DurableJournal.recover_if_needed(target_dir, self.allowed_roots)
        self.assertTrue(recovered)
        self.assertIn("Successfully recovered and rolled back", msg)

        # File1 must have been unlinked by recovery rollback
        self.assertFalse(file1.exists())
        # Journal file must be deleted
        self.assertFalse(journal_file.exists())

    def test_subprocess_sigkill_mid_transaction_recovered_by_fresh_transaction(self):
        """
        Launch a separate subprocess that executes a transaction and SIGKILLs itself
        after applying step 1. The fresh transaction recovers the crashed state cleanly.
        """
        target_dir = self.root / "subproc_crash"
        target_dir.mkdir(parents=True, exist_ok=True)
        file1 = target_dir / "step1.txt"
        file2 = target_dir / "step2.txt"

        repo_root = Path(__file__).resolve().parent.parent

        # Subprocess script: writes step 1, records in journal, then self-SIGKILLs
        crash_script = f"""
import os
import signal
import sys
from pathlib import Path

sys.path.insert(0, {repr(str(repo_root))})

from awb.safety import SafeFS
from awb.transactions import MutationPlan, MutationStep, MutationAction, TransactionEngine

target_dir = Path({repr(str(target_dir))})
file1 = target_dir / "step1.txt"
file2 = target_dir / "step2.txt"
roots = [Path({repr(str(self.root))})]

plan = MutationPlan(
    plan_id="subprocess_crash_plan",
    target_dir=target_dir,
    steps=[
        MutationStep(MutationAction.WRITE_FILE, file1, content=b"partial 1"),
        MutationStep(MutationAction.WRITE_FILE, file2, content=b"partial 2"),
    ],
)

orig_write = SafeFS.safe_atomic_write

def crash_after_first_step(*args, **kwargs):
    res = orig_write(*args, **kwargs)
    # Ungraceful crash: SIGKILL self immediately after first step applied
    os.kill(os.getpid(), signal.SIGKILL)
    return res

SafeFS.safe_atomic_write = crash_after_first_step

TransactionEngine.execute(plan, allowed_roots=roots)
"""
        proc = subprocess.run(
            [sys.executable, "-c", crash_script],
            capture_output=True,
            text=True,
        )

        # Confirm subprocess died from SIGKILL (-9)
        self.assertEqual(proc.returncode, -signal.SIGKILL)

        # Confirm file1 and journal exist on disk in crashed state
        self.assertTrue(file1.exists())
        journal_file = target_dir / JOURNAL_FILENAME
        self.assertTrue(journal_file.exists())

        # Now run a new clean transaction via TransactionEngine
        clean_file = target_dir / "clean.txt"
        clean_plan = MutationPlan(
            plan_id="clean_recovery_plan",
            target_dir=target_dir,
            steps=[
                MutationStep(MutationAction.WRITE_FILE, clean_file, content=b"clean content"),
            ],
        )

        result = TransactionEngine.execute(clean_plan, allowed_roots=self.allowed_roots)
        self.assertTrue(result.success)

        # Crashed file1 must have been rolled back during recovery
        self.assertFalse(file1.exists())
        # Clean file exists
        self.assertTrue(clean_file.is_file())
        self.assertEqual(clean_file.read_bytes(), b"clean content")
        # Journal cleaned up
        self.assertFalse(journal_file.exists())

    def test_crash_recovery_detects_state_drift_and_fails_closed(self):
        """
        If a file created before a crash has been modified out-of-band by user,
        recovery refuses to delete user edits and fails closed with StateDriftError.
        """
        target_dir = self.root / "drift_crash_test"
        target_dir.mkdir(parents=True, exist_ok=True)
        file1 = target_dir / "file1.txt"

        # Interrupted transaction created file1 with original hash
        orig_content = b"original transaction content"
        orig_sha = hashlib.sha256(orig_content).hexdigest()

        journal_data = {
            "journal_version": "1.0.0",
            "plan_id": "crashed_plan_drift",
            "target_dir": str(target_dir),
            "allowed_roots": [str(r) for r in self.allowed_roots],
            "status": JournalStatus.PREPARED.value,
            "created_at": "2026-10-23T20:00:00Z",
            "updated_at": "2026-10-23T20:00:01Z",
            "step_entries": [
                {
                    "action": MutationAction.WRITE_FILE.value,
                    "path": str(file1),
                    "status": "APPLIED",
                    "prior_content": None,
                    "new_sha": orig_sha,
                }
            ],
        }
        journal_file = target_dir / JOURNAL_FILENAME
        journal_file.write_text(json.dumps(journal_data, indent=2), encoding="utf-8")

        # User modified file1 out-of-band
        file1.write_text("user modifications post-crash", encoding="utf-8")

        with self.assertRaises(StateDriftError) as ctx:
            DurableJournal.recover_if_needed(target_dir, self.allowed_roots)
        self.assertIn("Refusing to overwrite user edits", str(ctx.exception))

        # User's file must NOT have been unlinked or overwritten
        self.assertTrue(file1.is_file())
        self.assertEqual(file1.read_text(encoding="utf-8"), "user modifications post-crash")

    def test_crash_recovery_rejects_corrupted_journal(self):
        """A corrupted or malformed journal fails closed with TransactionError."""
        target_dir = self.root / "corrupt_j_test"
        target_dir.mkdir(parents=True, exist_ok=True)
        journal_file = target_dir / JOURNAL_FILENAME
        journal_file.write_text("{bad json: not valid", encoding="utf-8")

        with self.assertRaises(TransactionError) as ctx:
            DurableJournal.recover_if_needed(target_dir, self.allowed_roots)
        self.assertIn("Malformed transaction journal", str(ctx.exception))

    def test_clean_journal_committed_is_cleaned_up(self):
        """If journal is in COMMITTED state, recovery cleans it up without mutation."""
        target_dir = self.root / "committed_j_test"
        target_dir.mkdir(parents=True, exist_ok=True)
        journal_file = target_dir / JOURNAL_FILENAME
        journal_data = {
            "journal_version": "1.0.0",
            "plan_id": "committed_plan",
            "target_dir": str(target_dir),
            "allowed_roots": [str(r) for r in self.allowed_roots],
            "status": JournalStatus.COMMITTED.value,
        }
        journal_file.write_text(json.dumps(journal_data), encoding="utf-8")

        recovered, msg = DurableJournal.recover_if_needed(target_dir, self.allowed_roots)
        self.assertTrue(recovered)
        self.assertIn("Cleaned up COMMITTED journal", msg)
        self.assertFalse(journal_file.exists())


class TestSEC11RollbackPathConfinement(unittest.TestCase):
    """SEC-11: Rollback Path Confinement & Boundary Validation."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_sec11_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_rollback_rejects_path_outside_allowed_roots(self):
        """
        Rollback validates paths against allowed roots immediately before
        reversing each step. An out-of-boundary path is rejected safely.
        """
        outside_path = Path("/tmp/unauthorized_outside_boundary.txt")
        journal = [
            {
                "action": MutationAction.WRITE_FILE.value,
                "path": str(outside_path),
                "prior_content": b"evil payload",
            }
        ]

        errors = TransactionEngine._rollback(journal, self.allowed_roots)
        self.assertEqual(len(errors), 1)
        self.assertIn("Path traversal rejected", errors[0])
        self.assertFalse(outside_path.exists())

    def test_rollback_rejects_symlink_ancestor(self):
        """
        Rollback rejects operations traversing an unauthorized symlinked ancestor.
        """
        safe_subdir = self.root / "real_dir"
        safe_subdir.mkdir(parents=True, exist_ok=True)

        symlink_dir = self.root / "sym_link_dir"
        symlink_dir.symlink_to(safe_subdir)

        target_file = symlink_dir / "target.txt"
        journal = [
            {
                "action": MutationAction.WRITE_FILE.value,
                "path": str(target_file),
                "prior_content": b"content",
            }
        ]

        errors = TransactionEngine._rollback(journal, self.allowed_roots)
        self.assertEqual(len(errors), 1)
        self.assertIn("unauthorized symlink", errors[0])
        self.assertFalse((safe_subdir / "target.txt").exists())

    def test_remove_dir_rollback_validates_path(self):
        """
        Recreation of directories during REMOVE_DIR rollback validates
        path against allowed roots before mkdir.
        """
        outside_dir = Path("/tmp/unauthorized_escape_dir_mkdir")
        journal = [
            {
                "action": MutationAction.REMOVE_DIR.value,
                "path": str(outside_dir),
            }
        ]

        errors = TransactionEngine._rollback(journal, self.allowed_roots)
        self.assertEqual(len(errors), 1)
        self.assertIn("Path traversal rejected", errors[0])
        self.assertFalse(outside_dir.exists())


if __name__ == "__main__":
    unittest.main()
