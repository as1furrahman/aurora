"""
Deterministic adversarial test suite for awb.safety primitives.
Verifies all 5 safety-test gates with adversarial rejection cases and positive controls.
"""

import errno
import hashlib
import os
import stat
import tempfile
import unittest
from pathlib import Path

from awb.safety.exceptions import (
    DescriptorValidationError,
    FIFOSubstitutionError,
    PathTraversalError,
    SafetyError,
    StateDriftError,
    SymlinkAncestorError,
)
from awb.safety.filesystem import SafeFS


class TestSafetyPrimitives(unittest.TestCase):
    """Adversarial and positive control tests for SafeFS safety primitives."""

    def setUp(self):
        self.temp_dir = tempfile.TemporaryDirectory(prefix="awb_test_safety_")
        self.root = os.path.realpath(self.temp_dir.name)
        self.allowed_roots = [self.root]

    def tearDown(self):
        self.temp_dir.cleanup()

    # --------------------------------------------------------------------------
    # GATE 1: ANCESTOR SYMLINKS
    # --------------------------------------------------------------------------
    def test_gate1_rejects_symlinked_ancestor_directory(self):
        """Reject paths when an ancestor directory along the chain is a symlink."""
        real_parent = os.path.join(self.root, "real_parent")
        os.makedirs(real_parent, exist_ok=True)
        sub_dir = os.path.join(real_parent, "child")
        os.makedirs(sub_dir, exist_ok=True)

        symlink_ancestor = os.path.join(self.root, "sym_parent")
        os.symlink(real_parent, symlink_ancestor)

        # Path through the symlinked ancestor
        traversal_path = os.path.join(symlink_ancestor, "child", "file.txt")

        with self.assertRaises(SymlinkAncestorError):
            SafeFS.validate_path(traversal_path, self.allowed_roots, allow_symlinks=False)

    def test_gate1_rejects_symlinked_leaf_target(self):
        """Reject paths when the target leaf itself is an unauthorized symlink."""
        real_file = os.path.join(self.root, "real_target.txt")
        with open(real_file, "w") as fp:
            fp.write("legitimate content")

        symlink_leaf = os.path.join(self.root, "sym_target.txt")
        os.symlink(real_file, symlink_leaf)

        with self.assertRaises(SymlinkAncestorError):
            SafeFS.validate_path(symlink_leaf, self.allowed_roots, allow_symlinks=False)

    def test_gate1_positive_control_regular_ancestry(self):
        """Positive control: valid deeply nested paths with regular ancestry succeed."""
        nested = os.path.join(self.root, "a", "b", "c")
        os.makedirs(nested, exist_ok=True)
        valid_path = os.path.join(nested, "manifest.json")
        with open(valid_path, "w") as fp:
            fp.write("ok")

        result = SafeFS.validate_path(valid_path, self.allowed_roots, allow_symlinks=False)
        self.assertEqual(result, os.path.realpath(valid_path))

    # --------------------------------------------------------------------------
    # GATE 2: PATH TRAVERSAL
    # --------------------------------------------------------------------------
    def test_gate2_rejects_dot_dot_traversal_escape(self):
        """Reject classical '..' traversal that escapes the permitted root."""
        sub = os.path.join(self.root, "subdir")
        os.makedirs(sub, exist_ok=True)

        escape_path = os.path.join(sub, "..", "..", "outside.txt")
        with self.assertRaises(PathTraversalError):
            SafeFS.validate_path(escape_path, [sub], allow_symlinks=False)

    def test_gate2_rejects_unauthorized_absolute_path(self):
        """Reject absolute paths outside declared permitted roots."""
        unauthorized = "/etc/passwd"
        with self.assertRaises(PathTraversalError):
            SafeFS.validate_path(unauthorized, self.allowed_roots, allow_symlinks=False)

    def test_gate2_positive_control_nested_boundary(self):
        """Positive control: nested paths staying within boundary succeed."""
        allowed_sub = os.path.join(self.root, "safe_zone")
        os.makedirs(allowed_sub, exist_ok=True)
        target = os.path.join(allowed_sub, "file.txt")
        with open(target, "w") as fp:
            fp.write("content")

        res = SafeFS.validate_path(target, [allowed_sub], allow_symlinks=False)
        self.assertEqual(res, os.path.realpath(target))

    # --------------------------------------------------------------------------
    # GATE 3: FIFO SUBSTITUTION & NON-BLOCKING INSPECTION
    # --------------------------------------------------------------------------
    def test_gate3_safe_open_read_rejects_fifo_non_blocking(self):
        """Never block on a substituted FIFO and fail closed immediately."""
        fifo_path = os.path.join(self.root, "test.fifo")
        os.mkfifo(fifo_path, 0o600)

        # safe_open_read must return immediately with FIFOSubstitutionError and not hang
        with self.assertRaises(FIFOSubstitutionError):
            SafeFS.safe_open_read(fifo_path, self.allowed_roots)

    def test_gate3_safe_unlink_rejects_fifo_substitution(self):
        """Reject unlinking when a target has been substituted with a FIFO."""
        fifo_path = os.path.join(self.root, "victim.fifo")
        os.mkfifo(fifo_path, 0o600)

        dummy_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        with self.assertRaises(FIFOSubstitutionError):
            SafeFS.safe_unlink_file(
                fifo_path,
                expected_dev=0,
                expected_ino=0,
                expected_sha=dummy_hash,
                allowed_roots=self.allowed_roots,
            )

    def test_gate3_positive_control_regular_read(self):
        """Positive control: normal regular files are read and hashed cleanly."""
        target = os.path.join(self.root, "regular.txt")
        payload = b"Hello Agent Workbench SafeFS\n"
        with open(target, "wb") as fp:
            fp.write(payload)

        info = SafeFS.safe_open_read(target, self.allowed_roots)
        self.assertEqual(info["content"], payload)
        self.assertEqual(info["sha256"], hashlib.sha256(payload).hexdigest())
        self.assertEqual(info["size"], len(payload))

    # --------------------------------------------------------------------------
    # GATE 4: CONCURRENT MODIFICATION & IDENTITY DRIFT
    # --------------------------------------------------------------------------
    def test_gate4_rejects_inode_substitution(self):
        """
        Detect when file identity (inode) has changed before unlinking.
        Deterministic test: allocates a replacement file while original is still present,
        guaranteeing the kernel allocates a distinct inode, then atomically replaces original.
        """
        target = os.path.join(self.root, "replaceable.txt")
        replacement = os.path.join(self.root, "replacement_stage.txt")
        payload = b"Initial content"
        write_info = SafeFS.safe_atomic_write(target, payload, self.allowed_roots)

        # Allocate replacement while target still exists (guarantees different inode)
        with open(replacement, "wb") as fp:
            fp.write(payload)
        st_rep = os.stat(replacement)
        self.assertNotEqual(st_rep.st_ino, write_info["ino"])

        # Atomically replace target with replacement
        os.replace(replacement, target)
        self.assertEqual(os.stat(target).st_ino, st_rep.st_ino)

        # safe_unlink_file must reject with StateDriftError deterministically
        with self.assertRaises(StateDriftError):
            SafeFS.safe_unlink_file(
                target,
                expected_dev=write_info["dev"],
                expected_ino=write_info["ino"],
                expected_sha=write_info["sha256"],
                allowed_roots=self.allowed_roots,
            )

    def test_gate4_rejects_content_hash_drift(self):
        """Detect when file content has been altered before unlinking."""
        target = os.path.join(self.root, "tampered.txt")
        payload = b"Authentic content"
        write_info = SafeFS.safe_atomic_write(target, payload, self.allowed_roots)

        # Modify content in-place (same inode, modified content)
        with open(target, "r+b") as fp:
            fp.write(b"Malicious change!")

        with self.assertRaises(StateDriftError):
            SafeFS.safe_unlink_file(
                target,
                expected_dev=write_info["dev"],
                expected_ino=write_info["ino"],
                expected_sha=write_info["sha256"],
                allowed_roots=self.allowed_roots,
            )

    def test_gate4_rejects_concurrent_replacement_during_pre_unlink_window(self):
        """
        Explicit concurrent race condition test:
        Simulates a competing process substituting the leaf target between descriptor
        open/verification and the final pre-unlink directory entry recheck.
        The recheck must detect identity drift and abort with StateDriftError.
        """
        target = os.path.join(self.root, "raced_target.txt")
        competing_donor = os.path.join(self.root, "competing_donor.txt")
        payload = b"Authentic verified payload"
        write_info = SafeFS.safe_atomic_write(target, payload, self.allowed_roots)

        # Competing donor allocated in advance
        with open(competing_donor, "wb") as fp:
            fp.write(payload)

        def competing_process_race_action():
            # Competing process swaps target immediately before pre-unlink recheck
            os.replace(competing_donor, target)

        with self.assertRaises(StateDriftError):
            SafeFS.safe_unlink_file(
                target,
                expected_dev=write_info["dev"],
                expected_ino=write_info["ino"],
                expected_sha=write_info["sha256"],
                allowed_roots=self.allowed_roots,
                _race_hook=competing_process_race_action,
            )

        # Verify that the substituted file was NOT unlinked because the race check failed safe
        self.assertTrue(os.path.exists(target))

    def test_gate4_positive_control_verified_unlink(self):
        """Positive control: file unlinks cleanly when dev, inode, and hash match."""
        target = os.path.join(self.root, "unlink_me.txt")
        payload = b"Clean unlinking payload"
        write_info = SafeFS.safe_atomic_write(target, payload, self.allowed_roots)

        self.assertTrue(os.path.exists(target))
        success = SafeFS.safe_unlink_file(
            target,
            expected_dev=write_info["dev"],
            expected_ino=write_info["ino"],
            expected_sha=write_info["sha256"],
            allowed_roots=self.allowed_roots,
        )
        self.assertTrue(success)
        self.assertFalse(os.path.exists(target))

    # --------------------------------------------------------------------------
    # GATE 5: INTERRUPTED WRITES & CRASH RESILIENCE
    # --------------------------------------------------------------------------
    def test_gate5_atomic_write_preserves_original_on_failure(self):
        """Verify that an interrupted write leaves the original file uncorrupted."""
        dest = os.path.join(self.root, "critical_state.json")
        original_payload = b'{"status": "pristine"}'
        SafeFS.safe_atomic_write(dest, original_payload, self.allowed_roots)

        # Attempt atomic write with invalid path boundary (triggers error before replace)
        with self.assertRaises(PathTraversalError):
            SafeFS.safe_atomic_write(
                "/tmp/unauthorized_destination_outside_root.json",
                b'{"status": "corrupt"}',
                self.allowed_roots,
            )

        # Assert original destination file remains 100% pristine and uncorrupted
        read_back = SafeFS.safe_open_read(dest, self.allowed_roots)
        self.assertEqual(read_back["content"], original_payload)

    def test_gate5_failure_injection_after_create(self):
        """Failure injection immediately after staging file creation."""
        dest = os.path.join(self.root, "state_after_create.txt")
        SafeFS.safe_atomic_write(dest, b"Pristine", self.allowed_roots)

        with self.assertRaises(IOError):
            SafeFS.safe_atomic_write(dest, b"Corrupt", self.allowed_roots, _fault_at="after_create")

        # Destination preserved, staging artifacts cleaned up
        self.assertEqual(SafeFS.safe_open_read(dest, self.allowed_roots)["content"], b"Pristine")
        staging_artifacts = [e for e in os.listdir(self.root) if e.startswith(".awb_stage_")]
        self.assertEqual(staging_artifacts, [])

    def test_gate5_failure_injection_during_write(self):
        """Failure injection during staging payload write loop."""
        dest = os.path.join(self.root, "state_during_write.txt")
        SafeFS.safe_atomic_write(dest, b"Pristine", self.allowed_roots)

        with self.assertRaises(IOError):
            SafeFS.safe_atomic_write(dest, b"Corrupt Payload Longer", self.allowed_roots, _fault_at="during_write")

        # Destination preserved, staging artifacts cleaned up
        self.assertEqual(SafeFS.safe_open_read(dest, self.allowed_roots)["content"], b"Pristine")
        staging_artifacts = [e for e in os.listdir(self.root) if e.startswith(".awb_stage_")]
        self.assertEqual(staging_artifacts, [])

    def test_gate5_failure_injection_before_fsync(self):
        """Failure injection before file descriptor fsync."""
        dest = os.path.join(self.root, "state_before_fsync.txt")
        SafeFS.safe_atomic_write(dest, b"Pristine", self.allowed_roots)

        with self.assertRaises(IOError):
            SafeFS.safe_atomic_write(dest, b"Corrupt", self.allowed_roots, _fault_at="before_fsync")

        # Destination preserved, staging artifacts cleaned up
        self.assertEqual(SafeFS.safe_open_read(dest, self.allowed_roots)["content"], b"Pristine")
        staging_artifacts = [e for e in os.listdir(self.root) if e.startswith(".awb_stage_")]
        self.assertEqual(staging_artifacts, [])

    def test_gate5_failure_injection_before_replace(self):
        """Failure injection immediately before atomic replace."""
        dest = os.path.join(self.root, "state_before_replace.txt")
        SafeFS.safe_atomic_write(dest, b"Pristine", self.allowed_roots)

        with self.assertRaises(IOError):
            SafeFS.safe_atomic_write(dest, b"Corrupt", self.allowed_roots, _fault_at="before_replace")

        # Destination preserved, staging artifacts cleaned up
        self.assertEqual(SafeFS.safe_open_read(dest, self.allowed_roots)["content"], b"Pristine")
        staging_artifacts = [e for e in os.listdir(self.root) if e.startswith(".awb_stage_")]
        self.assertEqual(staging_artifacts, [])

    def test_gate5_atomic_write_leaves_no_staging_artifacts_on_clean_run(self):
        """Atomic write cleans up temporary staging files completely."""
        dest = os.path.join(self.root, "atomic_output.txt")
        SafeFS.safe_atomic_write(dest, b"Complete payload", self.allowed_roots)

        # Check directory entries: only atomic_output.txt should exist, no .awb_stage_* left behind
        entries = os.listdir(self.root)
        self.assertIn("atomic_output.txt", entries)
        staging_artifacts = [e for e in entries if e.startswith(".awb_stage_")]
        self.assertEqual(staging_artifacts, [])

    def test_gate5_positive_control_rmdir_descriptor_relative(self):
        """Positive control: descriptor-relative directory pruning."""
        d = os.path.join(self.root, "empty_dir")
        os.makedirs(d, exist_ok=True)
        st_d = os.stat(d)

        success = SafeFS.safe_rmdir(
            d,
            expected_dev=st_d.st_dev,
            expected_ino=st_d.st_ino,
            allowed_roots=self.allowed_roots,
        )
        self.assertTrue(success)
        self.assertFalse(os.path.exists(d))


if __name__ == "__main__":
    unittest.main()
