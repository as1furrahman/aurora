"""
Unit tests for awb.transactions.TransactionEngine.
Verifies mandatory locking, post-acquire revalidation, atomic commit, and rollback.
"""

import tempfile
import unittest
from pathlib import Path

from awb.errors import TransactionError
from awb.safety.exceptions import LockTimeoutError
from awb.safety.locking import ProcessLock
from awb.transactions import (
    MutationAction,
    MutationPlan,
    MutationStep,
    TransactionEngine,
)


class TestTransactionEngine(unittest.TestCase):
    """Test suite for TransactionEngine."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_test_tx_")
        self.root = Path(self.temp_dir.name).resolve()
        self.allowed_roots = [self.root]

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_successful_transaction_commit(self):
        """Transaction executes all steps cleanly under lock."""
        file1 = self.root / "file1.txt"
        file2 = self.root / "file2.txt"

        plan = MutationPlan(
            plan_id="tx_test_commit",
            target_dir=self.root,
            steps=[
                MutationStep(action=MutationAction.WRITE_FILE, target_path=file1, content=b"First"),
                MutationStep(action=MutationAction.WRITE_FILE, target_path=file2, content=b"Second"),
            ],
        )

        result = TransactionEngine.execute(plan, self.allowed_roots)
        self.assertTrue(result.success)
        self.assertEqual(result.applied_count, 2)
        self.assertFalse(result.rolled_back)
        self.assertEqual(file1.read_bytes(), b"First")
        self.assertEqual(file2.read_bytes(), b"Second")

    def test_mandatory_locking_enforced_during_execution(self):
        """Verify that TransactionEngine holds exclusive ProcessLock during execution."""
        file1 = self.root / "file1.txt"
        lock_file = self.root / ".awb.lock"

        # Hook inside revalidation checking that lock is actively held
        def verify_lock_is_held():
            contender = ProcessLock(lock_file, timeout=0.1, allowed_roots=self.allowed_roots)
            with self.assertRaises(LockTimeoutError):
                contender.acquire()
            return True

        plan = MutationPlan(
            plan_id="tx_test_lock",
            target_dir=self.root,
            steps=[
                MutationStep(action=MutationAction.WRITE_FILE, target_path=file1, content=b"Locked"),
            ],
        )

        result = TransactionEngine.execute(
            plan,
            self.allowed_roots,
            revalidate_fn=verify_lock_is_held,
        )
        self.assertTrue(result.success)
        self.assertEqual(file1.read_bytes(), b"Locked")

    def test_post_acquire_revalidation_failure_aborts_without_mutation(self):
        """If target state changed concurrently and revalidation fails, transaction aborts with zero mutations."""
        file1 = self.root / "should_not_exist.txt"

        plan = MutationPlan(
            plan_id="tx_test_revalidate_fail",
            target_dir=self.root,
            steps=[
                MutationStep(action=MutationAction.WRITE_FILE, target_path=file1, content=b"Mutated"),
            ],
        )

        with self.assertRaises(TransactionError) as ctx:
            TransactionEngine.execute(
                plan,
                self.allowed_roots,
                revalidate_fn=lambda: False,  # Revalidation reports state drift
            )

        self.assertIn("Post-acquire target state re-validation failed", str(ctx.exception))
        self.assertFalse(file1.exists())

    def test_rollback_on_step_failure(self):
        """When an intermediate step fails, previously applied steps roll back completely."""
        file1 = self.root / "step1.txt"
        file2_invalid = Path("/unauthorized_outside_root/step2.txt")

        plan = MutationPlan(
            plan_id="tx_test_rollback",
            target_dir=self.root,
            steps=[
                MutationStep(action=MutationAction.WRITE_FILE, target_path=file1, content=b"Step 1 content"),
                MutationStep(action=MutationAction.WRITE_FILE, target_path=file2_invalid, content=b"Step 2 invalid"),
            ],
        )

        result = TransactionEngine.execute(plan, self.allowed_roots)
        self.assertFalse(result.success)
        self.assertTrue(result.rolled_back)
        self.assertIn("failed", result.error or "")

        # Verify step 1 was rolled back (file1 unlinked)
        self.assertFalse(file1.exists())


if __name__ == "__main__":
    unittest.main()
