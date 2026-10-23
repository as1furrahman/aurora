"""
Evidence-gated stage state machine for manual-handoff runs
(docs/architecture/workflow-schema.md §4-5). A stage never passes on an exit code:
only declared outputs plus explicitly claimed, evidenced criteria (and approval, if
required) move it to PASSED.
"""

import hashlib
import os
import stat
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Mapping, Optional, Tuple, Union

from awb.registry.schema import RegistryManifest
from awb.safety.exceptions import SafetyError

from . import runs
from .handoff import render_handoff
from .model import NOT_APPLICABLE_FILE, StageState, WorkflowDefinition
from .parser import load_workflow, parse_workflow_bytes
from .planner import build_plan
from .runs import RunError, RunIntegrityError

DONE = (StageState.PASSED.value, StageState.NOT_APPLICABLE.value)


def _now(now: Optional[str]) -> str:
    return now or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def start_run(
    definition: Union[str, Path],
    run_root: Union[str, Path],
    registry: Optional[RegistryManifest] = None,
    provider_override: Optional[str] = None,
    routes: Optional[Mapping[str, str]] = None,
    detected: Optional[Mapping[str, bool]] = None,
    run_id: Optional[str] = None,
    now: Optional[str] = None,
) -> Tuple[Path, dict]:
    """Validate, plan, create the run directory, pin the definition, and issue ready handoffs."""
    now = _now(now)
    definition = Path(definition)
    try:
        raw = definition.read_bytes()
    except OSError as e:
        raise RunError(f"Cannot read workflow definition '{definition}': {e}") from e
    # Validate, plan and pin the same bytes, so the pinned copy always matches the recorded hash.
    wf = parse_workflow_bytes(raw, registry=registry, source=str(definition))
    plan = build_plan(wf, registry=registry, provider_override=provider_override, routes=routes, detected=detected)
    if run_id is None:
        run_id = f"{wf.id}-{now.replace('-', '').replace(':', '')}"

    run_dir = runs.create_run_dir(Path(run_root), run_id)
    runs.write_file(run_dir, runs.WORKFLOW_FILE, raw)
    pinned = _load_pinned(run_dir, wf.sha256)

    record = {
        "schema_version": runs.RUN_SCHEMA_VERSION,
        "kind": runs.RUN_KIND,
        "run_id": run_id,
        "created_at": now,
        "updated_at": now,
        "workflow": {"id": wf.id, "version": wf.version, "sha256": wf.sha256, "path": runs.WORKFLOW_FILE},
        "plan": plan,
        "stages": {s["id"]: runs.new_stage_record(s["provider"]) for s in plan["stages"]},
        "events": [],
    }
    runs.add_event(record, now, None, "run-created", {"mode": plan["mode"], "provider_override": provider_override,
                                                      "routes": plan["routes"]})
    for s in plan["stages"]:
        if s["status"] == "blocked":
            st = record["stages"][s["id"]]
            st["state"] = StageState.BLOCKED.value
            st["blocked_reasons"] = list(s["blocked_reasons"])
            runs.add_event(record, now, s["id"], "blocked-by-plan", {"reasons": s["blocked_reasons"]})

    with runs.locked(run_dir):
        # Persist the record before issuing any handoff: an interruption leaves a resumable run.
        runs.save_record(run_dir, record)
        warnings = _advance(pinned, record, run_dir, now)
        runs.save_record(run_dir, record)
    record["_warnings"] = warnings
    return run_dir, record


def _load_pinned(run_dir: Path, expected_sha: str) -> WorkflowDefinition:
    info = runs.read_file(run_dir, runs.WORKFLOW_FILE)
    if info["sha256"] != expected_sha:
        raise RunIntegrityError(
            f"{runs.WORKFLOW_FILE} in '{run_dir}' no longer matches the pinned sha256 "
            f"({info['sha256'][:12]} != {expected_sha[:12]}); refusing to continue"
        )
    return load_workflow(run_dir / runs.WORKFLOW_FILE)


def _open(run_dir: Union[str, Path]) -> Tuple[Path, dict, WorkflowDefinition]:
    run_dir = Path(run_dir).resolve()
    record = runs.load_record(run_dir)
    wf = _load_pinned(run_dir, record["workflow"]["sha256"])
    return run_dir, record, wf


def _require_run_dir(run_dir: Union[str, Path]) -> Path:
    """Validate that run_dir holds a run record before taking its lock (never lock arbitrary directories)."""
    run_dir = Path(run_dir).resolve()
    runs.load_record(run_dir)
    return run_dir


def _plan_stage(record: dict, stage_id: str) -> dict:
    return next(s for s in record["plan"]["stages"] if s["id"] == stage_id)


def _advance(wf: WorkflowDefinition, record: dict, run_dir: Path, now: str) -> List[str]:
    """Promote PENDING stages whose dependencies are done; issue (or re-issue) their handoffs."""
    warnings: List[str] = []
    for sid in record["plan"]["order"]:
        st = record["stages"][sid]
        stage = wf.stage(sid)
        if st["state"] == StageState.PENDING.value and all(record["stages"][d]["state"] in DONE
                                                          for d in stage.depends_on):
            st["state"] = StageState.READY.value
            runs.add_event(record, now, sid, "ready")
        if st["state"] in (StageState.READY.value, StageState.RUNNING.value, StageState.FAILED.value):
            warning = _issue_handoff(wf, record, run_dir, sid, now)
            if warning:
                warnings.append(warning)
            if st["state"] == StageState.READY.value:
                st["state"] = StageState.RUNNING.value
    return warnings


def _issue_handoff(wf: WorkflowDefinition, record: dict, run_dir: Path, sid: str, now: str) -> Optional[str]:
    """Write the bundle if absent. Never overwrite a bundle whose content differs."""
    stage_plan = _plan_stage(record, sid)
    content = render_handoff(wf, record["plan"], stage_plan, run_dir, record["run_id"]).encode("utf-8")
    expected = hashlib.sha256(content).hexdigest()
    rel = stage_plan["handoff_path"]
    st = record["stages"][sid]
    runs.ensure_dir(run_dir, "handoff")
    runs.ensure_dir(run_dir, f"artifacts/{sid}")

    path = run_dir / rel
    if os.path.lexists(path):
        try:
            existing = runs.read_file(run_dir, rel)["sha256"]
        except (SafetyError, OSError) as e:
            existing = f"unreadable: {e}"
        if existing != expected:
            warning = f"handoff bundle for stage '{sid}' differs from the generated one; not overwritten"
            if st["handoff"] is None or st["handoff"].get("preserved_existing") is not True:
                runs.add_event(record, now, sid, "handoff-preserved", {"path": rel, "on_disk_sha256": existing})
            st["handoff"] = {"path": rel, "sha256": expected, "preserved_existing": True, "on_disk_sha256": existing}
            return warning
        if st["handoff"] is None:
            runs.add_event(record, now, sid, "handoff-issued", {"path": rel, "sha256": expected})
        st["handoff"] = {"path": rel, "sha256": expected, "preserved_existing": False, "on_disk_sha256": expected}
        return None

    runs.write_file(run_dir, rel, content)
    runs.add_event(record, now, sid, "handoff-reissued" if st["handoff"] else "handoff-issued",
                   {"path": rel, "sha256": expected})
    st["handoff"] = {"path": rel, "sha256": expected, "preserved_existing": False, "on_disk_sha256": expected}
    return None


def _inspect_artifact(run_dir: Path, rel: str) -> Tuple[Optional[str], Optional[str]]:
    """Return (sha256, None) for valid evidence, else (None, problem)."""
    path = run_dir / rel
    try:
        st = os.lstat(path)
    except FileNotFoundError:
        return None, f"output missing: {rel}"
    if not stat.S_ISREG(st.st_mode):
        return None, f"output not a regular file (symlink or special file): {rel}"
    try:
        info = runs.read_file(run_dir, rel, max_size=runs.EVIDENCE_MAX_BYTES)
    except (SafetyError, OSError) as e:
        return None, f"output unreadable: {rel} ({e})"
    # Size is taken from the bytes actually hashed, not from the earlier lstat.
    if info["size"] == 0:
        return None, f"output empty: {rel}"
    return info["sha256"], None


def submit(
    run_dir: Union[str, Path],
    stage_id: str,
    met: List[str],
    not_applicable: bool = False,
    now: Optional[str] = None,
) -> dict:
    """Ingest evidence for one stage and evaluate its acceptance gate."""
    now = _now(now)
    run_dir = _require_run_dir(run_dir)
    with runs.locked(run_dir):
        run_dir, record, wf = _open(run_dir)
        if stage_id not in record["stages"]:
            raise RunError(f"Unknown stage '{stage_id}'. Stages: {record['plan']['order']}")
        stage = wf.stage(stage_id)
        st = record["stages"][stage_id]

        if st["state"] not in (StageState.RUNNING.value, StageState.FAILED.value):
            raise RunError(f"Stage '{stage_id}' is {st['state']}; submissions are accepted only for RUNNING or "
                           "FAILED stages")
        known = {c.id for c in stage.acceptance}
        unknown = sorted(set(met) - known)
        if unknown:
            raise RunError(f"Unknown criteria for stage '{stage_id}': {unknown}. Known: {sorted(known)}")

        if not_applicable:
            if not stage.optional:
                raise RunError(f"Stage '{stage_id}' is not optional and cannot be marked NOT_APPLICABLE")
            rel = f"artifacts/{stage_id}/{NOT_APPLICABLE_FILE}"
            sha, problem = _inspect_artifact(run_dir, rel)
            if problem:
                raise RunError(f"--not-applicable requires a justification: {problem.replace('output', 'file', 1)}")
            st["state"] = StageState.NOT_APPLICABLE.value
            st["evidence"] = {rel: sha}
            st["missing"] = []
            st["not_applicable_reason"] = rel
            runs.add_event(record, now, stage_id, "not-applicable", {"justification": rel, "sha256": sha})
        else:
            # Resubmitting while awaiting approval re-evaluates the evidence (e.g. after drift).
            st["awaiting_approval"] = False
            st["attempts"] += 1
            evidence: Dict[str, str] = {}
            missing: List[str] = []
            for out in stage.outputs:
                rel = f"artifacts/{stage_id}/{out}"
                sha, problem = _inspect_artifact(run_dir, rel)
                if problem:
                    missing.append(problem)
                else:
                    evidence[rel] = sha
            for crit in stage.acceptance:
                if crit.id not in met:
                    missing.append(f"criterion '{crit.id}' not claimed (use --met {crit.id})")
                    continue
                absent = [e for e in crit.evidence if f"artifacts/{stage_id}/{e}" not in evidence]
                if absent:
                    missing.append(f"criterion '{crit.id}' lacks evidence: {absent}")

            st["missing"] = missing
            if not missing:
                st["evidence"] = evidence
                st["criteria_met"] = [c.id for c in stage.acceptance]
                if stage.requires_approval:
                    st["state"] = StageState.RUNNING.value
                    st["awaiting_approval"] = True
                    runs.add_event(record, now, stage_id, "gate-satisfied-awaiting-approval",
                                   {"attempt": st["attempts"], "evidence": evidence})
                else:
                    st["state"] = StageState.PASSED.value
                    runs.add_event(record, now, stage_id, "passed", {"attempt": st["attempts"], "evidence": evidence})
            elif st["attempts"] >= stage.max_attempts:
                st["state"] = StageState.BLOCKED.value
                st["escalated"] = stage.on_exhausted == "escalate"
                st["blocked_reasons"] = [f"max_attempts ({stage.max_attempts}) exhausted"]
                runs.add_event(record, now, stage_id, "escalated" if st["escalated"] else "blocked",
                               {"attempt": st["attempts"], "missing": missing})
            else:
                st["state"] = StageState.FAILED.value
                runs.add_event(record, now, stage_id, "failed", {"attempt": st["attempts"], "missing": missing})

        warnings = _advance(wf, record, run_dir, now)
        runs.save_record(run_dir, record)

    return {"stage": stage_id, "state": st["state"], "attempts": st["attempts"], "missing": st["missing"],
            "awaiting_approval": st["awaiting_approval"], "escalated": st["escalated"], "warnings": warnings}


def _drift(run_dir: Path, st: dict, stage_id: str) -> List[dict]:
    problems = []
    for rel, sha in sorted(st["evidence"].items()):
        current, problem = _inspect_artifact(run_dir, rel)
        if problem:
            problems.append({"stage": stage_id, "path": rel, "problem": "missing"})
        elif current != sha:
            problems.append({"stage": stage_id, "path": rel, "problem": "hash mismatch"})
    return problems


def approve(run_dir: Union[str, Path], stage_id: str, by: str, now: Optional[str] = None) -> dict:
    """Record human approval for a stage whose gate is satisfied; evidence must be unchanged."""
    now = _now(now)
    if not by or not by.strip():
        raise RunError("--by must name the approver")
    run_dir = _require_run_dir(run_dir)
    with runs.locked(run_dir):
        run_dir, record, wf = _open(run_dir)
        if stage_id not in record["stages"]:
            raise RunError(f"Unknown stage '{stage_id}'")
        st = record["stages"][stage_id]
        if not st["awaiting_approval"] or st["state"] != StageState.RUNNING.value:
            raise RunError(f"Stage '{stage_id}' is not awaiting approval (state {st['state']})")
        drift = _drift(run_dir, st, stage_id)
        if drift:
            raise RunIntegrityError(f"Evidence for stage '{stage_id}' changed since submission: {drift}; "
                                    "resubmission is required before approval")
        st["awaiting_approval"] = False
        st["state"] = StageState.PASSED.value
        st["approval"] = {"by": by, "at": now}
        runs.add_event(record, now, stage_id, "approved", {"by": by})
        warnings = _advance(wf, record, run_dir, now)
        runs.save_record(run_dir, record)
    return {"stage": stage_id, "state": st["state"], "warnings": warnings}


def resume(run_dir: Union[str, Path], now: Optional[str] = None) -> dict:
    """Re-verify integrity and re-issue missing handoffs for unfinished stages. Never touches PASSED stages."""
    now = _now(now)
    run_dir = _require_run_dir(run_dir)
    with runs.locked(run_dir):
        run_dir, record, wf = _open(run_dir)
        warnings = _advance(wf, record, run_dir, now)
        runs.save_record(run_dir, record)
    report = status(run_dir)
    report["warnings"] = warnings + report["warnings"]
    return report


def status(run_dir: Union[str, Path]) -> dict:
    """Read-only run summary with completion, waiting/escalation info, and evidence drift."""
    run_dir, record, wf = _open(run_dir)
    stages = []
    drift: List[dict] = []
    for sid in record["plan"]["order"]:
        st = record["stages"][sid]
        stage = wf.stage(sid)
        if st["state"] in DONE or st["awaiting_approval"]:
            drift.extend(_drift(run_dir, st, sid))
        waiting = [d for d in stage.depends_on if record["stages"][d]["state"] not in DONE] \
            if st["state"] == StageState.PENDING.value else []
        stages.append({
            "id": sid, "role": stage.role, "provider": st["provider"], "state": st["state"],
            "attempts": st["attempts"], "max_attempts": stage.max_attempts, "optional": stage.optional,
            "awaiting_approval": st["awaiting_approval"], "escalated": st["escalated"],
            "missing": st["missing"], "blocked_reasons": st["blocked_reasons"], "waiting_on": waiting,
            "handoff": st["handoff"]["path"] if st["handoff"] else None,
        })

    complete = all(
        (s["state"] == StageState.PASSED.value) or (s["optional"] and s["state"] == StageState.NOT_APPLICABLE.value)
        for s in stages
    ) and not drift
    reduced = sorted({s["id"] for s in record["plan"]["stages"]
                      for e in s["independence"] if e["status"] == "same-provider"})
    return {
        "run_id": record["run_id"],
        "run_dir": str(run_dir),
        "workflow": record["workflow"],
        "mode": record["plan"]["mode"],
        "complete": complete,
        "stages": stages,
        "escalations": [s["id"] for s in stages if s["escalated"]],
        "awaiting_approval": [s["id"] for s in stages if s["awaiting_approval"]],
        "drift": drift,
        "reduced_independence": reduced,
        "warnings": list(record["plan"]["warnings"]),
    }
