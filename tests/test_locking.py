"""
Unit tests for awb.safety.locking.ProcessLock.
Verifies mutual exclusion, timeout, stale-lock recovery, and post-acquire revalidation.
"""

import os
import tempfile
import time
import unittest
from pathlib import Path

from awb.safety.exceptions import LockError, LockTimeoutError, PathTraversalError
from awb.safety.locking import ProcessLock


class TestProcessLock(unittest.TestCase):
    """Test suite for cross-process mutation locking."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_test_lock_")
        self.root = Path(self.temp_dir.name).resolve()

    def tearDown(self):
        self.temp_dir.cleanup()

    def test_basic_acquire_and_release(self):
        """Lock acquires cleanly, writes metadata, and remains persistent on release to avoid split-brain."""
        lock_file = self.root / ".awb.lock"
        lock = ProcessLock(lock_file, timeout=2.0)

        with lock:
            self.assertTrue(lock_file.is_file())
            meta = lock._read_lock_metadata()
            self.assertEqual(meta["pid"], os.getpid())
            self.assertEqual(meta["uid"], os.getuid())

        # Invariant: Lock file remains persistently in place to prevent inode desynchronization
        self.assertTrue(lock_file.is_file())

        # Assert another lock can acquire it immediately
        lock2 = ProcessLock(lock_file, timeout=1.0)
        with lock2:
            self.assertTrue(lock2._acquired)

    def test_inode_desynchronization_detection_and_safe_recovery(self):
        """
        Adversarial test for SEC-01:
        Verify that if .awb.lock is replaced or unlinked while a contender is waiting,
        the contender detects the inode mismatch and re-synchronizes on the active inode.
        """
        import fcntl
        lock_file = self.root / ".awb.lock"

        # 1. Process A creates and holds lock on Inode 1
        lock1 = ProcessLock(lock_file, timeout=2.0)
        lock1.acquire()
        st1 = os.stat(lock_file)

        # 2. Simulate external unlinking and recreation of lock file with Inode 2
        lock_file.unlink()
        lock_file.write_text("{}", encoding="utf-8")
        st2 = os.stat(lock_file)
        self.assertNotEqual(st1.st_ino, st2.st_ino)

        # 3. Process A releases its lock on Inode 1
        lock1.release()

        # 4. Process B acquires lock: must synchronize on active Inode 2
        lock2 = ProcessLock(lock_file, timeout=1.0)
        with lock2:
            st_locked = os.fstat(lock2._fd)
            st_disk = os.stat(lock_file)
            self.assertEqual(st_locked.st_ino, st2.st_ino)
            self.assertEqual(st_locked.st_ino, st_disk.st_ino)

    def test_contested_lock_times_out(self):
        """Contested lock fails with LockTimeoutError after specified timeout."""
        lock_file = self.root / ".awb.lock"
        lock1 = ProcessLock(lock_file, timeout=2.0)
        lock2 = ProcessLock(lock_file, timeout=0.2)

        lock1.acquire()
        try:
            with self.assertRaises(LockTimeoutError) as ctx:
                lock2.acquire()
            self.assertIn("Timed out after 0.2s", str(ctx.exception))
        finally:
            lock1.release()

    def test_stale_lock_is_broken_when_pid_is_dead(self):
        """Stale lock held by a nonexistent PID is detected and safely broken."""
        lock_file = self.root / ".awb.lock"

        # Manually create a fake stale lock file with an impossible/dead PID
        # Using a very large PID unlikely to exist
        dead_pid = 999999
        import json
        import socket
        stale_meta = {
            "pid": dead_pid,
            "uid": os.getuid(),
            "host": socket.gethostname(),
            "acquired_at": time.time() - 100,
        }
        lock_file.write_text(json.dumps(stale_meta), encoding="utf-8")

        lock = ProcessLock(lock_file, timeout=1.0, stale_threshold=10.0)
        # Should detect dead PID and acquire lock cleanly
        with lock:
            meta = lock._read_lock_metadata()
            self.assertEqual(meta["pid"], os.getpid())

    def test_revalidation_callback_executes_under_lock(self):
        """revalidate() executes callback and returns state under active lock."""
        lock_file = self.root / ".awb.lock"
        target_file = self.root / "state.txt"
        target_file.write_text("initial", encoding="utf-8")

        lock = ProcessLock(lock_file, timeout=2.0)
        with lock:
            result = lock.revalidate(lambda: target_file.read_text(encoding="utf-8"))
            self.assertEqual(result, "initial")

        # Calling revalidate outside context raises LockError
        with self.assertRaises(LockError):
            lock.revalidate(lambda: True)

    def test_lock_enforces_path_boundaries(self):
        """ProcessLock rejects lock files placed outside allowed boundaries."""
        outside = Path("/tmp/outside_lock.lock")
        with self.assertRaises(PathTraversalError):
            ProcessLock(outside, allowed_roots=[self.root])


if __name__ == "__main__":
    unittest.main()
