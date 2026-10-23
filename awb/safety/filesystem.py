"""
awb.safety.filesystem
Hardened, deterministic filesystem safety primitives for Agent Workbench.
"""

import errno
import hashlib
import os
import stat
import tempfile
from pathlib import Path
from typing import Any, Dict, List, Optional, Set, Union

from .exceptions import (
    DescriptorValidationError,
    FIFOSubstitutionError,
    PathTraversalError,
    SafetyError,
    StateDriftError,
    SymlinkAncestorError,
)


class SafeFS:
    """Deterministic, descriptor-relative filesystem safety controls."""

    MAX_FILE_SIZE = 65536  # 64 KB safety bound for manifests and skills

    @staticmethod
    def canonicalize_path(p: Union[str, Path]) -> str:
        """Resolve full canonical path with user expansion."""
        return os.path.realpath(os.path.expanduser(str(p)))

    @classmethod
    def validate_path(
        cls,
        target_path: Union[str, Path],
        allowed_roots: List[Union[str, Path]],
        allow_symlinks: bool = False,
    ) -> str:
        """
        Validate that target_path stays strictly within at least one allowed root,
        rejecting path traversal escapes and unauthorized ancestor symlinks.
        """
        raw_str = os.path.expanduser(str(target_path))
        abs_target = os.path.abspath(raw_str)
        canon_target = os.path.realpath(abs_target)

        canon_roots = [cls.canonicalize_path(r) for r in allowed_roots]

        # 1. Boundary check against permitted roots
        in_boundary = False
        for root in canon_roots:
            if canon_target == root or canon_target.startswith(root.rstrip(os.sep) + os.sep):
                in_boundary = True
                break

        if not in_boundary:
            raise PathTraversalError(
                f"Path traversal rejected: '{target_path}' resolves to '{canon_target}' "
                f"which is outside allowed boundaries: {canon_roots}"
            )

        # 2. Check for symlinks in ancestry if disallowed
        if not allow_symlinks:
            # Walk from target parent down to root to check for symlinked ancestor components
            curr = abs_target
            # Check leaf if it exists
            if os.path.lexists(curr) and os.path.islink(curr):
                raise SymlinkAncestorError(f"Target path is an unauthorized symlink: '{curr}'")

            parent = os.path.dirname(curr)
            while parent and parent != curr:
                if os.path.lexists(parent) and os.path.islink(parent):
                    raise SymlinkAncestorError(
                        f"Ancestor directory is an unauthorized symlink: '{parent}' (target: '{target_path}')"
                    )
                # Bounded traversal: stop checking once we reach the canonical allowed root boundary
                if any(parent == r for r in canon_roots):
                    break
                curr = parent
                parent = os.path.dirname(curr)

        return canon_target

    @classmethod
    def safe_open_read(
        cls,
        target_path: Union[str, Path],
        allowed_roots: List[Union[str, Path]],
        max_size: int = MAX_FILE_SIZE,
    ) -> Dict[str, Any]:
        """
        Safely open and read a regular file using non-blocking descriptor inspection
        (O_NONBLOCK | O_NOFOLLOW), eliminating FIFO / named pipe blocking hazards.
        """
        canon_path = cls.validate_path(target_path, allowed_roots, allow_symlinks=False)
        parent_dir = os.path.dirname(canon_path)
        basename = os.path.basename(canon_path)

        # Open immediate parent with O_DIRECTORY | O_NOFOLLOW
        try:
            parent_fd = os.open(
                parent_dir,
                os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
            )
        except OSError as e:
            raise DescriptorValidationError(f"Cannot open parent directory '{parent_dir}': {e}") from e

        try:
            st_p = os.fstat(parent_fd)
            if not stat.S_ISDIR(st_p.st_mode):
                raise DescriptorValidationError(f"Parent '{parent_dir}' descriptor is not a directory")
            if st_p.st_uid != os.getuid():
                raise DescriptorValidationError(
                    f"Parent '{parent_dir}' owner UID mismatch (expected {os.getuid()}, got {st_p.st_uid})"
                )

            # Open target file non-blocking with O_NOFOLLOW relative to parent
            flags = os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
            try:
                file_fd = os.open(basename, flags, dir_fd=parent_fd)
            except OSError as e:
                if e.errno == getattr(errno, "ELOOP", 40):
                    raise SymlinkAncestorError(f"Target file '{basename}' is a symlink: {e}") from e
                raise DescriptorValidationError(f"Cannot open file '{canon_path}': {e}") from e

            try:
                st = os.fstat(file_fd)

                # Strictly fail closed on non-regular files (FIFOs, sockets, device nodes)
                if stat.S_ISFIFO(st.st_mode):
                    raise FIFOSubstitutionError(f"FIFO detected at '{canon_path}'; refused to read")
                if not stat.S_ISREG(st.st_mode):
                    raise FIFOSubstitutionError(
                        f"Non-regular file node (mode {oct(st.st_mode)}) at '{canon_path}'"
                    )

                # Validate ownership
                if st.st_uid != os.getuid():
                    raise DescriptorValidationError(
                        f"File '{canon_path}' owner mismatch (expected {os.getuid()}, got {st.st_uid})"
                    )

                # Validate bounded size
                if st.st_size > max_size:
                    raise DescriptorValidationError(
                        f"File '{canon_path}' exceeds maximum size bound ({st.st_size} > {max_size} bytes)"
                    )

                # Bounded read
                raw_chunks = []
                total_read = 0
                hasher = hashlib.sha256()
                while True:
                    chunk = os.read(file_fd, 65536)
                    if not chunk:
                        break
                    total_read += len(chunk)
                    if total_read > max_size:
                        raise DescriptorValidationError(
                            f"File '{canon_path}' streamed bytes exceeded max size limit ({max_size})"
                        )
                    raw_chunks.append(chunk)
                    hasher.update(chunk)

                content = b"".join(raw_chunks)
                sha = hasher.hexdigest()

                return {
                    "path": canon_path,
                    "content": content,
                    "dev": st.st_dev,
                    "ino": st.st_ino,
                    "uid": st.st_uid,
                    "mode": st.st_mode,
                    "size": len(content),
                    "sha256": sha,
                }
            finally:
                os.close(file_fd)
        finally:
            os.close(parent_fd)

    @classmethod
    def safe_atomic_write(
        cls,
        dest_path: Union[str, Path],
        content: Union[bytes, str],
        allowed_roots: List[Union[str, Path]],
        mode: int = 0o600,
        _fault_at: Optional[str] = None,
    ) -> Dict[str, Any]:
        """
        Atomically serialize and write content to dest_path via staged write,
        fchmod, fsync, atomic os.replace, and parent directory fsync.
        Guarantees zero partially-written or truncated visible states.

        _fault_at: Optional test seam for failure injection ('after_create',
                   'during_write', 'before_fsync', 'before_replace').
        """
        raw_bytes = content.encode("utf-8") if isinstance(content, str) else content

        # Validate destination within allowed roots
        canon_dest = cls.validate_path(dest_path, allowed_roots, allow_symlinks=False)
        parent_dir = os.path.dirname(canon_dest)

        if not os.path.isdir(parent_dir):
            raise DescriptorValidationError(f"Parent directory '{parent_dir}' does not exist")

        # Open immediate parent directory
        parent_fd = os.open(
            parent_dir,
            os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
        )
        try:
            st_p = os.fstat(parent_fd)
            if not stat.S_ISDIR(st_p.st_mode) or st_p.st_uid != os.getuid():
                raise DescriptorValidationError(f"Parent directory '{parent_dir}' not a user-owned directory")

            # Create temporary staging file in same parent directory (guarantees same filesystem)
            tmp_fd, tmp_path = tempfile.mkstemp(dir=parent_dir, prefix=".awb_stage_")
            try:
                if _fault_at == "after_create":
                    raise IOError("Fault injection: failure after staging file creation")

                # Loop write
                total_written = 0
                while total_written < len(raw_bytes):
                    if _fault_at == "during_write":
                        if len(raw_bytes) > 0:
                            os.write(tmp_fd, raw_bytes[:1])
                        raise IOError("Fault injection: failure during payload write loop")
                    n = os.write(tmp_fd, raw_bytes[total_written:])
                    if n == 0:
                        raise OSError("Zero bytes written during atomic file write")
                    total_written += n

                os.fchmod(tmp_fd, mode)

                if _fault_at == "before_fsync":
                    raise IOError("Fault injection: failure before file descriptor sync")
                os.fsync(tmp_fd)

                st_tmp = os.fstat(tmp_fd)
                tmp_dev = st_tmp.st_dev
                tmp_ino = st_tmp.st_ino
                os.close(tmp_fd)
                tmp_fd = None

                if _fault_at == "before_replace":
                    raise IOError("Fault injection: failure immediately before atomic replace")

                # Atomic replace on same filesystem
                os.replace(tmp_path, canon_dest)

                # Flush parent directory entry to disk
                try:
                    os.fsync(parent_fd)
                except OSError:
                    pass

                hasher = hashlib.sha256(raw_bytes)
                return {
                    "path": canon_dest,
                    "dev": tmp_dev,
                    "ino": tmp_ino,
                    "sha256": hasher.hexdigest(),
                    "size": len(raw_bytes),
                    "mode": mode,
                }
            finally:
                if tmp_fd is not None:
                    try:
                        os.close(tmp_fd)
                    except OSError:
                        pass
                if os.path.lexists(tmp_path):
                    try:
                        os.unlink(tmp_path)
                    except OSError:
                        pass
        finally:
            os.close(parent_fd)

    @classmethod
    def safe_unlink_file(
        cls,
        target_path: Union[str, Path],
        expected_dev: int,
        expected_ino: int,
        expected_sha: str,
        allowed_roots: List[Union[str, Path]],
        _race_hook: Optional[Any] = None,
    ) -> bool:
        """
        Descriptor-relative file removal with strict identity and content verification:
        - Pins parent directory descriptor (O_DIRECTORY | O_NOFOLLOW).
        - Opens child with O_NONBLOCK | O_NOFOLLOW relative to parent descriptor.
        - Verifies fstat identity (S_ISREG, UID, dev, ino, SHA-256).
        - Re-checks directory entry immediately before unlinkat.
        - Fails closed on any identity drift.

        _race_hook: Optional callable invoked immediately before the pre-unlink
                    recheck, simulating a competing concurrent modification race.
        """
        canon_path = cls.validate_path(target_path, allowed_roots, allow_symlinks=False)
        parent_dir = os.path.dirname(canon_path)
        basename = os.path.basename(canon_path)

        try:
            parent_fd = os.open(
                parent_dir,
                os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
            )
        except OSError as e:
            if e.errno == errno.ENOENT:
                return False
            raise DescriptorValidationError(f"Cannot open parent directory '{parent_dir}': {e}") from e

        try:
            st_p = os.fstat(parent_fd)
            if not stat.S_ISDIR(st_p.st_mode) or st_p.st_uid != os.getuid():
                raise DescriptorValidationError(f"Parent directory '{parent_dir}' not user-owned directory")

            file_flags = os.O_RDONLY | getattr(os, "O_NONBLOCK", 0) | getattr(os, "O_NOFOLLOW", 0)
            try:
                file_fd = os.open(basename, file_flags, dir_fd=parent_fd)
            except OSError as e:
                if e.errno == errno.ENOENT:
                    return False
                if e.errno == getattr(errno, "ELOOP", 40):
                    raise SymlinkAncestorError(f"Target '{basename}' is a symlink; refused to unlink")
                raise DescriptorValidationError(f"Cannot open '{canon_path}': {e}") from e

            try:
                st = os.fstat(file_fd)
                if stat.S_ISFIFO(st.st_mode):
                    raise FIFOSubstitutionError(f"FIFO substituted at '{canon_path}'; unlinking refused")
                if not stat.S_ISREG(st.st_mode):
                    raise StateDriftError(f"Target '{canon_path}' is not a regular file (mode {oct(st.st_mode)})")
                if st.st_uid != os.getuid():
                    raise StateDriftError(f"Target '{canon_path}' owner mismatch (got {st.st_uid})")
                if st.st_dev != expected_dev or st.st_ino != expected_ino:
                    raise StateDriftError(
                        f"Target '{canon_path}' device/inode mismatch "
                        f"(expected {expected_dev}:{expected_ino}, got {st.st_dev}:{st.st_ino})"
                    )

                hasher = hashlib.sha256()
                while True:
                    chunk = os.read(file_fd, 65536)
                    if not chunk:
                        break
                    hasher.update(chunk)
                if hasher.hexdigest() != expected_sha:
                    raise StateDriftError(f"Target '{canon_path}' SHA-256 mismatch before unlink")
            finally:
                os.close(file_fd)

            # Optional race hook simulating concurrent substitution before unlinkat
            if _race_hook is not None:
                _race_hook()

            # Pre-unlink stat re-check on directory entry
            st_fresh = os.stat(basename, dir_fd=parent_fd, follow_symlinks=False)
            if (st_fresh.st_dev != expected_dev or
                st_fresh.st_ino != expected_ino or
                st_fresh.st_uid != os.getuid()):
                raise StateDriftError(f"Target '{canon_path}' identity drifted before unlinkat")

            os.unlink(basename, dir_fd=parent_fd)
            return True
        finally:
            os.close(parent_fd)

    @classmethod
    def safe_rmdir(
        cls,
        target_dir: Union[str, Path],
        expected_dev: int,
        expected_ino: int,
        allowed_roots: List[Union[str, Path]],
    ) -> bool:
        """
        Descriptor-relative directory removal:
        - Pins parent directory descriptor (O_DIRECTORY | O_NOFOLLOW).
        - Re-checks entry identity (S_ISDIR, not symlink, UID, dev, ino).
        - Removes directory using os.rmdir(basename, dir_fd=parent_fd) (unlinkat AT_REMOVEDIR).
        """
        canon_dir = cls.validate_path(target_dir, allowed_roots, allow_symlinks=False)
        parent_dir = os.path.dirname(canon_dir)
        basename = os.path.basename(canon_dir)

        try:
            parent_fd = os.open(
                parent_dir,
                os.O_RDONLY | getattr(os, "O_DIRECTORY", 0) | getattr(os, "O_NOFOLLOW", 0),
            )
        except OSError as e:
            if e.errno == errno.ENOENT:
                return False
            raise DescriptorValidationError(f"Cannot open parent directory '{parent_dir}': {e}") from e

        try:
            st_p = os.fstat(parent_fd)
            if not stat.S_ISDIR(st_p.st_mode) or st_p.st_uid != os.getuid():
                raise DescriptorValidationError(f"Parent directory '{parent_dir}' not user-owned directory")

            try:
                st = os.stat(basename, dir_fd=parent_fd, follow_symlinks=False)
            except OSError as e:
                if e.errno == errno.ENOENT:
                    return False
                raise DescriptorValidationError(f"Cannot stat directory entry '{basename}': {e}") from e

            if stat.S_ISLNK(st.st_mode):
                raise SymlinkAncestorError(f"Directory entry '{canon_dir}' is a symlink; refusing removal")
            if not stat.S_ISDIR(st.st_mode):
                raise StateDriftError(f"Entry '{canon_dir}' is not a directory; refusing removal")
            if st.st_uid != os.getuid():
                raise StateDriftError(f"Directory '{canon_dir}' owner mismatch (got {st.st_uid})")
            if st.st_dev != expected_dev or st.st_ino != expected_ino:
                raise StateDriftError(
                    f"Directory '{canon_dir}' identity mismatch "
                    f"(expected {expected_dev}:{expected_ino}, got {st.st_dev}:{st.st_ino})"
                )

            os.rmdir(basename, dir_fd=parent_fd)
            return True
        finally:
            os.close(parent_fd)
