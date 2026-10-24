//! Step 6.3 — built-in workflow presets (feature, bugfix, refactor, docs, security-review,
//! spike). Every preset must validate against the shipped registry, plan without blocked
//! stages under every routing mode, and default review/verification to a provider distinct
//! from implementation.

mod common;

use aurora_core::presets::{PRESETS, get_preset};
use aurora_core::workflow::{PlanOptions, build_plan, parse_workflow_str};
use common::repo_registry;

const ROLE_TAXONOMY: [&str; 7] =
    ["analyst", "architect", "implementer", "tester", "reviewer", "security", "documenter"];

#[test]
fn ships_the_six_presets() {
    let names: Vec<&str> = PRESETS.iter().map(|p| p.name).collect();
    assert_eq!(names, ["feature", "bugfix", "refactor", "docs", "security-review", "spike"]);
    assert!(get_preset("feature").is_some() && get_preset("waterfall").is_none());
}

#[test]
fn presets_validate_cleanly_and_use_the_role_taxonomy() {
    let reg = repo_registry();
    for preset in &PRESETS {
        let wf = parse_workflow_str(preset.toml, Some(&reg), preset.name).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(wf.id, preset.name);
        assert!(wf.warnings.is_empty(), "{}: {:?}", preset.name, wf.warnings);
        assert!(!preset.summary.is_empty() && preset.summary == wf.description);
        for role in &wf.roles {
            assert!(ROLE_TAXONOMY.contains(&role.id.as_str()), "{}: role {}", preset.name, role.id);
        }
    }
}

#[test]
fn presets_plan_under_every_routing_mode() {
    let reg = repo_registry();
    for preset in &PRESETS {
        let wf = parse_workflow_str(preset.toml, Some(&reg), preset.name).expect("valid");
        for override_ in [None, Some("claude"), Some("codex"), Some("antigravity")] {
            let plan = build_plan(&wf, Some(&reg), &PlanOptions { provider_override: override_, ..Default::default() })
                .expect("plan");
            assert!(!plan.blocked, "{} blocked under {override_:?}", preset.name);
        }
    }
}

#[test]
fn review_defaults_to_a_distinct_provider() {
    let reg = repo_registry();
    for preset in &PRESETS {
        let wf = parse_workflow_str(preset.toml, Some(&reg), preset.name).expect("valid");
        let plan = build_plan(&wf, Some(&reg), &PlanOptions::default()).expect("plan");
        for stage in &plan.stages {
            for entry in &stage.independence {
                assert_eq!(entry.status, "distinct-provider", "{}: {} vs {}", preset.name, stage.id, entry.of);
            }
        }
    }
}

#[test]
fn reviewing_roles_require_read_only_and_emit_verdicts() {
    let reg = repo_registry();
    for preset in &PRESETS {
        let wf = parse_workflow_str(preset.toml, Some(&reg), preset.name).expect("valid");
        for stage in &wf.stages {
            if stage.role == "reviewer" || stage.role == "security" {
                assert!(wf.role(&stage.role).expect("role").requires.contains(&"read_only".to_string()));
            }
            if stage.role == "reviewer" {
                assert!(stage.verdict.is_some(), "{}: {} has no verdict", preset.name, stage.id);
            }
        }
        assert!(wf.stages.iter().any(|s| s.requires_approval), "{}: no human acceptance gate", preset.name);
    }
}

#[test]
fn preset_files_match_embedded_copies() {
    for preset in &PRESETS {
        let path = common::repo_root().join("workflows/presets").join(format!("{}.toml", preset.name));
        assert_eq!(std::fs::read_to_string(path).expect("preset file"), preset.toml);
    }
}
