//! Routing semantics: multi-provider routing vs single-provider override, capability checks,
//! declared fallback and independence reporting (ADR-0004). Ported from
//! `tests/test_workflow_routing.py`. (The Python "planning performs no writes" test is covered
//! structurally by `tests/purity.rs`.)

mod common;

use aurora_core::registry::Registry;
use aurora_core::workflow::planner::{FallbackStep, Independence, REDUCED_INDEPENDENCE_WARNING};
use aurora_core::workflow::{Detection, Plan, PlanOptions, WorkflowDefinition, build_plan, parse_workflow_str};
use common::{MINIMAL, example_path, read, registry, repo_registry};
use indexmap::IndexMap;

const CHECKER_FALLBACK: &str =
    "\n[[routing.fallback]]\nrole = \"checker\"\nfrom = \"antigravity\"\nto = \"codex\"\non = [\"incompatible\"]\n";

fn minimal_ag_checker() -> String {
    MINIMAL.replace(
        "eligible = [\"codex\", \"claude\"]\npreferred = \"codex\"",
        "eligible = [\"antigravity\", \"codex\", \"claude\"]\npreferred = \"antigravity\"",
    )
}

fn example() -> (WorkflowDefinition, Registry) {
    let reg = repo_registry();
    let wf = aurora_core::workflow::parse_workflow_bytes(&read(&example_path()), Some(&reg), "example").expect("valid");
    (wf, reg)
}

fn mini(text: &str) -> (WorkflowDefinition, Registry) {
    let reg = registry();
    (parse_workflow_str(text, Some(&reg), "mini").expect("valid"), reg)
}

fn routes(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs.iter().map(|(s, p)| ((*s).to_string(), (*p).to_string())).collect()
}

fn plan(wf: &WorkflowDefinition, reg: &Registry, opts: PlanOptions) -> Plan {
    build_plan(wf, Some(reg), &opts).expect("plan builds")
}

fn detection(pairs: &[(&str, bool)]) -> Detection {
    pairs.iter().map(|(k, v)| ((*k).to_string(), *v)).collect()
}

fn step(from: &str, to: &str, reason: &str) -> FallbackStep {
    FallbackStep { from: from.into(), to: to.into(), reason: reason.into() }
}

// --- multi-provider routing ---------------------------------------------------

#[test]
fn each_stage_gets_its_preferred_provider() {
    let (wf, reg) = example();
    let p = plan(&wf, &reg, PlanOptions::default());
    assert_eq!(p.mode, "multi");
    let got: Vec<(&str, &str, &str)> =
        p.stages.iter().map(|s| (s.id.as_str(), s.provider.as_str(), s.assignment_source.as_str())).collect();
    assert_eq!(
        got,
        [
            ("plan", "claude", "preferred"),
            ("implement", "antigravity", "preferred"),
            ("test", "codex", "preferred"),
            ("review", "codex", "preferred"),
            ("remediate", "antigravity", "preferred"),
            ("final-verify", "codex", "preferred"),
        ]
    );
    assert!(!p.blocked);
    assert!(p.stages.iter().all(|s| s.status == "assignable"));
}

#[test]
fn route_overrides_one_stage_only() {
    let (wf, reg) = example();
    let p = plan(&wf, &reg, PlanOptions { routes: routes(&[("implement", "codex")]), ..Default::default() });
    let s = |id| p.stage(id).expect("stage");
    assert_eq!((s("implement").provider.as_str(), s("implement").assignment_source.as_str()), ("codex", "route"));
    assert_eq!(s("remediate").provider, "antigravity");
    assert_eq!(s("plan").provider, "claude");
    assert_eq!(p.routes, routes(&[("implement", "codex")]));
}

#[test]
fn unknown_provider_fails_clearly() {
    let (wf, reg) = example();
    for opts in [
        PlanOptions { provider_override: Some("gpt-cli"), ..Default::default() },
        PlanOptions { routes: routes(&[("plan", "gpt-cli")]), ..Default::default() },
    ] {
        let msg = build_plan(&wf, Some(&reg), &opts).expect_err("unknown provider").to_string();
        assert!(msg.contains("gpt-cli") && msg.contains("claude"), "{msg}");
    }
}

#[test]
fn route_to_unknown_stage_fails() {
    let (wf, reg) = example();
    let opts = PlanOptions { routes: routes(&[("deploy", "codex")]), ..Default::default() };
    assert!(build_plan(&wf, Some(&reg), &opts).expect_err("unknown stage").to_string().contains("deploy"));
}

#[test]
fn ineligible_route_fails_clearly() {
    let (wf, reg) = mini(MINIMAL);
    let opts = PlanOptions { routes: routes(&[("check", "antigravity")]), ..Default::default() };
    let msg = build_plan(&wf, Some(&reg), &opts).expect_err("ineligible").to_string();
    assert!(msg.contains("not eligible") && msg.contains("checker"), "{msg}");
}

#[test]
fn override_and_routes_are_mutually_exclusive() {
    let (wf, reg) = example();
    let opts =
        PlanOptions { provider_override: Some("codex"), routes: routes(&[("plan", "claude")]), ..Default::default() };
    assert!(build_plan(&wf, Some(&reg), &opts).is_err());
}

#[test]
fn overrides_rejected_when_workflow_forbids_them() {
    let (wf, reg) = mini(&MINIMAL.replace("allow_override = true", "allow_override = false"));
    for opts in [
        PlanOptions { provider_override: Some("codex"), ..Default::default() },
        PlanOptions { routes: routes(&[("build", "codex")]), ..Default::default() },
    ] {
        assert!(build_plan(&wf, Some(&reg), &opts).expect_err("forbidden").to_string().contains("allow_override"));
    }
}

#[test]
fn order_is_topological_with_declaration_tiebreak() {
    let parts: Vec<&str> = MINIMAL.split("[[stages]]").collect();
    let build_block = parts[1];
    let check_block = parts[2].split("[routing]").next().expect("check block");
    let reordered = MINIMAL
        .replace(&format!("[[stages]]{build_block}"), "")
        .replace(&format!("[[stages]]{check_block}"), &format!("[[stages]]{check_block}[[stages]]{build_block}"));
    let (wf, reg) = mini(&reordered);
    assert_eq!(wf.stages.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["check", "build"]);
    assert_eq!(plan(&wf, &reg, PlanOptions::default()).order, ["build", "check"]);
}

#[test]
fn plan_is_deterministic() {
    let (wf, reg) = example();
    let mk = || {
        serde_json::to_string(&plan(
            &wf,
            &reg,
            PlanOptions { routes: routes(&[("implement", "codex")]), ..Default::default() },
        ))
        .expect("json")
    };
    assert_eq!(mk(), mk());
}

#[test]
fn plan_exposes_handoff_and_output_paths() {
    let (wf, reg) = example();
    let p = plan(&wf, &reg, PlanOptions::default());
    let review = p.stage("review").expect("review");
    assert_eq!(review.handoff_path, "handoff/review.md");
    assert_eq!(review.outputs, ["artifacts/review/review.md"]);
    assert!(review.inputs.iter().any(|i| i == "artifacts/test/test-report.md"));
    assert_eq!(review.acceptance[0].evidence, ["artifacts/review/review.md"]);
}

// --- single-provider override -------------------------------------------------

#[test]
fn override_maps_every_eligible_stage() {
    let (wf, reg) = example();
    for provider in ["codex", "claude", "antigravity"] {
        let p = plan(&wf, &reg, PlanOptions { provider_override: Some(provider), ..Default::default() });
        assert_eq!(p.mode, "single");
        assert_eq!(p.provider_override.as_deref(), Some(provider));
        for s in &p.stages {
            assert_eq!((s.provider.as_str(), s.assignment_source.as_str()), (provider, "override"));
            assert!(s.fallback_trail.is_empty());
        }
    }
}

#[test]
fn override_preserves_roles_and_gates() {
    let (wf, reg) = example();
    let multi = plan(&wf, &reg, PlanOptions::default());
    let single = plan(&wf, &reg, PlanOptions { provider_override: Some("codex"), ..Default::default() });
    for (m, s) in multi.stages.iter().zip(&single.stages) {
        assert_eq!(
            (&m.role, &m.responsibility, &m.required_skills, &m.acceptance, &m.outputs, &m.depends_on),
            (&s.role, &s.responsibility, &s.required_skills, &s.acceptance, &s.outputs, &s.depends_on),
            "{}",
            m.id
        );
        assert_eq!(
            (m.requires_approval, m.optional, m.max_attempts),
            (s.requires_approval, s.optional, s.max_attempts)
        );
    }
    assert_eq!(wf.stage("review").expect("review").role, "reviewer");
}

#[test]
fn override_to_ineligible_role_blocks_instead_of_switching() {
    let (wf, reg) = mini(MINIMAL);
    let p = plan(&wf, &reg, PlanOptions { provider_override: Some("antigravity"), ..Default::default() });
    assert_eq!(p.stage("build").expect("build").status, "assignable");
    let check = p.stage("check").expect("check");
    assert_eq!((check.status.as_str(), check.provider.as_str()), ("blocked", "antigravity"));
    assert!(check.blocked_reasons.iter().any(|r| r.contains("not-eligible")));
    assert!(p.blocked);
}

#[test]
fn single_provider_reports_reduced_independence() {
    let (wf, reg) = example();
    let p = plan(&wf, &reg, PlanOptions { provider_override: Some("claude"), ..Default::default() });
    assert_eq!(
        p.stage("review").expect("review").independence,
        [Independence {
            of: "implement".into(),
            stage_provider: "claude".into(),
            other_provider: "claude".into(),
            status: "same-provider".into(),
        }]
    );
    assert!(p.warnings.iter().any(|w| w == REDUCED_INDEPENDENCE_WARNING));
    assert!(
        REDUCED_INDEPENDENCE_WARNING.contains("reduced independence")
            && REDUCED_INDEPENDENCE_WARNING.contains("fresh-session")
    );
}

// --- capability checks and fallback -------------------------------------------

#[test]
fn incompatible_required_skill_blocks_with_report() {
    let (wf, reg) = mini(&minimal_ag_checker());
    let p = plan(&wf, &reg, PlanOptions::default());
    let check = p.stage("check").expect("check");
    assert_eq!((check.status.as_str(), check.provider.as_str()), ("blocked", "antigravity"));
    assert!(check.fallback_trail.is_empty());
    let i = &check.incompatibilities[0];
    assert_eq!((i.skill.as_str(), i.provider.as_str()), ("beta", "antigravity"));
    assert_eq!(i.checked_adapters, ["antigravity-cli", "antigravity-workspace"]);
    assert_eq!(i.compatible_agents, ["codex-cli", "claude-code"]);
}

#[test]
fn declared_fallback_is_applied_and_recorded() {
    let (wf, reg) = mini(&(minimal_ag_checker() + CHECKER_FALLBACK));
    let p = plan(&wf, &reg, PlanOptions::default());
    let check = p.stage("check").expect("check");
    assert_eq!(
        (check.provider.as_str(), check.assignment_source.as_str(), check.status.as_str()),
        ("codex", "fallback", "assignable")
    );
    assert_eq!(check.fallback_trail, [step("antigravity", "codex", "incompatible")]);
}

#[test]
fn fallback_disabled_under_override_and_route() {
    let (wf, reg) = mini(&(minimal_ag_checker() + CHECKER_FALLBACK));
    for opts in [
        PlanOptions { provider_override: Some("antigravity"), ..Default::default() },
        PlanOptions { routes: routes(&[("check", "antigravity")]), ..Default::default() },
    ] {
        let p = plan(&wf, &reg, opts);
        let check = p.stage("check").expect("check");
        assert_eq!((check.provider.as_str(), check.status.as_str()), ("antigravity", "blocked"));
        assert!(check.fallback_trail.is_empty());
    }
}

#[test]
fn unavailable_fallback_requires_detection_and_a_rule() {
    let (wf, reg) = example();
    let not_checked = plan(&wf, &reg, PlanOptions::default());
    let implement = not_checked.stage("implement").expect("implement");
    assert_eq!(implement.provider, "antigravity");
    assert!(implement.fallback_trail.is_empty());

    let detected = detection(&[("claude", true), ("codex", true), ("antigravity", false)]);
    let p = plan(&wf, &reg, PlanOptions { detected: Some(&detected), ..Default::default() });
    assert_eq!(p.detection.as_ref(), Some(&detected));
    let implement = p.stage("implement").expect("implement");
    assert_eq!(implement.provider, "codex");
    assert_eq!(implement.fallback_trail, [step("antigravity", "codex", "unavailable")]);
}

#[test]
fn unavailable_without_rule_is_warning_not_switch() {
    let (wf, reg) = mini(MINIMAL);
    let none = detection(&[("claude", false), ("codex", false), ("antigravity", false)]);
    let p = plan(&wf, &reg, PlanOptions { detected: Some(&none), ..Default::default() });
    let build = p.stage("build").expect("build");
    assert_eq!((build.provider.as_str(), build.status.as_str()), ("antigravity", "assignable"));
    assert!(build.fallback_trail.is_empty());
    assert!(build.warnings.iter().any(|w| w.contains("not installed")));
}

#[test]
fn fallback_skips_targets_with_the_same_problem() {
    let text = minimal_ag_checker()
        + "\n[[routing.fallback]]\nrole = \"checker\"\nfrom = \"antigravity\"\nto = \"codex\"\non = [\"unavailable\"]\n"
        + "\n[[routing.fallback]]\nrole = \"checker\"\nfrom = \"antigravity\"\nto = \"claude\"\non = [\"unavailable\"]\n";
    let (wf, reg) = mini(&text);
    let some = detection(&[("antigravity", false), ("codex", false), ("claude", true)]);
    let p = plan(&wf, &reg, PlanOptions { detected: Some(&some), ..Default::default() });
    let check = p.stage("check").expect("check");
    assert_eq!(check.fallback_trail, [step("antigravity", "claude", "unavailable")]);
    assert_eq!(check.provider, "claude");

    let none = detection(&[("antigravity", false), ("codex", false), ("claude", false)]);
    let p = plan(&wf, &reg, PlanOptions { detected: Some(&none), ..Default::default() });
    let check = p.stage("check").expect("check");
    assert_eq!(check.provider, "antigravity");
    assert!(check.fallback_trail.is_empty());
}

#[test]
fn multi_provider_review_is_distinct_unless_shared() {
    let (wf, reg) = example();
    let p = plan(&wf, &reg, PlanOptions::default());
    assert_eq!(p.stage("review").expect("review").independence[0].status, "distinct-provider");
    let shared = plan(&wf, &reg, PlanOptions { routes: routes(&[("implement", "codex")]), ..Default::default() });
    assert_eq!(shared.stage("review").expect("review").independence[0].status, "same-provider");
}

#[test]
fn independence_never_claims_more_than_provider_distinctness() {
    let (wf, reg) = example();
    for opts in [
        PlanOptions::default(),
        PlanOptions { provider_override: Some("codex"), ..Default::default() },
        PlanOptions { routes: routes(&[("implement", "codex")]), ..Default::default() },
    ] {
        let p = plan(&wf, &reg, opts);
        assert!(!serde_json::to_string(&p).expect("json").contains("\"independent\""));
        for s in &p.stages {
            for e in &s.independence {
                assert!(["distinct-provider", "same-provider"].contains(&e.status.as_str()));
            }
        }
    }
}

#[test]
fn missing_registry_is_reported_not_assumed_compatible() {
    let (wf, _) = example();
    let p = build_plan(&wf, None, &PlanOptions::default()).expect("plan");
    assert!(p.warnings[0].contains("compatibility was not checked"));
}

#[test]
fn parallel_ready_stages_follow_declaration_order() {
    let reg = registry();
    let raw = read(&common::fixture_path("parallel.toml"));
    let wf = aurora_core::workflow::parse_workflow_bytes(&raw, Some(&reg), "parallel").expect("valid");
    let p = plan(&wf, &reg, PlanOptions::default());
    // Not alphabetical, not reversed: the declaration order of simultaneously-ready stages.
    assert_eq!(p.order, ["build", "zeta-docs", "alpha-lint", "merge"]);
}
