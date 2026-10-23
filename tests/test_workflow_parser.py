"""
Tests for strict workflow definition parsing and validation (schema v1).
"""

import hashlib
import unittest

from awb.config import Config
from awb.registry.parser import load_registry
from awb.workflow import StageState, WorkflowValidationError, load_workflow, parse_workflow_string

from tests.workflow_helpers import EXAMPLE, FIXTURES, MINIMAL, REGISTRY


class TestWorkflowParser(unittest.TestCase):

    def assertInvalid(self, text, *fragments, registry=REGISTRY):
        with self.assertRaises(WorkflowValidationError) as ctx:
            parse_workflow_string(text, registry=registry)
        message = str(ctx.exception)
        for fragment in fragments:
            self.assertIn(fragment, message)
        return ctx.exception

    # --- valid definitions ---------------------------------------------------

    def test_example_workflow_validates_against_repo_registry(self):
        registry = load_registry(Config.load().registry_path)
        wf = load_workflow(EXAMPLE, registry=registry)
        self.assertEqual(wf.id, "plan-implement-review")
        self.assertEqual(
            [s.id for s in wf.stages],
            ["plan", "implement", "test", "review", "remediate", "final-verify"],
        )
        self.assertEqual(wf.sha256, hashlib.sha256(EXAMPLE.read_bytes()).hexdigest())
        self.assertTrue(wf.stage("remediate").optional)
        self.assertTrue(wf.stage("final-verify").requires_approval)
        self.assertEqual(wf.warnings, ())

    def test_minimal_defaults(self):
        wf = parse_workflow_string(MINIMAL, registry=REGISTRY)
        build = wf.stage("build")
        self.assertEqual(build.max_attempts, 2)
        self.assertEqual(build.on_exhausted, "escalate")
        self.assertFalse(build.optional)
        self.assertFalse(build.requires_approval)
        self.assertEqual(wf.routing.roles["builder"].preferred, "antigravity")

    def test_preferred_defaults_to_first_eligible(self):
        text = MINIMAL.replace('eligible = ["codex", "claude"]\npreferred = "codex"', 'eligible = ["claude", "codex"]')
        wf = parse_workflow_string(text, registry=REGISTRY)
        self.assertEqual(wf.routing.roles["checker"].preferred, "claude")

    def test_roles_carry_no_provider_information(self):
        wf = parse_workflow_string(MINIMAL, registry=REGISTRY)
        for role in wf.roles.values():
            for attr in ("provider", "providers", "eligible", "preferred"):
                self.assertFalse(hasattr(role, attr))

    def test_stage_states_cover_lifecycle(self):
        self.assertEqual(
            sorted(s.value for s in StageState),
            sorted(["PENDING", "READY", "RUNNING", "PASSED", "FAILED", "BLOCKED", "SKIPPED", "NOT_APPLICABLE"]),
        )

    def test_registry_is_optional(self):
        wf = parse_workflow_string(MINIMAL.replace('"alpha"', '"not-in-any-registry"'), registry=None)
        self.assertEqual(wf.id, "mini")

    # --- role != provider ----------------------------------------------------

    def test_rejects_provider_key_in_role(self):
        with self.assertRaises(WorkflowValidationError) as ctx:
            load_workflow(FIXTURES / "provider-in-role.toml")
        self.assertIn("roles.builder", str(ctx.exception))
        self.assertIn("role != provider", str(ctx.exception))

    def test_rejects_model_key_in_role(self):
        text = MINIMAL.replace('responsibility = "Build it."', 'responsibility = "Build it."\nmodel = "x"')
        self.assertInvalid(text, "role != provider")

    # --- structure -----------------------------------------------------------

    def test_rejects_unsupported_schema_version(self):
        self.assertInvalid(MINIMAL.replace("schema_version = 1", "schema_version = 2"), "schema_version")

    def test_rejects_unknown_keys(self):
        self.assertInvalid(MINIMAL.replace("[workflow]", "colour = 1\n[workflow]"), "unknown key", "colour")
        self.assertInvalid(MINIMAL.replace('role = "builder"\n', 'role = "builder"\ntimeout = 3\n'), "timeout")

    def test_rejects_invalid_toml(self):
        self.assertInvalid("schema_version = = 1", "TOML")

    def test_rejects_bad_ids_and_semver(self):
        self.assertInvalid(MINIMAL.replace('id = "mini"', 'id = "Mini Flow"'), "workflow.id")
        self.assertInvalid(MINIMAL.replace('version = "1.0.0"\nname', 'version = "1.0"\nname'), "semver")

    def test_rejects_bad_assurance(self):
        self.assertInvalid(MINIMAL.replace('"basic"', '"extreme"'), "assurance")

    def test_rejects_duplicate_stage_ids(self):
        text = MINIMAL.replace('id = "check"', 'id = "build"').replace('depends_on = ["build"]\n', "")
        self.assertInvalid(text, "duplicate stage id 'build'")

    def test_rejects_dangling_role(self):
        self.assertInvalid(MINIMAL.replace('role = "checker"', 'role = "ghost"'), "unknown role 'ghost'")

    def test_rejects_dangling_dependency(self):
        self.assertInvalid(MINIMAL.replace('depends_on = ["build"]', 'depends_on = ["nope"]'), "unknown stage 'nope'")

    def test_rejects_cycle_with_path(self):
        with self.assertRaises(WorkflowValidationError) as ctx:
            load_workflow(FIXTURES / "cycle.toml")
        self.assertIn("cycle", str(ctx.exception))
        self.assertIn("a -> b -> c -> a", str(ctx.exception))

    def test_rejects_stage_without_acceptance(self):
        text = MINIMAL.replace(
            '  [[stages.acceptance]]\n  id = "checked"\n  description = "check.md exists."\n  evidence = ["check.md"]\n', ""
        )
        self.assertInvalid(text, "stages.check", "acceptance")

    def test_rejects_evidence_not_in_outputs(self):
        self.assertInvalid(MINIMAL.replace('evidence = ["out.md"]', 'evidence = ["other.md"]'), "other.md", "outputs")

    def test_rejects_empty_evidence(self):
        self.assertInvalid(MINIMAL.replace('evidence = ["out.md"]', "evidence = []"), "evidence")

    def test_rejects_unsafe_output_names(self):
        for bad in ("../escape.md", "sub/dir.md", ".hidden", "NOT_APPLICABLE.md", ""):
            text = MINIMAL.replace('outputs = ["out.md"]', f'outputs = ["{bad}"]')
            self.assertInvalid(text, "output")

    def test_rejects_input_from_non_dependency(self):
        text = MINIMAL.replace('inputs = ["build/out.md"]', 'inputs = ["check/check.md"]')
        self.assertInvalid(text, "input 'check/check.md'")
        text = MINIMAL.replace('inputs = ["build/out.md"]', 'inputs = ["build/missing.md"]')
        self.assertInvalid(text, "input 'build/missing.md'")

    def test_rejects_bad_retry_policy(self):
        self.assertInvalid(MINIMAL.replace('role = "builder"\n', 'role = "builder"\nmax_attempts = 0\n'), "max_attempts")
        self.assertInvalid(MINIMAL.replace('role = "builder"\n', 'role = "builder"\nmax_attempts = 11\n'), "max_attempts")
        self.assertInvalid(MINIMAL.replace('role = "builder"\n', 'role = "builder"\non_exhausted = "retry"\n'), "on_exhausted")

    def test_rejects_bad_independence_reference(self):
        self.assertInvalid(MINIMAL.replace('independent_of = ["build"]', 'independent_of = ["check"]'), "independent_of")
        self.assertInvalid(MINIMAL.replace('independent_of = ["build"]', 'independent_of = ["zzz"]'), "independent_of")

    def test_rejects_wrong_types(self):
        self.assertInvalid(MINIMAL.replace('depends_on = ["build"]', 'depends_on = "build"'), "depends_on")
        self.assertInvalid(MINIMAL.replace("allow_override = true", 'allow_override = "yes"'), "allow_override")

    # --- skills --------------------------------------------------------------

    def test_rejects_unknown_required_skill(self):
        self.assertInvalid(MINIMAL.replace('required_skills = ["alpha"]', 'required_skills = ["zeta"]'), "zeta")

    def test_warns_on_unknown_recommended_skill(self):
        text = MINIMAL.replace('required_skills = ["alpha"]', 'required_skills = ["alpha"]\nrecommended_skills = ["zeta"]')
        wf = parse_workflow_string(text, registry=REGISTRY)
        self.assertEqual(len(wf.warnings), 1)
        self.assertIn("zeta", wf.warnings[0])

    # --- routing -------------------------------------------------------------

    def test_rejects_missing_role_routing(self):
        text = MINIMAL.replace('[routing.roles.checker]\neligible = ["codex", "claude"]\npreferred = "codex"\n', "")
        self.assertInvalid(text, "routing.roles.checker")

    def test_rejects_routing_for_undeclared_role(self):
        self.assertInvalid(MINIMAL + '\n[routing.roles.ghost]\neligible = ["codex"]\n', "ghost")

    def test_rejects_unknown_provider(self):
        self.assertInvalid(MINIMAL.replace('eligible = ["codex", "claude"]', 'eligible = ["codex", "gpt-cli"]'), "gpt-cli")

    def test_rejects_empty_or_duplicate_eligible(self):
        self.assertInvalid(MINIMAL.replace('eligible = ["codex", "claude"]\npreferred = "codex"', "eligible = []"), "eligible")
        self.assertInvalid(MINIMAL.replace('eligible = ["codex", "claude"]', 'eligible = ["codex", "codex"]'), "eligible")

    def test_rejects_preferred_not_eligible(self):
        self.assertInvalid(MINIMAL.replace('preferred = "codex"', 'preferred = "antigravity"'), "preferred")

    def test_rejects_bad_fallback_rules(self):
        base = MINIMAL + '\n[[routing.fallback]]\nrole = "checker"\nfrom = "codex"\nto = "{to}"\non = [{on}]\n'
        self.assertInvalid(base.format(to="antigravity", on='"unavailable"'), "fallback", "eligible")
        self.assertInvalid(base.format(to="claude", on='"crashed"'), "fallback", "crashed")
        self.assertInvalid(base.format(to="codex", on='"unavailable"'), "fallback", "same")
        self.assertInvalid(base.format(to="claude", on=""), "fallback", "on")
        ok = parse_workflow_string(base.format(to="claude", on='"incompatible"'), registry=REGISTRY)
        self.assertEqual(ok.routing.fallback[0].to_provider, "claude")

    def test_collects_multiple_errors(self):
        text = MINIMAL.replace('role = "checker"', 'role = "ghost"').replace('"basic"', '"extreme"')
        err = self.assertInvalid(text, "ghost", "assurance")
        self.assertGreaterEqual(len(err.errors), 2)


if __name__ == "__main__":
    unittest.main()
