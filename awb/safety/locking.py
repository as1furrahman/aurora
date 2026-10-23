"""
awb.safety.locking
Cross-process advisory mutation locking for Agent Workbench.
Serializes concurrent AWB writers, prevents race conditions, and handles stale locks.
Zero third-party runtime dependencies.
"""

import errno
import fcntl
import json
import os
import socket
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Union

from .exceptions import LockError, LockTimeoutError, PathTraversalError


class ProcessLock:
    """
    POSIX cross-process advisory lock using fcntl.flock with timeout,
    stale-lock breaking, and post-acquire re-validation support.
    """

    DEFAULT_TIMEOUT = 10.0
    DEFAULT_STALE_THRESHOLD = 60.0

    def __init__(
        self,
        lock_path: Union[str, Path],
        timeout: float = DEFAULT_TIMEOUT,
        stale_threshold: float = DEFAULT_STALE_THRESHOLD,
        allowed_roots: Optional[List[Union[str, Path]]] = None,
    ):
        self.lock_path = Path(lock_path).resolve()
        self.timeout = timeout
        self.stale_threshold = stale_threshold
        self.allowed_roots = [Path(r).resolve() for r in allowed_roots] if allowed_roots else None
        self._fd: Optional[int] = None
        self._acquired = False

        if self.allowed_roots:
            in_boundary = any(
                self.lock_path == root or root in self.lock_path.parents
                for root in self.allowed_roots
            )
            if not in_boundary:
                raise PathTraversalError(
                    f"Lock file '{self.lock_path}' is outside allowed roots: {self.allowed_roots}"
                )

    def _read_lock_metadata(self) -> Dict[str, Any]:
        """Read and parse lock metadata from disk if present."""
        if not self.lock_path.is_file():
            return {}
        try:
            content = self.lock_path.read_text(encoding="utf-8")
            return json.loads(content) if content else {}
        except (OSError, json.JSONDecodeError):
            return {}

    def _is_stale(self, meta: Dict[str, Any]) -> bool:
        """
        Diagnostic helper for determining whether recorded lock metadata belongs
        to a dead PID or has timed out.

        NOTE ON SAFETY INVARIANT (SEC-08):
        This method is intentionally NOT invoked during acquire() to forcefully
        unlink or break the lock file. In Linux/POSIX, kernel advisory locks via
        fcntl.flock() are automatically released by the kernel when the holding
        process terminates (including crashes or SIGKILL). Attempting to unlink
        or recreate .awb.lock upon finding dead metadata creates an inode
        desynchronization race (SEC-01) with other waiting contenders. Contenders
        rely on kernel flock release and subsequent in-place metadata overwrite,
        keeping .awb.lock strictly persistent.
        """
        if not meta or "pid" not in meta:
            return False

        lock_pid = meta.get("pid")
        lock_host = meta.get("host")
        lock_time = meta.get("acquired_at", 0)

        current_host = socket.gethostname()
        if lock_host and lock_host != current_host:
            # Different host: only rely on timestamp threshold
            return (time.time() - lock_time) > self.stale_threshold

        # Same host: check if process is still alive
        if isinstance(lock_pid, int):
            try:
                os.kill(lock_pid, 0)
                # Process is alive: lock is NOT stale unless exceeded max threshold
                return False
            except OSError as e:
                if e.errno == errno.ESRCH:
                    # No such process: PID died, lock is definitely stale
                    return True
                if e.errno == errno.EPERM:
                    # Process exists but owned by another user: alive
                    return False

        return (time.time() - lock_time) > self.stale_threshold

    def acquire(self) -> None:
        """
        Acquire exclusive cross-process lock with timeout and inode re-verification.
        Guarantees all contenders synchronize on the identical active filesystem inode.
        """
        start_time = time.monotonic()
        parent_dir = self.lock_path.parent
        parent_dir.mkdir(parents=True, exist_ok=True)

        fd: Optional[int] = None

        while True:
            # 1. Open descriptor to target lock path if not already opened
            if fd is None:
                try:
                    fd = os.open(
                        str(self.lock_path),
                        os.O_RDWR | os.O_CREAT | getattr(os, "O_NOFOLLOW", 0),
                        0o600,
                    )
                except OSError as e:
                    elapsed = time.monotonic() - start_time
                    if elapsed >= self.timeout:
                        raise LockTimeoutError(
                            f"Timed out opening lock file '{self.lock_path}': {e}"
                        ) from e
                    time.sleep(0.05)
                    continue

            # 2. Attempt non-blocking flock on current descriptor
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
            except (OSError, BlockingIOError) as e:
                if e.errno not in (errno.EWOULDBLOCK, errno.EAGAIN):
                    if fd is not None:
                        os.close(fd)
                    raise LockError(f"Failed to acquire lock on '{self.lock_path}': {e}") from e

                # Check if lock file on disk holds metadata from a dead process
                meta = self._read_lock_metadata()

                # Check timeout
                elapsed = time.monotonic() - start_time
                if elapsed >= self.timeout:
                    if fd is not None:
                        os.close(fd)
                    holder_info = f" (held by PID {meta.get('pid')} on {meta.get('host')})" if meta else ""
                    raise LockTimeoutError(
                        f"Timed out after {self.timeout:.1f}s waiting for lock on '{self.lock_path}'{holder_info}"
                    )

                time.sleep(0.05)
                continue

            # 3. flock succeeded. CRUCIAL: Re-verify that locked descriptor matches active path on disk
            try:
                st_fd = os.fstat(fd)
                st_path = os.stat(str(self.lock_path), follow_symlinks=False)
            except OSError:
                # Path was unlinked or modified beneath us; unlock, close, and retry
                try:
                    fcntl.flock(fd, fcntl.LOCK_UN)
                except OSError:
                    pass
                os.close(fd)
                fd = None
                time.sleep(0.02)
                continue

            if st_fd.st_ino != st_path.st_ino or st_fd.st_dev != st_path.st_dev:
                # Inode mismatch: locked an unlinked/superseded inode. Release, close, and re-open.
                try:
                    fcntl.flock(fd, fcntl.LOCK_UN)
                except OSError:
                    pass
                os.close(fd)
                fd = None
                time.sleep(0.02)
                continue

            # 4. Verified on identical active filesystem inode
            self._fd = fd
            self._acquired = True

            # Write lock metadata and flush
            meta = {
                "pid": os.getpid(),
                "uid": os.getuid(),
                "host": socket.gethostname(),
                "acquired_at": time.time(),
            }
            try:
                os.ftruncate(fd, 0)
                os.lseek(fd, 0, os.SEEK_SET)
                payload = json.dumps(meta).encode("utf-8")
                os.write(fd, payload)
                os.fsync(fd)
            except OSError:
                pass
            return

    def release(self) -> None:
        """
        Release exclusive lock and close descriptor.
        Invariant: Lock file remains persistently in place to prevent split-brain inode desynchronization.
        """
        if not self._acquired or self._fd is None:
            return

        try:
            # Clear metadata and sync before releasing lock
            try:
                os.ftruncate(self._fd, 0)
                os.fsync(self._fd)
            except OSError:
                pass
            fcntl.flock(self._fd, fcntl.LOCK_UN)
        finally:
            try:
                os.close(self._fd)
            except OSError:
                pass
            self._fd = None
            self._acquired = False

    def revalidate(self, callback: Callable[[], Any]) -> Any:
        """
        Execute post-acquisition target state revalidation.
        Ensures filesystem assumptions still hold after serializing behind prior writers.
        """
        if not self._acquired:
            raise LockError("Cannot revalidate target state: lock is not acquired")
        return callback()

    def __enter__(self) -> "ProcessLock":
        self.acquire()
        return self

    def __exit__(self, exc_type, exc_val, exc_tb) -> None:
        self.release()
