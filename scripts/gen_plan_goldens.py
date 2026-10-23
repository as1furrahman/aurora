#!/usr/bin/env python3
"""
Regenerate plan-parity goldens from the Python reference planner (the oracle, ADR-0006).

Writes tests/fixtures/parity/{<case>.toml, <case>.<mode>.json, cases.json}. The Rust test
crates/aurora-core/tests/plan_parity.rs replays every case and requires identical JSON.
Run from the repo root:  python3 scripts/gen_plan_goldens.py
"""

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT))

from awb.registry.parser import load_registry, parse_registry_string  # noqa: E402
from awb.workflow import load_workflow  # noqa: E402
from awb.workflow.planner import build_plan  # noqa: E402
from tests.workflow_helpers import EXAMPLE, MINIMAL, REGISTRY  # noqa: E402
from tests.test_workflow_routing import CHECKER_FALLBACK, MINIMAL_AG_CHECKER  # noqa: E402

OUT = ROOT / "tests" / "fixtures" / "parity"
FIXTURE_REGISTRY = OUT / "fixture-registry.toml"

SKIP_TARGET = MINIMAL_AG_CHECKER + (
    '\n[[routing.fallback]]\nrole = "checker"\nfrom = "antigravity"\nto = "codex"\non = ["unavailable"]\n'
    '\n[[routing.fallback]]\nrole = "checker"\nfrom = "antigravity"\nto = "claude"\non = ["unavailable"]\n'
)

DEFINITIONS = {
    "minimal": MINIMAL,
    "minimal-ag-checker": MINIMAL_AG_CHECKER,
    "minimal-ag-fallback": MINIMAL_AG_CHECKER + CHECKER_FALLBACK,
    "minimal-skip-target": SKIP_TARGET,
}

ALL_OFF = {"antigravity": False, "claude": False, "codex": False}
CASES = [
    # (case, workflow, registry, override, routes, detected)
    ("example", "example", "repo", None, {}, None),
    ("example", "example", "repo", "claude", {}, None),
    ("example", "example", "repo", "codex", {}, None),
    ("example", "example", "repo", "antigravity", {}, None),
    ("example", "example", "repo", None, {"implement": "codex"}, None),
    ("example", "example", "repo", None, {"implement": "codex", "plan": "codex"}, None),
    ("example", "example", "repo", None, {}, {"antigravity": False, "claude": True, "codex": True}),
    ("example", "example", "repo", None, {}, ALL_OFF),
    ("example", "example", "none", None, {}, None),
    ("minimal", "minimal", "fixture", None, {}, None),
    ("parallel", "parallel", "fixture", None, {}, None),
    ("parallel", "parallel", "fixture", "codex", {}, None),
    ("minimal", "minimal", "fixture", "antigravity", {}, None),
    ("minimal", "minimal", "fixture", None, {}, ALL_OFF),
    ("minimal-ag-checker", "minimal-ag-checker", "fixture", None, {}, None),
    ("minimal-ag-fallback", "minimal-ag-fallback", "fixture", None, {}, None),
    ("minimal-ag-fallback", "minimal-ag-fallback", "fixture", "antigravity", {}, None),
    ("minimal-ag-fallback", "minimal-ag-fallback", "fixture", None, {"check": "antigravity"}, None),
    ("minimal-skip-target", "minimal-skip-target", "fixture", None, {},
     {"antigravity": False, "codex": False, "claude": True}),
    ("minimal-skip-target", "minimal-skip-target", "fixture", None, {}, ALL_OFF),
]


def mode_name(override, routes, detected, registry):
    parts = [f"provider-{override}" if override else "default"]
    parts += [f"route-{s}-{p}" for s, p in routes.items()]
    if detected is not None:
        parts.append("detect-" + "-".join(k for k, v in sorted(detected.items()) if v) or "detect-none")
        if not any(detected.values()):
            parts[-1] = "detect-none"
    if registry == "none":
        parts.append("no-registry")
    return ".".join(parts)


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    FIXTURE_REGISTRY.write_text(REGISTRY_TEXT, encoding="utf-8")
    for name, text in DEFINITIONS.items():
        (OUT / f"{name}.toml").write_text(text, encoding="utf-8")

    registries = {"repo": load_registry(ROOT / "registry" / "skills.toml"),
                  "fixture": load_registry(FIXTURE_REGISTRY), "none": None}
    workflow_paths = {"example": EXAMPLE, "parallel": ROOT / "tests" / "fixtures" / "workflows" / "parallel.toml",
                      **{n: OUT / f"{n}.toml" for n in DEFINITIONS}}

    manifest = []
    for case, wf_key, reg_key, override, routes, detected in CASES:
        wf = load_workflow(workflow_paths[wf_key], registry=registries[reg_key])
        plan = build_plan(wf, registry=registries[reg_key], provider_override=override, routes=routes,
                          detected=detected)
        expected = f"{case}.{mode_name(override, routes, detected, reg_key)}.json"
        (OUT / expected).write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        manifest.append({
            "workflow": str(workflow_paths[wf_key].relative_to(ROOT)),
            "registry": {"repo": "registry/skills.toml", "fixture": str(FIXTURE_REGISTRY.relative_to(ROOT)),
                         "none": None}[reg_key],
            "provider_override": override,
            "routes": [[s, p] for s, p in routes.items()],
            "detected": detected,
            "expected": str((OUT / expected).relative_to(ROOT)),
        })
    (OUT / "cases.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {len(manifest)} parity cases to {OUT.relative_to(ROOT)}")
    return 0


# Same content as tests/workflow_helpers.REGISTRY, kept as text so Rust can parse it too.
REGISTRY_TEXT = '''
[registry]
version = "1.0.0"
updated_at = "2026-10-23T00:00:00Z"

[[skills]]
id = "alpha"
name = "Alpha"
version = "1.0.0"
description = "a"
compatible_agents = ["universal"]
files = ["skills/alpha/SKILL.md"]

[[skills]]
id = "beta"
name = "Beta"
version = "1.0.0"
description = "b"
compatible_agents = ["codex-cli", "claude-code"]
files = ["skills/beta/SKILL.md"]

[[skills]]
id = "gamma"
name = "Gamma"
version = "1.0.0"
description = "g"
compatible_agents = ["claude-code"]
files = ["skills/gamma/SKILL.md"]
'''

if __name__ == "__main__":
    sys.exit(main())
