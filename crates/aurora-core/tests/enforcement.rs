//! Step 6.2 — enforcement strength (after AWS CAO's tool-restriction table): roles may require
//! `read_only` / `tool_restriction`; the plan records how strongly the assigned provider can
//! enforce each requirement, warns when it is only soft, and the handoff says how to start the
//! CLI in the restricted mode. Additive: definitions without `requires` plan exactly as before.

mod common;

use aurora_core::providers::{CAPABILITIES, PROVIDERS, Strength};
use aurora_core::workflow::handoff::render_handoff;
use aurora_core::workflow::{PlanOptions, build_plan, describe, parse_workflow_str};
use common::{MINIMAL, registry};

fn with_requires(caps: &str) -> String {
    MINIMAL.replace("responsibility = \"Check it.\"", &format!("responsibility = \"Check it.\"\nrequires = [{caps}]"))
}

#[test]
fn every_provider_declares_every_capability_with_a_source() {
    for p in &PROVIDERS {
        for cap in CAPABILITIES {
            let e = p.enforcement(cap).unwrap_or_else(|| panic!("{} lacks {cap}", p.id));
            assert!(e.source.starts_with("https://"), "{} {cap}", p.id);
            assert!(!e.how.is_empty());
            for flag in p.forbidden_flags {
                assert!(!e.how.contains(flag), "{} {cap} suggests forbidden {flag}", p.id);
            }
        }
    }
}

#[test]
fn requires_accepts_known_capabilities_only() {
    let reg = registry();
    let wf = parse_workflow_str(&with_requires("\"read_only\""), Some(&reg), "t").expect("valid");
    assert_eq!(wf.role("checker").expect("role").requires, ["read_only"]);
    let err = parse_workflow_str(&with_requires("\"telepathy\""), Some(&reg), "t").expect_err("unknown cap");
    assert!(err.to_string().contains("telepathy") && err.to_string().contains("read_only"), "{err}");
}

#[test]
fn hard_enforcement_is_recorded_without_warnings() {
    let reg = registry();
    let wf = parse_workflow_str(&with_requires("\"read_only\""), Some(&reg), "t").expect("valid");
    for provider in ["codex", "claude"] {
        let opts = PlanOptions {
            routes: [("check".to_string(), provider.to_string())].into_iter().collect(),
            ..Default::default()
        };
        let plan = build_plan(&wf, Some(&reg), &opts).expect("plan");
        let check = plan.stage("check").expect("check");
        assert_eq!(check.enforcement.len(), 1);
        assert_eq!(
            (check.enforcement[0].capability.as_str(), check.enforcement[0].strength),
            ("read_only", Strength::Hard)
        );
        assert!(check.warnings.iter().all(|w| !w.contains("softly")), "{provider}: {:?}", check.warnings);
    }
}

#[test]
fn soft_enforcement_warns_and_is_never_silent() {
    let reg = registry();
    let text = with_requires("\"read_only\"")
        .replace("eligible = [\"codex\", \"claude\"]", "eligible = [\"codex\", \"claude\", \"antigravity\"]");
    let wf = parse_workflow_str(&text, Some(&reg), "t").expect("valid");
    let opts = PlanOptions { provider_override: Some("antigravity"), ..Default::default() };
    let plan = build_plan(&wf, Some(&reg), &opts).expect("plan");
    let check = plan.stage("check").expect("check");
    assert_eq!(check.enforcement[0].strength, Strength::Soft);
    assert!(check.warnings.iter().any(|w| w.contains("'read_only'") && w.contains("softly")), "{:?}", check.warnings);
}

#[test]
fn handoff_lists_required_safeguards_only_when_required() {
    let reg = registry();
    let wf = parse_workflow_str(&with_requires("\"read_only\", \"tool_restriction\""), Some(&reg), "t").expect("valid");
    let plan = build_plan(&wf, Some(&reg), &PlanOptions::default()).expect("plan");
    let bundle = render_handoff(&wf, plan.stage("check").expect("check"), std::path::Path::new("/r"), "x");
    assert!(bundle.contains("## Required safeguards"), "{bundle}");
    assert!(bundle.contains("read_only (hard):") && bundle.contains("--sandbox read-only"), "{bundle}");
    let build_bundle = render_handoff(&wf, plan.stage("build").expect("build"), std::path::Path::new("/r"), "x");
    assert!(!build_bundle.contains("Required safeguards"));
}

#[test]
fn definitions_without_requires_serialize_as_before() {
    let reg = registry();
    let wf = parse_workflow_str(MINIMAL, Some(&reg), "t").expect("valid");
    let plan = serde_json::to_value(build_plan(&wf, Some(&reg), &PlanOptions::default()).expect("plan")).expect("json");
    assert!(plan["stages"][1].get("enforcement").is_none());
    let shown = serde_json::to_value(describe(&wf)).expect("json");
    assert!(shown["roles"]["checker"].get("requires").is_none());
}
