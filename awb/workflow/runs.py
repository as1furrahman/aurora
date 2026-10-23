"""
Run directory and run-record persistence (ADR-0003).
Every write is boundary-checked against the run directory and staged atomically.
"""

import json
import os
import re
from contextlib import contextmanager
from pathlib import Path
from typing import Dict, Iterator, Optional

from awb.safety.boundaries import validate_security_preconditions
from awb.safety.filesystem import SafeFS
from awb.safety.locking import ProcessLock

from .model import StageState, WorkflowError

RUN_SCHEMA_VERSION = 1
RUN_KIND = "aurora.workflow-run"
RECORD_FILE = "run.json"
WORKFLOW_FILE = "workflow.toml"
LOCK_FILE = ".awb.lock"
RECORD_MAX_BYTES = 8 * 1024 * 1024
EVIDENCE_MAX_BYTES = 16 * 1024 * 1024
RUN_ID_PATTERN = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$")

STAGE_KEYS = {"state", "provider", "attempts", "awaiting_approval", "escalated", "handoff", "evidence",
              "criteria_met", "missing", "blocked_reasons", "approval", "not_applicable_reason"}
RECORD_KEYS = {"schema_version", "kind", "run_id", "created_at", "updated_at", "workflow", "plan",
               "stages", "events"}


class RunError(WorkflowError):
    """Invalid run operation (bad run root/id, wrong stage state, usage error)."""
    pass


class RunRecordError(RunError):
    """run.json is missing, unreadable, or violates the v1 record schema."""
    pass


class RunIntegrityError(RunError):
    """Pinned workflow or accepted evidence no longer matches its recorded hash."""
    pass


def validate_run_id(run_id: str) -> str:
    if not isinstance(run_id, str) or not RUN_ID_PATTERN.fullmatch(run_id) or run_id in (".", ".."):
        raise RunError(f"Invalid run id '{run_id}': use letters, digits, '.', '_' or '-', starting alphanumeric")
    return run_id


def create_run_dir(run_root: Path, run_id: str) -> Path:
    """Create <run_root>/<run_id> after boundary validation; never reuses an existing directory."""
    validate_run_id(run_id)
    root = Path(run_root)
    if not root.is_dir():
        raise RunError(f"Run root '{root}' does not exist or is not a directory; create it explicitly first")
    root = root.resolve()
    run_dir = root / run_id
    validate_security_preconditions(run_dir, [root])
    try:
        os.mkdir(run_dir, 0o700)
    except FileExistsError as e:
        raise RunError(f"Run directory '{run_dir}' already exists; refusing to reuse it") from e
    return run_dir


def ensure_dir(run_dir: Path, rel: str) -> Path:
    """Create a directory inside run_dir (each level boundary-checked)."""
    path = run_dir
    for part in Path(rel).parts:
        path = path / part
        validate_security_preconditions(path, [run_dir])
        if not path.is_dir():
            os.mkdir(path, 0o700)
    return path


def write_file(run_dir: Path, rel: str, content) -> None:
    dest = run_dir / rel
    validate_security_preconditions(dest, [run_dir])
    SafeFS.safe_atomic_write(dest, content, [run_dir])


def read_file(run_dir: Path, rel: str, max_size: int = RECORD_MAX_BYTES) -> dict:
    """Read a regular file inside run_dir (no symlinks, no FIFOs). Returns SafeFS read info."""
    return SafeFS.safe_open_read(run_dir / rel, [run_dir], max_size=max_size)


@contextmanager
def locked(run_dir: Path) -> Iterator[None]:
    with ProcessLock(run_dir / LOCK_FILE, allowed_roots=[run_dir]):
        yield


def save_record(run_dir: Path, record: dict) -> None:
    write_file(run_dir, RECORD_FILE, json.dumps(record, indent=2, sort_keys=True) + "\n")


def load_record(run_dir: Path) -> dict:
    run_dir = Path(run_dir).resolve()
    if not (run_dir / RECORD_FILE).is_file():
        raise RunRecordError(f"'{run_dir}' is not an Aurora run directory (missing {RECORD_FILE})")
    try:
        record = json.loads(read_file(run_dir, RECORD_FILE)["content"].decode("utf-8"))
    except (ValueError, UnicodeDecodeError) as e:
        raise RunRecordError(f"{RECORD_FILE} is not valid JSON: {e}") from e
    _validate_record(record)
    return record


def _validate_record(record) -> None:
    def fail(msg: str):
        raise RunRecordError(f"{RECORD_FILE}: {msg}")

    if not isinstance(record, dict):
        fail("root must be an object")
    if record.get("schema_version") != RUN_SCHEMA_VERSION:
        fail(f"unsupported schema_version {record.get('schema_version')!r}; expected {RUN_SCHEMA_VERSION}")
    if record.get("kind") != RUN_KIND:
        fail(f"kind must be '{RUN_KIND}'")
    missing = RECORD_KEYS - set(record)
    if missing:
        fail(f"missing keys {sorted(missing)}")
    plan_ids = [s.get("id") for s in record["plan"].get("stages", [])] if isinstance(record["plan"], dict) else []
    stages = record["stages"]
    if not isinstance(stages, dict) or sorted(stages) != sorted(plan_ids):
        fail("stage set does not match the pinned plan")
    valid_states = {s.value for s in StageState}
    for sid, st in stages.items():
        if not isinstance(st, dict) or set(st) != STAGE_KEYS:
            fail(f"stage '{sid}' has an invalid shape")
        if st["state"] not in valid_states:
            fail(f"stage '{sid}' has invalid state {st['state']!r}")


def new_stage_record(provider: str) -> Dict[str, object]:
    return {
        "state": StageState.PENDING.value, "provider": provider, "attempts": 0,
        "awaiting_approval": False, "escalated": False, "handoff": None, "evidence": {},
        "criteria_met": [], "missing": [], "blocked_reasons": [], "approval": None,
        "not_applicable_reason": None,
    }


def add_event(record: dict, now: str, stage: Optional[str], event: str, detail: Optional[dict] = None) -> None:
    record["events"].append({"seq": len(record["events"]) + 1, "at": now, "stage": stage,
                             "event": event, "detail": detail or {}})
    record["updated_at"] = now
