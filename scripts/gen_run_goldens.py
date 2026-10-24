#!/usr/bin/env python3
"""
Regenerate run-record goldens from the Python reference engine (the oracle, ADR-0006).

Drives a run through start -> failed submit -> passing submit with a frozen clock and copies
run.json + workflow.toml to tests/fixtures/runs/python-run/. The Rust run store must load
these records (schema compatibility). Run from the repo root:  python3 scripts/gen_run_goldens.py
"""

import json
import shutil
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from awb.registry.parser import load_registry  # noqa: E402
from awb.workflow import engine, load_workflow  # noqa: E402
from awb.workflow.handoff import render_handoff  # noqa: E402
from awb.workflow.planner import build_plan  # noqa: E402

FROZEN = "2026-10-23T12:00:00Z"
EXAMPLE = ROOT / "workflows" / "examples" / "plan-implement-review.toml"
OUT = ROOT / "tests" / "fixtures" / "runs" / "python-run"
HANDOFF_OUT = ROOT / "tests" / "fixtures" / "handoff"
# Handoff rendering is pure; a fixed (non-existent) run path keeps the goldens location-independent.
FIXED_RUN_DIR = Path("/aurora-golden/run")
FIXTURE_REGISTRY = ROOT / "tests" / "fixtures" / "parity" / "fixture-registry.toml"

# (name, workflow, registry, provider_override, routes)
HANDOFF_CASES = [
    ("example-multi", EXAMPLE, "repo", None, {}),
    ("example-single-claude", EXAMPLE, "repo", "claude", {}),
    ("example-route-implement-codex", EXAMPLE, "repo", None, {"implement": "codex"}),
    ("fallback", ROOT / "tests" / "fixtures" / "parity" / "minimal-ag-fallback.toml", "fixture", None, {}),
]


def write_handoff_goldens() -> int:
    registries = {"repo": load_registry(ROOT / "registry" / "skills.toml"), "fixture": load_registry(FIXTURE_REGISTRY)}
    HANDOFF_OUT.mkdir(parents=True, exist_ok=True)
    manifest = []
    for name, path, reg, override, routes in HANDOFF_CASES:
        wf = load_workflow(path, registry=registries[reg])
        plan = build_plan(wf, registry=registries[reg], provider_override=override, routes=routes)
        for stage_plan in plan["stages"]:
            out = HANDOFF_OUT / f"{name}.{stage_plan['id']}.md"
            out.write_text(render_handoff(wf, plan, stage_plan, FIXED_RUN_DIR, "golden"), encoding="utf-8")
            manifest.append({"workflow": str(path.relative_to(ROOT)), "registry": reg, "provider_override": override,
                             "routes": [[k, v] for k, v in routes.items()], "stage": stage_plan["id"],
                             "expected": str(out.relative_to(ROOT))})
    (HANDOFF_OUT / "cases.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return len(manifest)


def main() -> int:
    registry = load_registry(ROOT / "registry" / "skills.toml")
    # A fixed (gitignored) location keeps bundle hashes, and so run.json, reproducible.
    golden_root = ROOT / "target" / "golden-runs"
    shutil.rmtree(golden_root, ignore_errors=True)
    golden_root.mkdir(parents=True, mode=0o700)
    run_dir, _ = engine.start_run(EXAMPLE, golden_root, registry=registry, run_id="golden", now=FROZEN)
    engine.submit(run_dir, "plan", met=[], now=FROZEN)  # attempt 1: nothing produced
    (run_dir / "artifacts" / "plan" / "plan.md").write_text("Scope: demo\n", encoding="utf-8")
    engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)  # attempt 2: passes
    OUT.mkdir(parents=True, exist_ok=True)
    for name in ("run.json", "workflow.toml"):
        shutil.copyfile(run_dir / name, OUT / name)
    print(f"wrote {OUT.relative_to(ROOT)}/{{run.json,workflow.toml}}")
    print(f"wrote {write_handoff_goldens()} handoff goldens to {HANDOFF_OUT.relative_to(ROOT)}")
    return 0

if __name__ == "__main__":
    sys.exit(main())
