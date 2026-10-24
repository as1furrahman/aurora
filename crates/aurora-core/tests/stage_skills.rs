//! R2 Step 5: catalog skills per stage travel in the plan and appear in handoffs with each
//! CLI's invocation syntax. Plans and handoffs without them retain their original schema.

mod common;

use std::path::Path;

use aurora_core::presets::{PRESETS, get_preset};
use aurora_core::providers::get_provider;
use aurora_core::workflow::handoff::render_handoff;
use aurora_core::workflow::{PlanOptions, build_plan, parse_workflow_str};
use common::repo_registry;
use indexmap::IndexMap;

fn bugfix() -> aurora_core::workflow::WorkflowDefinition {
    let p = get_preset("bugfix").expect("preset");
    parse_workflow_str(p.toml, Some(&repo_registry()), p.name).expect("valid")
}

#[test]
fn stage_skills_are_carried_into_the_plan() {
    let wf = bugfix();
    let mut stage_skills = IndexMap::new();
    stage_skills.insert("review".to_string(), vec!["code-review".to_string(), "find-bugs".to_string()]);
    let opts = PlanOptions { stage_skills, ..Default::default() };
    let plan = build_plan(&wf, Some(&repo_registry()), &opts).expect("plan");
    assert_eq!(plan.stage("review").expect("review").stage_skills, ["code-review", "find-bugs"]);
    assert!(plan.stage("fix").expect("fix").stage_skills.is_empty());
}

#[test]
fn plans_without_stage_skills_serialize_as_before() {
    let plan = build_plan(&bugfix(), Some(&repo_registry()), &PlanOptions::default()).expect("plan");
    let json = serde_json::to_string(&plan).expect("json");
    assert!(!json.contains("stage_skills"), "the field is omitted when empty");
}

#[test]
fn unknown_stages_in_stage_skills_are_rejected() {
    let mut stage_skills = IndexMap::new();
    stage_skills.insert("nope".to_string(), vec!["x".to_string()]);
    let err = build_plan(&bugfix(), Some(&repo_registry()), &PlanOptions { stage_skills, ..Default::default() })
        .expect_err("unknown stage");
    assert!(err.to_string().contains("nope"), "{err}");
}

#[test]
fn handoffs_list_stage_skills_with_each_cli_syntax() {
    let wf = bugfix();
    let mut stage_skills = IndexMap::new();
    stage_skills.insert("review".to_string(), vec!["code-review".to_string()]);
    let mut routes = IndexMap::new();
    for (stage, cli) in [("review", "codex"), ("fix", "claude")] {
        routes.insert(stage.to_string(), cli.to_string());
    }
    stage_skills.insert("fix".to_string(), vec!["test-driven-development".to_string()]);
    let plan = build_plan(&wf, Some(&repo_registry()), &PlanOptions { routes, stage_skills, ..Default::default() })
        .expect("plan");
    let review = render_handoff(&wf, plan.stage("review").expect("review"), Path::new("/runs/r"), "r");
    assert!(review.contains("- Use for this stage: `$code-review`"), "{review}");
    let fix = render_handoff(&wf, plan.stage("fix").expect("fix"), Path::new("/runs/r"), "r");
    assert!(fix.contains("`/test-driven-development`"), "{fix}");
    let accept = render_handoff(&wf, plan.stage("accept").expect("accept"), Path::new("/runs/r"), "r");
    assert!(!accept.contains("Use for this stage"), "no skills, no line");
}

#[test]
fn every_cli_has_a_skill_invocation_form() {
    assert_eq!(get_provider("claude").expect("claude").invoke_skill("x"), "`/x`");
    assert_eq!(get_provider("codex").expect("codex").invoke_skill("x"), "`$x`");
    assert_eq!(get_provider("hermes").expect("hermes").invoke_skill("x"), "`/x`");
    assert!(get_provider("antigravity").expect("agy").invoke_skill("x").contains("by name"));
}

#[test]
fn any_cli_may_play_any_preset_role() {
    // The user's rule: any CLI can take any role. Preferences stay as they were.
    for p in &PRESETS {
        let wf = parse_workflow_str(p.toml, Some(&repo_registry()), p.name).expect("valid");
        for r in &wf.routing.roles {
            for cli in ["claude", "codex", "antigravity", "hermes"] {
                assert!(r.eligible.iter().any(|e| e == cli), "{}: role {} does not allow {cli}", p.name, r.role);
            }
        }
    }
}
