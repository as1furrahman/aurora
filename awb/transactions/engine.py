"""
Centralized Transaction Engine for Agent Workbench.
Mandates cross-process ProcessLock acquisition, post-acquire revalidation,
and journaled atomic execution with automatic rollback on failure.
Zero third-party runtime dependencies.
"""

import hashlib
import json
import os
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Union

from awb.errors import StateDriftError, TransactionError
from awb.safety.filesystem import SafeFS
from awb.safety.locking import ProcessLock
from .journal import DurableJournal
from .models import MutationAction, MutationPlan, MutationStep, TransactionResult


class TransactionEngine:
    """
    Centralized execution engine for all filesystem mutations.
    Enforces mandatory ProcessLock acquisition and pre-mutation re-validation.
    """

    LOCK_FILENAME = ".awb.lock"

    @classmethod
    def execute(
        cls,
        plan: MutationPlan,
        allowed_roots: List[Union[str, Path]],
        revalidate_fn: Optional[Callable[[], bool]] = None,
    ) -> TransactionResult:
        """
        Execute a MutationPlan under exclusive cross-process lock.
        Re-validates target state after lock acquisition and before executing any mutation.
        """
        target_dir = plan.target_dir.resolve()
        lock_file = target_dir / cls.LOCK_FILENAME
        roots = [Path(r).resolve() for r in allowed_roots]

        # Invariant 1: Lock acquisition is mandatory
        lock = ProcessLock(lock_file, timeout=10.0, allowed_roots=roots)
        durable_journal = DurableJournal(target_dir, roots)

        applied_journal: List[Dict[str, Any]] = []

        with lock:
            # SEC-10B: Detect and recover any crashed/interrupted transaction first
            durable_journal.recover_if_needed(target_dir, roots)

            # Invariant 2: Mandatory post-acquire re-validation
            if revalidate_fn is not None:
                is_valid = lock.revalidate(revalidate_fn)
                if not is_valid:
                    raise TransactionError(
                        f"Post-acquire target state re-validation failed for plan '{plan.plan_id}'. "
                        "Filesystem state modified concurrently prior to lock acquisition; aborting."
                    )

            # SEC-10B: Persist durable PREPARED state before any filesystem mutation steps
            durable_journal.write_prepared(plan)

            # Invariant 3: Execute steps transactionally with rollback on failure
            for idx, step in enumerate(plan.steps):
                try:
                    # SEC-05: Mandatory execution-time boundary and symlink validation on every step target
                    SafeFS.validate_path(step.target_path, roots, allow_symlinks=False)

                    if step.action == MutationAction.WRITE_FILE:
                        # Capture prior state for rollback if file exists
                        prior_content = None
                        if step.target_path.is_file():
                            prior_read = SafeFS.safe_open_read(step.target_path, roots)
                            prior_content = prior_read["content"]

                        # WAL: Record intent BEFORE write
                        content_bytes = step.content if isinstance(step.content, bytes) else (step.content or "").encode("utf-8")
                        target_sha = hashlib.sha256(content_bytes).hexdigest()
                        intent = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                            "prior_content": prior_content,
                            "mode": step.mode,
                            "new_sha": target_sha,
                        }
                        durable_journal.record_step_intent(intent)

                        write_info = SafeFS.safe_atomic_write(
                            dest_path=step.target_path,
                            content=step.content or b"",
                            allowed_roots=roots,
                            mode=step.mode,
                        )
                        record = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                            "prior_content": prior_content,
                            "new_dev": write_info["dev"],
                            "new_ino": write_info["ino"],
                            "new_sha": write_info["sha256"],
                        }
                        applied_journal.append(record)
                        durable_journal.record_step_applied(record)

                    elif step.action == MutationAction.UNLINK_FILE:
                        # Capture content before unlinking for rollback
                        read_info = SafeFS.safe_open_read(step.target_path, roots)
                        # WAL: Record intent BEFORE destructive unlink
                        intent = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                            "prior_content": read_info["content"],
                            "prior_mode": read_info["mode"],
                        }
                        durable_journal.record_step_intent(intent)

                        SafeFS.safe_unlink_file(
                            target_path=step.target_path,
                            expected_dev=step.expected_dev or read_info["dev"],
                            expected_ino=step.expected_ino or read_info["ino"],
                            expected_sha=step.expected_sha or read_info["sha256"],
                            allowed_roots=roots,
                        )
                        record = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                            "prior_content": read_info["content"],
                            "prior_mode": read_info["mode"],
                        }
                        applied_journal.append(record)
                        durable_journal.record_step_applied(record)

                    elif step.action == MutationAction.CREATE_DIR:
                        # SEC-05: Explicit validation before mkdir
                        SafeFS.validate_path(step.target_path, roots, allow_symlinks=False)
                        intent = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                        }
                        durable_journal.record_step_intent(intent)

                        created = False
                        if not step.target_path.exists():
                            step.target_path.mkdir(parents=True, exist_ok=True)
                            created = True
                        record = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                            "created": created,
                        }
                        applied_journal.append(record)
                        durable_journal.record_step_applied(record)

                    elif step.action == MutationAction.REMOVE_DIR:
                        st = os.stat(step.target_path)
                        intent = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                            "mode": st.st_mode,
                        }
                        durable_journal.record_step_intent(intent)

                        SafeFS.safe_rmdir(
                            target_dir=step.target_path,
                            expected_dev=step.expected_dev or st.st_dev,
                            expected_ino=step.expected_ino or st.st_ino,
                            allowed_roots=roots,
                        )
                        record = {
                            "action": step.action.value,
                            "path": str(step.target_path),
                            "mode": st.st_mode,
                        }
                        applied_journal.append(record)
                        durable_journal.record_step_applied(record)

                except KeyboardInterrupt as ki:
                    # SEC-10A: Interruption triggers reverse rollback, cleans journal, and re-raises
                    rollback_errors = cls._rollback(applied_journal, roots)
                    durable_journal.mark_rolled_back()
                    if rollback_errors:
                        raise TransactionError(
                            f"Transaction '{plan.plan_id}' interrupted by KeyboardInterrupt and rollback encountered errors: {rollback_errors}"
                        ) from ki
                    raise

                except Exception as step_exc:
                    # Rollback applied steps in reverse order
                    rollback_errors = cls._rollback(applied_journal, roots)
                    durable_journal.mark_rolled_back()
                    err_msg = f"Transaction step #{idx} ({step.action.value} on {step.target_path}) failed: {step_exc}"
                    if rollback_errors:
                        err_msg += f" (Rollback errors: {rollback_errors})"
                    return TransactionResult(
                        success=False,
                        plan_id=plan.plan_id,
                        applied_count=len(applied_journal),
                        rolled_back=True,
                        error=err_msg,
                    )

            # Invariant 4: Commit transaction and clean up durable journal
            durable_journal.mark_committed()
            return TransactionResult(
                success=True,
                plan_id=plan.plan_id,
                applied_count=len(applied_journal),
                rolled_back=False,
                error=None,
            )

    @classmethod
    def _rollback(cls, journal: List[Dict[str, Any]], roots: List[Path]) -> List[str]:
        """Rollback applied steps in reverse order with strict allowed-root confinement (SEC-11)."""
        errors: List[str] = []
        for record in reversed(journal):
            action = record.get("action")
            p = Path(record["path"])
            try:
                # SEC-11: Mandatory path validation immediately before every rollback operation
                SafeFS.validate_path(p, roots, allow_symlinks=False)

                if action == MutationAction.WRITE_FILE.value:
                    prior = record.get("prior_content")
                    if prior is not None:
                        SafeFS.safe_atomic_write(p, prior, roots)
                    else:
                        if p.exists():
                            st = os.stat(p)
                            new_sha = record.get("new_sha")
                            if not new_sha:
                                with open(p, "rb") as f:
                                    new_sha = hashlib.sha256(f.read()).hexdigest()
                            SafeFS.safe_unlink_file(
                                target_path=p,
                                expected_dev=record.get("new_dev", st.st_dev),
                                expected_ino=record.get("new_ino", st.st_ino),
                                expected_sha=new_sha,
                                allowed_roots=roots,
                            )
                elif action == MutationAction.UNLINK_FILE.value:
                    prior = record.get("prior_content")
                    mode = record.get("prior_mode", 0o600)
                    if prior is not None:
                        SafeFS.safe_atomic_write(p, prior, roots, mode=mode)
                elif action == MutationAction.CREATE_DIR.value:
                    if record.get("created") and p.is_dir():
                        st = os.stat(p)
                        SafeFS.safe_rmdir(p, st.st_dev, st.st_ino, roots)
                elif action == MutationAction.REMOVE_DIR.value:
                    SafeFS.validate_path(p, roots, allow_symlinks=False)
                    if not p.exists():
                        p.mkdir(parents=True, exist_ok=True)
                        mode = record.get("mode")
                        if mode is not None:
                            try:
                                os.chmod(p, mode)
                            except OSError:
                                pass
            except Exception as e:
                errors.append(f"Rollback failed on {p}: {e}")
        return errors
