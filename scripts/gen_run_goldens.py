#!/usr/bin/env python3
"""
Regenerate run-record goldens from the Python reference engine (the oracle, ADR-0006).

Drives a run through start -> failed submit -> passing submit with a frozen clock and copies
run.json + workflow.toml to tests/fixtures/runs/python-run/. The Rust run store must load
these records (schema compatibility). Run from the repo root:  python3 scripts/gen_run_goldens.py
"""

import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from awb.registry.parser import load_registry  # noqa: E402
from awb.workflow import engine  # noqa: E402

FROZEN = "2026-10-23T12:00:00Z"
EXAMPLE = ROOT / "workflows" / "examples" / "plan-implement-review.toml"
OUT = ROOT / "tests" / "fixtures" / "runs" / "python-run"


def main() -> int:
    registry = load_registry(ROOT / "registry" / "skills.toml")
    with tempfile.TemporaryDirectory(prefix="aurora_golden_") as tmp:
        run_dir, _ = engine.start_run(EXAMPLE, Path(tmp), registry=registry, run_id="golden", now=FROZEN)
        engine.submit(run_dir, "plan", met=[], now=FROZEN)  # attempt 1: nothing produced
        (run_dir / "artifacts" / "plan" / "plan.md").write_text("Scope: demo\n", encoding="utf-8")
        engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)  # attempt 2: passes
        OUT.mkdir(parents=True, exist_ok=True)
        for name in ("run.json", "workflow.toml"):
            shutil.copyfile(run_dir / name, OUT / name)
    print(f"wrote {OUT.relative_to(ROOT)}/{{run.json,workflow.toml}}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
