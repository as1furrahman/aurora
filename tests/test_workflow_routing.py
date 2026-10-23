"""
Routing semantics: multi-provider routing vs single-provider override, capability
checks, declared fallback, and independence reporting (ADR-0004).
"""

import builtins
import json
import os
import unittest
from unittest import mock

from awb.config import Config
from awb.registry.parser import load_registry
from awb.workflow import load_workflow, parse_workflow_string
from awb.workflow.planner import PlanError, build_plan

from tests.workflow_helpers import EXAMPLE, FIXTURES, MINIMAL, REGISTRY

# checker may use antigravity, which cannot load skill 'beta' (Codex/Claude only).
MINIMAL_AG_CHECKER = MINIMAL.replace(
    'eligible = ["codex", "claude"]\npreferred = "codex"',
    'eligible = ["antigravity", "codex", "claude"]\npreferred = "antigravity"',
)
CHECKER_FALLBACK = '\n[[routing.fallback]]\nrole = "checker"\nfrom = "antigravity"\nto = "codex"\non = ["incompatible"]\n'


def stages_by_id(plan):
    return {s["id"]: s for s in plan["stages"]}


class TestMultiProviderRouting(unittest.TestCase):

    def setUp(self):
        self.registry = load_registry(Config.load().registry_path)
        self.wf = load_workflow(EXAMPLE, registry=self.registry)

    def test_each_stage_gets_its_preferred_provider(self):
        plan = build_plan(self.wf, registry=self.registry)
        self.assertEqual(plan["mode"], "multi")
        got = {s["id"]: (s["provider"], s["assignment_source"]) for s in plan["stages"]}
        self.assertEqual(got, {
            "plan": ("claude", "preferred"),
            "implement": ("antigravity", "preferred"),
            "test": ("codex", "preferred"),
            "review": ("codex", "preferred"),
            "remediate": ("antigravity", "preferred"),
            "final-verify": ("codex", "preferred"),
        })
        self.assertFalse(plan["blocked"])
        self.assertTrue(all(s["status"] == "assignable" for s in plan["stages"]))

    def test_route_overrides_one_stage_only(self):
        plan = build_plan(self.wf, registry=self.registry, routes={"implement": "codex"})
        s = stages_by_id(plan)
        self.assertEqual((s["implement"]["provider"], s["implement"]["assignment_source"]), ("codex", "route"))
        self.assertEqual(s["remediate"]["provider"], "antigravity")
        self.assertEqual(s["plan"]["provider"], "claude")
        self.assertEqual(plan["routes"], {"implement": "codex"})

    def test_unknown_provider_fails_clearly(self):
        for kwargs in ({"provider_override": "gpt-cli"}, {"routes": {"plan": "gpt-cli"}}):
            with self.assertRaises(PlanError) as ctx:
                build_plan(self.wf, registry=self.registry, **kwargs)
            self.assertIn("gpt-cli", str(ctx.exception))
            self.assertIn("claude", str(ctx.exception))

    def test_route_to_unknown_stage_fails(self):
        with self.assertRaises(PlanError) as ctx:
            build_plan(self.wf, registry=self.registry, routes={"deploy": "codex"})
        self.assertIn("deploy", str(ctx.exception))

    def test_ineligible_route_fails_clearly(self):
        wf = parse_workflow_string(MINIMAL, registry=REGISTRY)
        with self.assertRaises(PlanError) as ctx:
            build_plan(wf, registry=REGISTRY, routes={"check": "antigravity"})
        self.assertIn("not eligible", str(ctx.exception))
        self.assertIn("checker", str(ctx.exception))

    def test_override_and_routes_are_mutually_exclusive(self):
        with self.assertRaises(PlanError):
            build_plan(self.wf, registry=self.registry, provider_override="codex", routes={"plan": "claude"})

    def test_overrides_rejected_when_workflow_forbids_them(self):
        wf = parse_workflow_string(MINIMAL.replace("allow_override = true", "allow_override = false"), registry=REGISTRY)
        for kwargs in ({"provider_override": "codex"}, {"routes": {"build": "codex"}}):
            with self.assertRaises(PlanError) as ctx:
                build_plan(wf, registry=REGISTRY, **kwargs)
            self.assertIn("allow_override", str(ctx.exception))

    def test_order_is_topological_with_declaration_tiebreak(self):
        build_block, check_block = MINIMAL.split("[[stages]]")[1:3]
        check_block = check_block.split("[routing]")[0]
        reordered = MINIMAL.replace("[[stages]]" + build_block, "").replace(
            "[[stages]]" + check_block, "[[stages]]" + check_block + "[[stages]]" + build_block)
        wf = parse_workflow_string(reordered, registry=REGISTRY)
        self.assertEqual([s.id for s in wf.stages], ["check", "build"])
        self.assertEqual(build_plan(wf, registry=REGISTRY)["order"], ["build", "check"])

    def test_parallel_ready_stages_follow_declaration_order(self):
        wf = load_workflow(FIXTURES / "parallel.toml", registry=REGISTRY)
        self.assertEqual(build_plan(wf, registry=REGISTRY)["order"], ["build", "zeta-docs", "alpha-lint", "merge"])

    def test_plan_is_deterministic(self):
        a = build_plan(self.wf, registry=self.registry, routes={"implement": "codex"})
        b = build_plan(self.wf, registry=self.registry, routes={"implement": "codex"})
        self.assertEqual(json.dumps(a, sort_keys=True), json.dumps(b, sort_keys=True))

    def test_planning_performs_no_writes(self):
        def deny(*args, **kwargs):
            raise AssertionError("planning attempted file I/O")
        with mock.patch.object(builtins, "open", deny), mock.patch.object(os, "open", deny), \
                mock.patch.object(os, "mkdir", deny), mock.patch.object(os, "replace", deny):
            build_plan(self.wf, registry=self.registry, provider_override="claude")

    def test_plan_exposes_handoff_and_output_paths(self):
        s = stages_by_id(build_plan(self.wf, registry=self.registry))["review"]
        self.assertEqual(s["handoff_path"], "handoff/review.md")
        self.assertEqual(s["outputs"], ["artifacts/review/review.md"])
        self.assertIn("artifacts/test/test-report.md", s["inputs"])
        self.assertEqual(s["acceptance"][0]["evidence"], ["artifacts/review/review.md"])


class TestSingleProviderOverride(unittest.TestCase):

    def setUp(self):
        self.registry = load_registry(Config.load().registry_path)
        self.wf = load_workflow(EXAMPLE, registry=self.registry)

    def test_override_maps_every_eligible_stage(self):
        for provider in ("codex", "claude", "antigravity"):
            plan = build_plan(self.wf, registry=self.registry, provider_override=provider)
            self.assertEqual(plan["mode"], "single")
            self.assertEqual(plan["provider_override"], provider)
            for s in plan["stages"]:
                self.assertEqual((s["provider"], s["assignment_source"]), (provider, "override"))
                self.assertEqual(s["fallback_trail"], [])

    def test_override_preserves_roles_and_gates(self):
        multi = stages_by_id(build_plan(self.wf, registry=self.registry))
        single = stages_by_id(build_plan(self.wf, registry=self.registry, provider_override="codex"))
        for sid in multi:
            for key in ("role", "responsibility", "required_skills", "acceptance", "outputs",
                        "depends_on", "requires_approval", "optional", "max_attempts"):
                self.assertEqual(multi[sid][key], single[sid][key], f"{sid}.{key}")
        self.assertEqual(self.wf.stage("review").role, "reviewer")

    def test_override_to_ineligible_role_blocks_instead_of_switching(self):
        wf = parse_workflow_string(MINIMAL, registry=REGISTRY)
        plan = build_plan(wf, registry=REGISTRY, provider_override="antigravity")
        s = stages_by_id(plan)
        self.assertEqual(s["build"]["status"], "assignable")
        self.assertEqual(s["check"]["status"], "blocked")
        self.assertEqual(s["check"]["provider"], "antigravity")
        self.assertTrue(any("not-eligible" in r for r in s["check"]["blocked_reasons"]))
        self.assertTrue(plan["blocked"])

    def test_single_provider_reports_reduced_independence(self):
        plan = build_plan(self.wf, registry=self.registry, provider_override="claude")
        review = stages_by_id(plan)["review"]
        self.assertEqual(review["independence"],
                         [{"of": "implement", "stage_provider": "claude", "other_provider": "claude",
                           "status": "same-provider"}])
        self.assertTrue(any("reduced independence" in w for w in plan["warnings"]))
        self.assertTrue(any("fresh-session" in w for w in plan["warnings"]))


class TestCapabilityAndFallback(unittest.TestCase):

    def test_incompatible_required_skill_blocks_with_report(self):
        wf = parse_workflow_string(MINIMAL_AG_CHECKER, registry=REGISTRY)
        check = stages_by_id(build_plan(wf, registry=REGISTRY))["check"]
        self.assertEqual(check["status"], "blocked")
        self.assertEqual(check["provider"], "antigravity")
        self.assertEqual(check["fallback_trail"], [])
        self.assertEqual(check["incompatibilities"], [{
            "skill": "beta", "provider": "antigravity",
            "checked_adapters": ["antigravity-cli", "antigravity-workspace"],
            "compatible_agents": ["codex-cli", "claude-code"],
        }])

    def test_declared_fallback_is_applied_and_recorded(self):
        wf = parse_workflow_string(MINIMAL_AG_CHECKER + CHECKER_FALLBACK, registry=REGISTRY)
        plan = build_plan(wf, registry=REGISTRY)
        check = stages_by_id(plan)["check"]
        self.assertEqual((check["provider"], check["assignment_source"], check["status"]),
                         ("codex", "fallback", "assignable"))
        self.assertEqual(check["fallback_trail"], [{"from": "antigravity", "to": "codex", "reason": "incompatible"}])
        self.assertEqual(json.dumps(plan, sort_keys=True),
                         json.dumps(build_plan(wf, registry=REGISTRY), sort_keys=True))

    def test_fallback_disabled_under_override_and_route(self):
        wf = parse_workflow_string(MINIMAL_AG_CHECKER + CHECKER_FALLBACK, registry=REGISTRY)
        for kwargs in ({"provider_override": "antigravity"}, {"routes": {"check": "antigravity"}}):
            check = stages_by_id(build_plan(wf, registry=REGISTRY, **kwargs))["check"]
            self.assertEqual(check["provider"], "antigravity")
            self.assertEqual(check["status"], "blocked")
            self.assertEqual(check["fallback_trail"], [])

    def test_unavailable_fallback_requires_detection_and_a_rule(self):
        registry = load_registry(Config.load().registry_path)
        wf = load_workflow(EXAMPLE, registry=registry)
        not_checked = stages_by_id(build_plan(wf, registry=registry))["implement"]
        self.assertEqual((not_checked["provider"], not_checked["fallback_trail"]), ("antigravity", []))

        detected = {"claude": True, "codex": True, "antigravity": False}
        plan = build_plan(wf, registry=registry, detected=detected)
        s = stages_by_id(plan)
        self.assertEqual(plan["detection"], detected)
        self.assertEqual(s["implement"]["provider"], "codex")
        self.assertEqual(s["implement"]["fallback_trail"], [{"from": "antigravity", "to": "codex", "reason": "unavailable"}])

    def test_unavailable_without_rule_is_warning_not_switch(self):
        wf = parse_workflow_string(MINIMAL, registry=REGISTRY)
        plan = build_plan(wf, registry=REGISTRY, detected={"claude": False, "codex": False, "antigravity": False})
        build = stages_by_id(plan)["build"]
        self.assertEqual((build["provider"], build["status"], build["fallback_trail"]), ("antigravity", "assignable", []))
        self.assertTrue(any("not installed" in w for w in build["warnings"]))

    def test_fallback_skips_targets_with_the_same_problem(self):
        text = MINIMAL_AG_CHECKER + (
            '\n[[routing.fallback]]\nrole = "checker"\nfrom = "antigravity"\nto = "codex"\non = ["unavailable"]\n'
            '\n[[routing.fallback]]\nrole = "checker"\nfrom = "antigravity"\nto = "claude"\non = ["unavailable"]\n'
        )
        wf = parse_workflow_string(text, registry=REGISTRY)
        # codex is also unavailable, so the first rule would gain nothing; the second rule applies.
        detected = {"antigravity": False, "codex": False, "claude": True}
        check = stages_by_id(build_plan(wf, registry=REGISTRY, detected=detected))["check"]
        self.assertEqual(check["fallback_trail"], [{"from": "antigravity", "to": "claude", "reason": "unavailable"}])
        self.assertEqual(check["provider"], "claude")
        # No rule leads anywhere better: the preferred provider stays, with a visible warning.
        none_better = {"antigravity": False, "codex": False, "claude": False}
        build = stages_by_id(build_plan(wf, registry=REGISTRY, detected=none_better))["check"]
        self.assertEqual((build["provider"], build["fallback_trail"]), ("antigravity", []))

    def test_multi_provider_review_is_distinct_unless_shared(self):
        registry = load_registry(Config.load().registry_path)
        wf = load_workflow(EXAMPLE, registry=registry)
        review = stages_by_id(build_plan(wf, registry=registry))["review"]
        self.assertEqual(review["independence"][0]["status"], "distinct-provider")
        shared = stages_by_id(build_plan(wf, registry=registry, routes={"implement": "codex"}))["review"]
        self.assertEqual(shared["independence"][0]["status"], "same-provider")

    def test_independence_never_claims_more_than_provider_distinctness(self):
        registry = load_registry(Config.load().registry_path)
        wf = load_workflow(EXAMPLE, registry=registry)
        for kwargs in ({}, {"provider_override": "codex"}, {"routes": {"implement": "codex"}}):
            plan = build_plan(wf, registry=registry, **kwargs)
            text = json.dumps(plan)
            self.assertNotIn('"independent"', text)
            for s in plan["stages"]:
                for entry in s["independence"]:
                    self.assertIn(entry["status"], ("distinct-provider", "same-provider"))


if __name__ == "__main__":
    unittest.main()
