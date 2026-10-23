"""
Durable Write-Ahead Transaction Journal (.awb.journal.json) for Agent Workbench.
Guarantees crash recovery, write-ahead durability, and post-interruption consistency.
Zero third-party runtime dependencies.
"""

import base64
import hashlib
import json
import os
import stat
import tempfile
from datetime import datetime, timezone
from enum import Enum
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple

from awb.errors import (
    SafetyError,
    StateDriftError,
    TransactionError,
)
from awb.safety.filesystem import SafeFS
from awb.transactions.models import MutationAction, MutationPlan, MutationStep

JOURNAL_FILENAME = ".awb.journal.json"
JOURNAL_VERSION = "1.0.0"


class JournalStatus(str, Enum):
    """Lifecycle status of a durable transaction journal."""
    PREPARED = "PREPARED"
    COMMITTED = "COMMITTED"
    ROLLED_BACK = "ROLLED_BACK"


class DurableJournal:
    """
    Durable on-disk write-ahead journal tracking transaction state, intended mutations,
    and prior states for crash recovery (SEC-10B).
    """

    def __init__(self, target_dir: Path, allowed_roots: List[Path]):
        self.target_dir = Path(target_dir).resolve()
        self.allowed_roots = [Path(r).resolve() for r in allowed_roots]
        self.journal_path = self.target_dir / JOURNAL_FILENAME
        self.plan_id: Optional[str] = None
        self.status: JournalStatus = JournalStatus.PREPARED
        self.step_entries: List[Dict[str, Any]] = []

    def write_prepared(self, plan: MutationPlan) -> None:
        """
        Persist initial PREPARED state and plan metadata before any filesystem mutations.
        Uses atomic staged write and directory fsync.
        """
        self.plan_id = plan.plan_id
        self.status = JournalStatus.PREPARED
        self.step_entries = []

        serialized_steps = []
        for s in plan.steps:
            serialized_steps.append({
                "action": s.action.value,
                "target_path": str(s.target_path),
                "expected_dev": s.expected_dev,
                "expected_ino": s.expected_ino,
                "expected_sha": s.expected_sha,
                "mode": s.mode,
            })

        data = {
            "journal_version": JOURNAL_VERSION,
            "plan_id": plan.plan_id,
            "target_dir": str(self.target_dir),
            "allowed_roots": [str(r) for r in self.allowed_roots],
            "status": self.status.value,
            "created_at": datetime.now(timezone.utc).isoformat(),
            "updated_at": datetime.now(timezone.utc).isoformat(),
            "planned_steps": serialized_steps,
            "step_entries": [],
        }

        self._atomic_save(data)

    def record_step_intent(self, intent_record: Dict[str, Any]) -> None:
        """
        Write-Ahead Log: Record step intent and prior state BEFORE performing destructive operation.
        Flushes and fsyncs metadata to disk so any crash mid-operation can be safely rolled back.
        """
        rec_copy = dict(intent_record)
        prior_content = rec_copy.get("prior_content")
        if isinstance(prior_content, bytes):
            rec_copy["prior_content_b64"] = base64.b64encode(prior_content).decode("ascii")
            rec_copy.pop("prior_content", None)

        rec_copy["status"] = "IN_PROGRESS"
        self.step_entries.append(rec_copy)

        data = self._read_raw()
        if not data:
            data = {
                "journal_version": JOURNAL_VERSION,
                "plan_id": self.plan_id,
                "target_dir": str(self.target_dir),
                "allowed_roots": [str(r) for r in self.allowed_roots],
                "status": self.status.value,
                "created_at": datetime.now(timezone.utc).isoformat(),
            }
        data["updated_at"] = datetime.now(timezone.utc).isoformat()
        data["status"] = self.status.value
        data["step_entries"] = self.step_entries

        self._atomic_save(data)

    def record_step_applied(self, applied_info: Dict[str, Any]) -> None:
        """
        Mark the in-progress step as APPLIED with resulting dev, ino, and sha.
        """
        if self.step_entries:
            last = self.step_entries[-1]
            last["status"] = "APPLIED"
            info_copy = dict(applied_info)
            prior = info_copy.get("prior_content")
            if isinstance(prior, bytes):
                info_copy["prior_content_b64"] = base64.b64encode(prior).decode("ascii")
                info_copy.pop("prior_content", None)
            last.update(info_copy)

            data = self._read_raw()
            if data:
                data["updated_at"] = datetime.now(timezone.utc).isoformat()
                data["step_entries"] = self.step_entries
                self._atomic_save(data)

    def mark_committed(self) -> None:
        """Mark transaction as COMMITTED and clean up journal file."""
        self.status = JournalStatus.COMMITTED
        if self.journal_path.exists():
            try:
                self.journal_path.unlink()
            except OSError:
                pass

    def mark_rolled_back(self) -> None:
        """Mark transaction as ROLLED_BACK and clean up journal file."""
        self.status = JournalStatus.ROLLED_BACK
        if self.journal_path.exists():
            try:
                self.journal_path.unlink()
            except OSError:
                pass

    def _read_raw(self) -> Dict[str, Any]:
        """Read journal JSON from disk."""
        if not self.journal_path.is_file():
            return {}
        try:
            return json.loads(self.journal_path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return {}

    def _atomic_save(self, data: Dict[str, Any]) -> None:
        """
        Write journal to temporary file in same directory, fsync, atomic replace,
        and fsync parent directory to guarantee metadata persistence.
        """
        def _default_serial(obj: Any) -> Any:
            if isinstance(obj, bytes):
                return base64.b64encode(obj).decode("ascii")
            raise TypeError(f"Object of type {type(obj)} is not JSON serializable")

        payload = json.dumps(data, indent=2, default=_default_serial).encode("utf-8")
        parent_dir = self.journal_path.parent
        parent_dir.mkdir(parents=True, exist_ok=True)

        # Staged write
        fd, tmp_path = tempfile.mkstemp(dir=parent_dir, prefix=".awb_jstage_")
        try:
            os.write(fd, payload)
            os.fchmod(fd, 0o600)
            os.fsync(fd)
            os.close(fd)
            fd = None

            os.replace(tmp_path, self.journal_path)

            # fsync parent directory
            pfd = os.open(str(parent_dir), os.O_RDONLY | getattr(os, "O_DIRECTORY", 0))
            try:
                os.fsync(pfd)
            finally:
                os.close(pfd)
        except Exception:
            if fd is not None:
                os.close(fd)
            if os.path.exists(tmp_path):
                try:
                    os.unlink(tmp_path)
                except OSError:
                    pass
            raise

    @classmethod
    def recover_if_needed(cls, target_dir: Path, allowed_roots: List[Path]) -> Tuple[bool, Optional[str]]:
        """
        Inspect target directory for an interrupted transaction journal.
        If found in PREPARED state, validates safety invariants and executes
        deterministic reverse rollback (SEC-10B).
        Fails closed if state drift or corruption is detected.
        """
        roots = [Path(r).resolve() for r in allowed_roots]
        canon_target = SafeFS.validate_path(target_dir, roots, allow_symlinks=False)
        target_path = Path(canon_target)
        journal_file = target_path / JOURNAL_FILENAME

        if not journal_file.exists():
            return True, None

        if not journal_file.is_file() or journal_file.is_symlink():
            raise SafetyError(f"Corrupted or symlinked transaction journal detected at '{journal_file}'")

        try:
            content = journal_file.read_text(encoding="utf-8")
            data = json.loads(content)
        except Exception as e:
            raise TransactionError(f"Malformed transaction journal at '{journal_file}': {e}. Fails closed.")

        if not isinstance(data, dict):
            raise TransactionError(f"Invalid transaction journal structure at '{journal_file}'")

        status = data.get("status")
        plan_id = data.get("plan_id", "unknown")
        entries = data.get("step_entries") or data.get("applied_records") or []

        if status in (JournalStatus.COMMITTED.value, JournalStatus.ROLLED_BACK.value):
            # Cleanly finished before crash; safe to unlink
            try:
                journal_file.unlink()
            except OSError:
                pass
            return True, f"Cleaned up {status} journal for plan '{plan_id}'"

        if status != JournalStatus.PREPARED.value:
            raise TransactionError(f"Unrecognized journal status '{status}' at '{journal_file}'")

        # Status is PREPARED: an interrupted transaction crashed mid-execution
        if not data.get("target_dir"):
            raise TransactionError(f"Journal at '{journal_file}' missing required 'target_dir' field")

        journal_target = Path(data["target_dir"]).resolve()
        if journal_target != target_path:
            raise SafetyError(
                f"Journal target '{journal_target}' mismatch with current target '{target_path}'"
            )

        if not isinstance(entries, list):
            raise TransactionError(f"Journal step entries must be a list at '{journal_file}'")

        # Reconstitute entries and verify no unexpected user state drift
        reconstituted_journal: List[Dict[str, Any]] = []
        for rec in entries:
            if not isinstance(rec, dict) or "path" not in rec or "action" not in rec:
                raise TransactionError(f"Malformed step entry in journal at '{journal_file}'")

            rec_copy = dict(rec)
            b64_val = rec_copy.get("prior_content_b64")
            if b64_val is not None:
                rec_copy["prior_content"] = base64.b64decode(b64_val.encode("ascii"))
            reconstituted_journal.append(rec_copy)

            p = Path(rec["path"])
            SafeFS.validate_path(p, roots, allow_symlinks=False)

            action = rec.get("action")
            if action == MutationAction.WRITE_FILE.value:
                if p.exists():
                    st = os.stat(p)
                    hasher = hashlib.sha256()
                    with open(p, "rb") as f:
                        while chunk := f.read(65536):
                            hasher.update(chunk)
                    current_sha = hasher.hexdigest()
                    expected_new_sha = rec.get("new_sha")
                    prior_content = rec_copy.get("prior_content")
                    expected_prior_sha = hashlib.sha256(prior_content).hexdigest() if prior_content else None

                    # If file on disk has unexpected edits by third party, fail closed
                    if (
                        expected_new_sha
                        and current_sha != expected_new_sha
                        and current_sha != expected_prior_sha
                    ):
                        raise StateDriftError(
                            f"Crash recovery aborted: file '{p}' was modified out-of-band "
                            f"(current SHA {current_sha} != expected {expected_new_sha}). Refusing to overwrite user edits."
                        )

        # Perform reverse rollback
        from awb.transactions.engine import TransactionEngine
        rollback_errors = TransactionEngine._rollback(reconstituted_journal, roots)
        if rollback_errors:
            raise TransactionError(
                f"Crash recovery rollback for plan '{plan_id}' encountered errors: {rollback_errors}"
            )

        # Clean up journal after successful recovery
        try:
            journal_file.unlink()
        except OSError:
            pass

        return True, f"Successfully recovered and rolled back interrupted transaction '{plan_id}'"
