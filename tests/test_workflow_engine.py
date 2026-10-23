"""
Evidence-gated run tracking: run layout, handoff bundles, acceptance gates,
retry/escalation, approval, resume, integrity and drift (manual-handoff mode).
All runs live in temporary directories.
"""

import hashlib
import io
import json
import os
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path
from unittest import mock

import awb.safety.boundaries as boundaries
from awb.cli import main
from awb.config import Config
from awb.errors import GlobalMutationBlockedError
from awb.providers import PROVIDERS
from awb.registry.parser import load_registry
from awb.workflow import StageState
from awb.workflow import engine, runs
from awb.workflow.runs import RunError, RunIntegrityError, RunRecordError

from tests.workflow_helpers import EXAMPLE, MINIMAL, REGISTRY

FROZEN = "2026-10-23T12:00:00Z"


class RunTestCase(unittest.TestCase):

    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="awb_test_runs_")
        self.root = Path(self.tmp.name) / "runs"
        self.root.mkdir()
        self.registry = load_registry(Config.load().registry_path)

    def tearDown(self):
        self.tmp.cleanup()

    def start(self, definition=EXAMPLE, registry=None, run_id="r1", **kwargs):
        run_dir, record = engine.start_run(
            definition, self.root, registry=registry or self.registry, run_id=run_id, now=FROZEN, **kwargs)
        return run_dir, record

    def minimal(self, text=MINIMAL, **kwargs):
        path = Path(self.tmp.name) / "mini.toml"
        path.write_text(text, encoding="utf-8")
        return self.start(definition=path, registry=REGISTRY, **kwargs)

    def produce(self, run_dir, stage, name, content="evidence\n"):
        path = run_dir / "artifacts" / stage / name
        path.write_text(content, encoding="utf-8")
        return path

    def states(self, run_dir):
        return {sid: s["state"] for sid, s in runs.load_record(run_dir)["stages"].items()}

    def pass_stage(self, run_dir, stage, outputs, criteria):
        for name in outputs:
            self.produce(run_dir, stage, name)
        return engine.submit(run_dir, stage, met=criteria, now=FROZEN)


class TestRunLayout(RunTestCase):

    def test_start_creates_versioned_record_and_first_handoff(self):
        run_dir, record = self.start()
        self.assertEqual(run_dir, self.root / "r1")
        self.assertEqual(record["schema_version"], 1)
        self.assertEqual(record["kind"], "aurora.workflow-run")
        self.assertEqual((run_dir / "workflow.toml").read_bytes(), EXAMPLE.read_bytes())
        self.assertEqual(record["workflow"]["sha256"], hashlib.sha256(EXAMPLE.read_bytes()).hexdigest())
        self.assertEqual(self.states(run_dir), {
            "plan": "RUNNING", "implement": "PENDING", "test": "PENDING",
            "review": "PENDING", "remediate": "PENDING", "final-verify": "PENDING",
        })
        self.assertTrue((run_dir / "handoff" / "plan.md").is_file())
        self.assertFalse((run_dir / "handoff" / "implement.md").exists())
        self.assertTrue((run_dir / "artifacts" / "plan").is_dir())
        self.assertEqual(json.loads((run_dir / "run.json").read_text())["plan"]["mode"], "multi")

    def test_handoff_bundle_is_role_specific_and_safe(self):
        run_dir, _ = self.start()
        text = (run_dir / "handoff" / "plan.md").read_text()
        self.assertIn("planner", text)
        self.assertIn("scoped implementation plan", text)
        self.assertIn("principles", text)
        self.assertIn("artifacts/plan/plan.md", text)
        self.assertIn("plan-written", text)
        self.assertIn("Claude Code", text)
        self.assertIn(f"aurora workflow submit --run {run_dir} --stage plan --met plan-written", text)
        self.assertIn("exit code", text)
        for provider in PROVIDERS.values():
            for flag in provider.spec.forbidden_flags:
                self.assertNotIn(flag, text)

    def test_handoff_bundles_are_deterministic(self):
        a, _ = self.start(run_id="a")
        b, _ = self.start(run_id="b")
        ta = (a / "handoff" / "plan.md").read_text().replace(str(a), "<RUN>").replace("Run: a", "Run: <ID>")
        tb = (b / "handoff" / "plan.md").read_text().replace(str(b), "<RUN>").replace("Run: b", "Run: <ID>")
        self.assertEqual(ta, tb)

    def test_single_provider_run_records_reduced_independence(self):
        run_dir, record = self.start(provider_override="codex")
        self.assertEqual({s["provider"] for s in record["stages"].values()}, {"codex"})
        review = next(s for s in record["plan"]["stages"] if s["id"] == "review")
        self.assertEqual(review["independence"][0]["status"], "same-provider")
        self.assertIn("Codex", (run_dir / "handoff" / "plan.md").read_text())

    def test_plan_blocked_stage_starts_blocked(self):
        run_dir, record = self.minimal(provider_override="antigravity")
        self.assertEqual(self.states(run_dir), {"build": "RUNNING", "check": "BLOCKED"})
        self.assertTrue(record["stages"]["check"]["blocked_reasons"])

    def test_run_root_must_exist_and_run_id_is_safe(self):
        with self.assertRaises(RunError):
            engine.start_run(EXAMPLE, self.root / "missing", registry=self.registry, run_id="x")
        for bad in ("../escape", "a/b", ".", "..", "", ".hidden"):
            with self.assertRaises(RunError, msg=bad):
                engine.start_run(EXAMPLE, self.root, registry=self.registry, run_id=bad)
        self.start(run_id="taken")
        with self.assertRaises(RunError):
            self.start(run_id="taken")

    def test_default_run_id_uses_workflow_and_time(self):
        run_dir, _ = engine.start_run(EXAMPLE, self.root, registry=self.registry, now=FROZEN)
        self.assertEqual(run_dir.name, "plan-implement-review-20261023T120000Z")

    def test_run_root_inside_global_agent_dir_is_rejected(self):
        fake_global = Path(self.tmp.name) / "fake-global-skills"
        fake_global.mkdir()
        with mock.patch.object(boundaries, "GLOBAL_AGENT_SKILL_PATHS", [fake_global]):
            with self.assertRaises(GlobalMutationBlockedError):
                engine.start_run(EXAMPLE, fake_global, registry=self.registry, run_id="x")
        self.assertEqual(list(fake_global.iterdir()), [])


class TestAcceptanceGates(RunTestCase):

    def test_missing_outputs_fail_and_do_not_advance(self):
        run_dir, _ = self.start()
        result = engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        self.assertEqual(result["state"], "FAILED")
        self.assertIn("output missing: artifacts/plan/plan.md", result["missing"])
        self.assertEqual(self.states(run_dir)["implement"], "PENDING")
        self.assertEqual(runs.load_record(run_dir)["stages"]["plan"]["attempts"], 1)

    def test_outputs_without_claimed_criteria_do_not_pass(self):
        run_dir, _ = self.start()
        self.produce(run_dir, "plan", "plan.md")
        result = engine.submit(run_dir, "plan", met=[], now=FROZEN)
        self.assertEqual(result["state"], "FAILED")
        self.assertIn("criterion 'plan-written' not claimed (use --met plan-written)", result["missing"])

    def test_empty_and_symlinked_outputs_are_not_evidence(self):
        run_dir, _ = self.start()
        self.produce(run_dir, "plan", "plan.md", content="")
        result = engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        self.assertIn("output empty: artifacts/plan/plan.md", result["missing"])

        (run_dir / "artifacts" / "plan" / "plan.md").unlink()
        real = Path(self.tmp.name) / "elsewhere.md"
        real.write_text("x", encoding="utf-8")
        (run_dir / "artifacts" / "plan" / "plan.md").symlink_to(real)
        result = engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        self.assertEqual(result["state"], "BLOCKED")  # second attempt of max 2
        self.assertTrue(any("not a regular file" in m for m in result["missing"]))

    def test_complete_submission_passes_and_records_evidence(self):
        run_dir, _ = self.start()
        path = self.produce(run_dir, "plan", "plan.md", content="the plan\n")
        result = engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        self.assertEqual(result["state"], "PASSED")
        stage = runs.load_record(run_dir)["stages"]["plan"]
        self.assertEqual(stage["evidence"], {"artifacts/plan/plan.md": hashlib.sha256(path.read_bytes()).hexdigest()})
        self.assertEqual(stage["criteria_met"], ["plan-written"])
        self.assertEqual(self.states(run_dir)["implement"], "RUNNING")
        self.assertTrue((run_dir / "handoff" / "implement.md").is_file())

    def test_resubmitting_passed_stage_is_rejected(self):
        run_dir, _ = self.start()
        self.pass_stage(run_dir, "plan", ["plan.md"], ["plan-written"])
        with self.assertRaises(RunError) as ctx:
            engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        self.assertIn("PASSED", str(ctx.exception))

    def test_submit_to_pending_stage_is_rejected(self):
        run_dir, _ = self.start()
        with self.assertRaises(RunError):
            engine.submit(run_dir, "review", met=["review-complete"], now=FROZEN)

    def test_unknown_criterion_rejected_without_consuming_attempt(self):
        run_dir, _ = self.start()
        with self.assertRaises(RunError):
            engine.submit(run_dir, "plan", met=["plan-written", "made-up"], now=FROZEN)
        self.assertEqual(runs.load_record(run_dir)["stages"]["plan"]["attempts"], 0)
        with self.assertRaises(RunError):
            engine.submit(run_dir, "nope", met=[], now=FROZEN)

    def test_exhausted_attempts_block_and_escalate(self):
        run_dir, _ = self.start()
        engine.submit(run_dir, "plan", met=[], now=FROZEN)
        result = engine.submit(run_dir, "plan", met=[], now=FROZEN)
        self.assertEqual(result["state"], "BLOCKED")
        stage = runs.load_record(run_dir)["stages"]["plan"]
        self.assertTrue(stage["escalated"])
        status = engine.status(run_dir)
        self.assertFalse(status["complete"])
        implement = next(s for s in status["stages"] if s["id"] == "implement")
        self.assertEqual(implement["waiting_on"], ["plan"])
        self.assertEqual(status["escalations"], ["plan"])

    def test_on_exhausted_block_does_not_escalate(self):
        text = MINIMAL.replace('role = "builder"\n', 'role = "builder"\nmax_attempts = 1\non_exhausted = "block"\n')
        run_dir, _ = self.minimal(text)
        result = engine.submit(run_dir, "build", met=[], now=FROZEN)
        self.assertEqual(result["state"], "BLOCKED")
        self.assertFalse(runs.load_record(run_dir)["stages"]["build"]["escalated"])

    def test_failed_stage_can_retry(self):
        run_dir, _ = self.start()
        engine.submit(run_dir, "plan", met=[], now=FROZEN)
        result = self.pass_stage(run_dir, "plan", ["plan.md"], ["plan-written"])
        self.assertEqual(result["state"], "PASSED")


class TestOptionalApprovalAndCompletion(RunTestCase):

    def advance_to_remediate(self, run_dir):
        self.pass_stage(run_dir, "plan", ["plan.md"], ["plan-written"])
        self.pass_stage(run_dir, "implement", ["change-summary.md"], ["change-described"])
        self.pass_stage(run_dir, "test", ["test-report.md"], ["suite-green"])
        self.pass_stage(run_dir, "review", ["review.md"], ["review-complete"])

    def test_not_applicable_requires_optional_stage_and_justification(self):
        run_dir, _ = self.start()
        with self.assertRaises(RunError):
            engine.submit(run_dir, "plan", met=[], not_applicable=True, now=FROZEN)
        self.advance_to_remediate(run_dir)
        with self.assertRaises(RunError) as ctx:
            engine.submit(run_dir, "remediate", met=[], not_applicable=True, now=FROZEN)
        self.assertIn("NOT_APPLICABLE.md", str(ctx.exception))
        self.produce(run_dir, "remediate", "NOT_APPLICABLE.md", "Review found no issues.\n")
        result = engine.submit(run_dir, "remediate", met=[], not_applicable=True, now=FROZEN)
        self.assertEqual(result["state"], "NOT_APPLICABLE")
        self.assertEqual(self.states(run_dir)["final-verify"], "RUNNING")

    def test_approval_gate_and_completion(self):
        run_dir, _ = self.start()
        self.advance_to_remediate(run_dir)
        self.produce(run_dir, "remediate", "NOT_APPLICABLE.md", "No findings.\n")
        engine.submit(run_dir, "remediate", met=[], not_applicable=True, now=FROZEN)

        result = self.pass_stage(run_dir, "final-verify", ["final-report.md"], ["final-suite-green"])
        self.assertEqual(result["state"], "RUNNING")
        self.assertTrue(result["awaiting_approval"])
        self.assertFalse(engine.status(run_dir)["complete"])
        with self.assertRaises(RunError):
            engine.submit(run_dir, "final-verify", met=["final-suite-green"], now=FROZEN)

        engine.approve(run_dir, "final-verify", by="reviewer@example", now=FROZEN)
        status = engine.status(run_dir)
        self.assertTrue(status["complete"])
        stage = runs.load_record(run_dir)["stages"]["final-verify"]
        self.assertEqual(stage["state"], "PASSED")
        self.assertEqual(stage["approval"], {"by": "reviewer@example", "at": FROZEN})

    def test_approve_rejects_drifted_evidence_and_wrong_state(self):
        run_dir, _ = self.start()
        with self.assertRaises(RunError):
            engine.approve(run_dir, "plan", by="me", now=FROZEN)
        self.advance_to_remediate(run_dir)
        self.produce(run_dir, "remediate", "NOT_APPLICABLE.md", "No findings.\n")
        engine.submit(run_dir, "remediate", met=[], not_applicable=True, now=FROZEN)
        path = self.produce(run_dir, "final-verify", "final-report.md", "green\n")
        engine.submit(run_dir, "final-verify", met=["final-suite-green"], now=FROZEN)
        path.write_text("tampered\n", encoding="utf-8")
        with self.assertRaises(RunIntegrityError):
            engine.approve(run_dir, "final-verify", by="me", now=FROZEN)
        self.assertEqual(self.states(run_dir)["final-verify"], "RUNNING")

    def test_completion_requires_every_mandatory_stage(self):
        run_dir, _ = self.start()
        self.advance_to_remediate(run_dir)
        self.assertFalse(engine.status(run_dir)["complete"])


class TestResumeAndIntegrity(RunTestCase):

    def test_resume_reissues_missing_bundle_identically(self):
        run_dir, _ = self.start()
        bundle = run_dir / "handoff" / "plan.md"
        original = bundle.read_bytes()
        bundle.unlink()
        engine.resume(run_dir, now=FROZEN)
        self.assertEqual(bundle.read_bytes(), original)

    def test_resume_preserves_edited_bundle(self):
        run_dir, _ = self.start()
        bundle = run_dir / "handoff" / "plan.md"
        bundle.write_text("my edited prompt\n", encoding="utf-8")
        report = engine.resume(run_dir, now=FROZEN)
        self.assertEqual(bundle.read_text(), "my edited prompt\n")
        self.assertTrue(any("plan" in w and "not overwritten" in w for w in report["warnings"]))

    def test_resume_never_reissues_passed_stages(self):
        run_dir, _ = self.start()
        self.pass_stage(run_dir, "plan", ["plan.md"], ["plan-written"])
        (run_dir / "handoff" / "plan.md").unlink()
        events_before = len(runs.load_record(run_dir)["events"])
        engine.resume(run_dir, now=FROZEN)
        self.assertFalse((run_dir / "handoff" / "plan.md").exists())
        record = runs.load_record(run_dir)
        self.assertEqual(record["stages"]["plan"]["state"], "PASSED")
        self.assertFalse(any(e["stage"] == "plan" for e in record["events"][events_before:]))

    def test_interrupted_submission_is_safely_resumable(self):
        run_dir, _ = self.start()
        self.produce(run_dir, "plan", "plan.md")
        with mock.patch.object(runs, "save_record", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        # The record was never written: plan is still RUNNING, no attempt consumed.
        self.assertEqual(self.states(run_dir)["plan"], "RUNNING")
        self.assertEqual(runs.load_record(run_dir)["stages"]["plan"]["attempts"], 0)
        bundle = run_dir / "handoff" / "implement.md"
        early = bundle.read_bytes() if bundle.exists() else None
        result = engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        self.assertEqual(result["state"], "PASSED")
        if early is not None:
            self.assertEqual(bundle.read_bytes(), early)
        self.assertEqual(result["warnings"], [])
        self.assertEqual(self.states(run_dir)["implement"], "RUNNING")

    def test_tampered_workflow_copy_is_detected(self):
        run_dir, _ = self.start()
        with open(run_dir / "workflow.toml", "a", encoding="utf-8") as f:
            f.write("\n# edited\n")
        for call in (lambda: engine.status(run_dir), lambda: engine.resume(run_dir, now=FROZEN),
                     lambda: engine.submit(run_dir, "plan", met=[], now=FROZEN)):
            with self.assertRaises(RunIntegrityError):
                call()

    def test_status_reports_evidence_drift_without_changing_state(self):
        run_dir, _ = self.start()
        path = self.produce(run_dir, "plan", "plan.md")
        engine.submit(run_dir, "plan", met=["plan-written"], now=FROZEN)
        path.write_text("changed after acceptance\n", encoding="utf-8")
        status = engine.status(run_dir)
        self.assertEqual(status["drift"], [{"stage": "plan", "path": "artifacts/plan/plan.md", "problem": "hash mismatch"}])
        self.assertEqual(self.states(run_dir)["plan"], "PASSED")
        path.unlink()
        self.assertEqual(engine.status(run_dir)["drift"][0]["problem"], "missing")

    def test_record_schema_is_strict(self):
        run_dir, _ = self.start()
        record = json.loads((run_dir / "run.json").read_text())
        for mutate in (lambda r: r.update(schema_version=2), lambda r: r.update(kind="other"),
                       lambda r: r["stages"]["plan"].update(state="DONE"),
                       lambda r: r["stages"].pop("plan")):
            bad = json.loads(json.dumps(record))
            mutate(bad)
            (run_dir / "run.json").write_text(json.dumps(bad), encoding="utf-8")
            with self.assertRaises(RunRecordError):
                engine.status(run_dir)

    def test_not_a_run_directory(self):
        with self.assertRaises(RunRecordError):
            engine.status(self.root)


class TestRunCLI(RunTestCase):

    def cli(self, argv):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err), mock.patch("shutil.which", return_value=None):
            code = main(argv)
        return code, out.getvalue(), err.getvalue()

    def test_end_to_end_manual_handoff(self):
        code, out, err = self.cli(["workflow", "start", str(EXAMPLE), "--run-root", str(self.root),
                                   "--run-id", "e2e", "--route", "implement=claude", "--json"])
        self.assertEqual(code, 0, err)
        run_dir = self.root / "e2e"
        self.assertEqual(json.loads(out)["run_dir"], str(run_dir))

        code, out, _ = self.cli(["workflow", "submit", "--run", str(run_dir), "--stage", "plan", "--met", "plan-written"])
        self.assertEqual(code, 1)
        self.assertIn("output missing", out)

        self.produce(run_dir, "plan", "plan.md")
        code, out, _ = self.cli(["workflow", "submit", "--run", str(run_dir), "--stage", "plan", "--met", "plan-written"])
        self.assertEqual(code, 0)
        self.assertIn("PASSED", out)

        code, out, _ = self.cli(["workflow", "status", "--run", str(run_dir), "--json"])
        self.assertEqual(code, 0)
        status = json.loads(out)
        self.assertFalse(status["complete"])
        self.assertEqual({s["id"]: s["state"] for s in status["stages"]}["implement"], "RUNNING")

        code, out, _ = self.cli(["workflow", "status", "--run", str(run_dir)])
        self.assertIn("implement", out)
        self.assertIn("RUNNING", out)

        code, out, _ = self.cli(["workflow", "resume", "--run", str(run_dir)])
        self.assertEqual(code, 0)

        code, _, err = self.cli(["workflow", "approve", "--run", str(run_dir), "--stage", "plan", "--by", "me"])
        self.assertEqual(code, 1)
        self.assertIn("not awaiting approval", err)

    def test_start_requires_run_root(self):
        with self.assertRaises(SystemExit) as ctx:
            self.cli(["workflow", "start", str(EXAMPLE)])
        self.assertNotEqual(ctx.exception.code, 0)


if __name__ == "__main__":
    unittest.main()
