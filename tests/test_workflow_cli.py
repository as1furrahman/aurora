"""
CLI tests for `aurora workflow validate|show|plan` (read-only commands).
"""

import io
import json
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

from awb.cli import main

from tests.workflow_helpers import EXAMPLE, FIXTURES, MINIMAL

REGISTRY_TOML = '''
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
'''


def run(argv):
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = main(argv)
    return code, out.getvalue(), err.getvalue()


class TestWorkflowCLI(unittest.TestCase):

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="awb_test_wfcli_")
        self.dir = Path(self.tmp.name)
        self.registry = self.dir / "skills.toml"
        self.registry.write_text(REGISTRY_TOML, encoding="utf-8")

    def tearDown(self):
        self.tmp.cleanup()

    def write_workflow(self, text):
        path = self.dir / "wf.toml"
        path.write_text(text, encoding="utf-8")
        return path

    def test_validate_example(self):
        code, out, _ = run(["workflow", "validate", str(EXAMPLE)])
        self.assertEqual(code, 0)
        self.assertIn("VALID", out)
        self.assertIn("plan-implement-review", out)

    def test_validate_invalid_reports_every_error(self):
        code, out, err = run(["workflow", "validate", str(FIXTURES / "provider-in-role.toml")])
        self.assertEqual(code, 1)
        self.assertIn("role != provider", out + err)

    def test_validate_json(self):
        code, out, _ = run(["workflow", "validate", str(FIXTURES / "cycle.toml"), "--json"])
        self.assertEqual(code, 1)
        data = json.loads(out)
        self.assertFalse(data["valid"])
        self.assertTrue(any("cycle" in e for e in data["errors"]))

    def test_show_json_keeps_roles_provider_neutral(self):
        code, out, _ = run(["workflow", "show", str(EXAMPLE), "--json"])
        self.assertEqual(code, 0)
        data = json.loads(out)
        self.assertEqual(data["workflow"]["id"], "plan-implement-review")
        self.assertEqual(set(data["roles"]["planner"]), {"responsibility", "required_skills", "recommended_skills"})
        self.assertEqual(data["routing"]["roles"]["implementer"]["preferred"], "antigravity")
        self.assertEqual([s["id"] for s in data["stages"]][:2], ["plan", "implement"])

    def test_show_text(self):
        code, out, _ = run(["workflow", "show", str(EXAMPLE)])
        self.assertEqual(code, 0)
        self.assertIn("review", out)
        self.assertIn("independent_of", out)

    def test_plan_multi_and_single(self):
        code, out, _ = run(["workflow", "plan", str(EXAMPLE), "--no-detect", "--json"])
        self.assertEqual(code, 0)
        plan = json.loads(out)
        self.assertEqual(plan["mode"], "multi")
        self.assertIsNone(plan["detection"])

        code, out, _ = run(["workflow", "plan", str(EXAMPLE), "--no-detect", "--json", "--provider", "codex"])
        self.assertEqual(code, 0)
        plan = json.loads(out)
        self.assertEqual({s["provider"] for s in plan["stages"]}, {"codex"})

    def test_plan_route(self):
        code, out, _ = run(["workflow", "plan", str(EXAMPLE), "--no-detect", "--json",
                            "--route", "implement=codex", "--route", "plan=codex"])
        self.assertEqual(code, 0)
        plan = json.loads(out)
        self.assertEqual(plan["routes"], {"implement": "codex", "plan": "codex"})

    def test_plan_rejects_malformed_route_and_unknown_provider(self):
        code, _, err = run(["workflow", "plan", str(EXAMPLE), "--route", "implement"])
        self.assertEqual(code, 1)
        self.assertIn("STAGE=PROVIDER", err)
        code, _, err = run(["workflow", "plan", str(EXAMPLE), "--provider", "gpt-cli"])
        self.assertEqual(code, 1)
        self.assertIn("gpt-cli", err)

    def test_plan_blocked_exit_code(self):
        wf = self.write_workflow(MINIMAL)
        code, out, _ = run(["workflow", "plan", str(wf), "--registry", str(self.registry),
                            "--no-detect", "--provider", "antigravity"])
        self.assertEqual(code, 3)
        self.assertIn("BLOCKED", out)

    def test_plan_uses_path_detection_by_default(self):
        with mock.patch("shutil.which", side_effect=lambda b: f"/usr/bin/{b}" if b in ("claude", "codex") else None):
            code, out, _ = run(["workflow", "plan", str(EXAMPLE), "--json"])
        self.assertEqual(code, 0)
        plan = json.loads(out)
        self.assertEqual(plan["detection"], {"antigravity": False, "claude": True, "codex": True})
        implement = next(s for s in plan["stages"] if s["id"] == "implement")
        self.assertEqual(implement["fallback_trail"][0]["reason"], "unavailable")

    def test_plan_writes_nothing(self):
        wf = self.write_workflow(MINIMAL)
        before = sorted(p.name for p in self.dir.iterdir())
        run(["workflow", "plan", str(wf), "--registry", str(self.registry), "--no-detect"])
        self.assertEqual(sorted(p.name for p in self.dir.iterdir()), before)


if __name__ == "__main__":
    unittest.main()
