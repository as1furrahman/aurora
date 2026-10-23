//! R2 Step 0: the curated skill catalog (`catalog/catalog.toml`). It references upstream skills
//! by pinned commit, maps them to roles and stages, and recommends a per-stage skill set for a
//! 1-3 CLI profile without exceeding any CLI's skill-listing budget.

use std::collections::BTreeSet;

use aurora_core::catalog::{Catalog, CatalogError, Kind, RecommendRequest, Tier, builtin_catalog, parse_catalog_str};
use aurora_core::presets::PRESETS;
use aurora_core::providers::PROVIDER_IDS;
use aurora_core::workflow::{WorkflowDefinition, parse_workflow_str};

const ROLE_TAXONOMY: [&str; 7] =
    ["analyst", "architect", "implementer", "tester", "reviewer", "security", "documenter"];

fn catalog() -> Catalog {
    builtin_catalog().unwrap_or_else(|e| panic!("built-in catalog is invalid: {e}"))
}

fn preset(name: &str) -> WorkflowDefinition {
    let p = PRESETS.iter().find(|p| p.name == name).expect("preset");
    parse_workflow_str(p.toml, None, p.name).expect("valid preset")
}

fn request(profile: &str) -> RecommendRequest {
    RecommendRequest { profile: profile.into(), clis: Vec::new(), with: Vec::new() }
}

fn validation_errors(text: &str) -> Vec<String> {
    match parse_catalog_str(text) {
        Err(CatalogError::Validation(errors)) => errors,
        other => panic!("expected validation errors, got {other:?}"),
    }
}

/// Smallest valid catalog; tests append to or edit it.
const MINIMAL: &str = r#"
schema_version = 1
verified_on = "2026-10-24"

[sources.up]
repo = "https://github.com/example/skills"
rev = "0123456789abcdef0123456789abcdef01234567"
license = "MIT"
trust = "curated"

[targets.claude]
listing_chars = 8000
per_skill_chars = 1536
rules_file = "CLAUDE.md"
evidence = "https://code.claude.com/docs/en/skills"

[targets.codex]
listing_chars = 8000
rules_file = "AGENTS.md"
evidence = "https://learn.chatgpt.com/docs/build-skills"

[targets.antigravity]
listing_chars = 8000
rules_file = "AGENTS.md"
evidence = "assumed"

[targets.hermes]
listing_chars = 12000
rules_file = "AGENTS.md"
evidence = "agent/coding_context.py"

[profiles.solo]
description = "one"
default_clis = ["claude"]
roles = { analyst = 1, architect = 1, implementer = 1, tester = 1, reviewer = 1, security = 1, documenter = 1 }

[profiles.pair]
description = "two"
default_clis = ["claude", "codex"]
roles = { analyst = 1, architect = 1, implementer = 1, tester = 2, reviewer = 2, security = 1, documenter = 1 }

[[entries]]
id = "verify"
kind = "skill"
source = "up"
path = "skills/verify"
tier = "baseline"
description_chars = 100
why = "evidence before claims"
"#;

#[test]
fn builtin_catalog_is_valid() {
    let cat = catalog();
    assert!(cat.entries().len() >= 30, "only {} entries", cat.entries().len());
    assert_eq!(cat.verified_on(), "2026-10-24");
}

#[test]
fn every_source_is_pinned_licensed_and_on_github() {
    for (id, src) in catalog().sources() {
        assert!(src.repo.starts_with("https://github.com/"), "{id}: {}", src.repo);
        assert!(src.rev.len() == 40 && src.rev.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert!(!src.license.trim().is_empty(), "{id} has no licence");
    }
}

#[test]
fn installable_entries_have_a_path_and_a_measured_description() {
    for e in catalog().entries() {
        match e.kind {
            Kind::Skill | Kind::Rules => {
                let path = e.path.as_deref().unwrap_or_else(|| panic!("{} has no path", e.id));
                assert!(!path.starts_with('/') && !path.contains(".."), "{}: {path}", e.id);
                assert!(e.description_chars > 0, "{}", e.id);
            }
            Kind::Framework | Kind::Infra => assert_eq!(e.tier, Tier::OptIn, "{} must be opt-in", e.id),
        }
        assert!(!e.why.trim().is_empty(), "{} has no rationale", e.id);
    }
}

#[test]
fn roles_and_stages_refer_to_real_preset_stages() {
    let presets: Vec<WorkflowDefinition> = PRESETS.iter().map(|p| preset(p.name)).collect();
    for e in catalog().entries() {
        for role in &e.roles {
            assert!(ROLE_TAXONOMY.contains(&role.as_str()), "{}: role {role}", e.id);
        }
        for stage in &e.stages {
            let ok = presets.iter().flat_map(|wf| &wf.stages).any(|s| &s.id == stage && e.roles.contains(&s.role));
            assert!(ok, "{}: no preset stage '{stage}' played by {:?}", e.id, e.roles);
        }
        if e.tier == Tier::Stage {
            assert!(!e.roles.is_empty(), "stage entry {} names no role", e.id);
        }
    }
}

#[test]
fn conflicts_are_symmetric_and_known() {
    let cat = catalog();
    for e in cat.entries() {
        for other in &e.conflicts_with {
            let o = cat.get(other).unwrap_or_else(|| panic!("{} conflicts with unknown {other}", e.id));
            assert!(o.conflicts_with.contains(&e.id), "{} -> {other} is not symmetric", e.id);
        }
    }
}

#[test]
fn profiles_cover_every_role_with_known_providers() {
    let cat = catalog();
    for name in ["solo", "pair", "trio"] {
        let p = cat.profile(name).unwrap_or_else(|| panic!("missing profile {name}"));
        assert!(!p.description.is_empty());
        for cli in &p.default_clis {
            assert!(PROVIDER_IDS.contains(&cli.as_str()), "{name}: {cli}");
        }
        for role in ROLE_TAXONOMY {
            let slot = p.roles.get(role).copied().unwrap_or_else(|| panic!("{name} has no slot for {role}"));
            assert!((1..=p.default_clis.len()).contains(&slot), "{name}.{role} = {slot}");
        }
    }
    assert_eq!(cat.profile("pair").expect("pair").default_clis, ["claude", "codex"]);
}

#[test]
fn every_preset_and_profile_recommends_within_budget_and_without_conflicts() {
    let cat = catalog();
    for p in &PRESETS {
        let wf = preset(p.name);
        for profile in ["solo", "pair", "trio"] {
            let rec = cat.recommend(&wf, &request(profile)).unwrap_or_else(|e| panic!("{}/{profile}: {e}", p.name));
            assert_eq!(rec.stages.len(), wf.stages.len());
            for stage in &rec.stages {
                assert!(!stage.skills.is_empty(), "{}/{profile}: stage {} has no skills", p.name, stage.stage);
            }
            for load in &rec.providers {
                assert!(load.listing_chars <= load.budget_chars, "{}/{profile}: {load:?}", p.name);
                for a in &load.skills {
                    let conflicts = &cat.get(a).expect("entry").conflicts_with;
                    assert!(!load.skills.iter().any(|b| conflicts.contains(b)), "{a} conflicts on {}", load.provider);
                }
            }
            assert!(rec.rules.iter().any(|r| r.entry == "karpathy-guidelines"), "baseline rules missing");
        }
    }
}

#[test]
fn pair_profile_routes_review_away_from_implementation() {
    let rec = catalog().recommend(&preset("bugfix"), &request("pair")).expect("recommend");
    let stage = |id: &str| rec.stages.iter().find(|s| s.stage == id).expect("stage");
    assert_eq!(stage("fix").provider, "claude");
    assert_eq!(stage("review").provider, "codex");
    assert!(stage("review").skills.contains(&"code-review".to_string()), "{:?}", stage("review").skills);
    assert!(stage("root-cause").skills.contains(&"diagnosing-bugs".to_string()));
    assert!(stage("fix").skills.contains(&"verification-before-completion".to_string()), "baseline everywhere");
    assert!(!rec.warnings.iter().any(|w| w.contains("independence")), "{:?}", rec.warnings);
}

#[test]
fn solo_profile_reports_reduced_independence() {
    let rec = catalog().recommend(&preset("bugfix"), &request("solo")).expect("recommend");
    assert!(rec.stages.iter().all(|s| s.provider == "claude"));
    assert!(rec.warnings.iter().any(|w| w.contains("reduced independence")), "{:?}", rec.warnings);
}

#[test]
fn clis_override_fills_profile_slots_in_order() {
    let req = RecommendRequest { profile: "pair".into(), clis: vec!["codex".into(), "claude".into()], with: vec![] };
    let rec = catalog().recommend(&preset("bugfix"), &req).expect("recommend");
    assert_eq!(rec.stages.iter().find(|s| s.stage == "fix").expect("fix").provider, "codex");
    assert_eq!(rec.clis, ["codex", "claude"]);
}

#[test]
fn ineligible_provider_is_a_warning_with_a_fix() {
    let req = RecommendRequest { profile: "pair".into(), clis: vec!["claude".into(), "hermes".into()], with: vec![] };
    let rec = catalog().recommend(&preset("bugfix"), &req).expect("recommend");
    let w = rec.warnings.iter().find(|w| w.contains("not eligible")).expect("eligibility warning");
    assert!(w.contains("hermes") && w.contains("eligible"), "{w}");
}

#[test]
fn bad_requests_fail_clearly() {
    let cat = catalog();
    let wf = preset("bugfix");
    let err = |req: RecommendRequest| cat.recommend(&wf, &req).expect_err("rejected").to_string();
    assert!(err(request("quartet")).contains("quartet"));
    let wrong_count = RecommendRequest { profile: "pair".into(), clis: vec!["claude".into()], with: vec![] };
    assert!(err(wrong_count).contains("2 CLIs"));
    let unknown =
        RecommendRequest { profile: "pair".into(), clis: vec!["claude".into(), "gpt-cli".into()], with: vec![] };
    assert!(err(unknown).contains("gpt-cli"));
    let missing = RecommendRequest { profile: "pair".into(), clis: vec![], with: vec!["nope".into()] };
    assert!(err(missing).contains("nope"));
    let infra = RecommendRequest { profile: "pair".into(), clis: vec![], with: vec!["headroom".into()] };
    assert!(err(infra).contains("not installed by Aurora"));
}

#[test]
fn opt_in_entries_appear_only_when_requested() {
    let cat = catalog();
    let wf = preset("bugfix");
    let all = |rec: &aurora_core::catalog::Recommendation| -> BTreeSet<String> {
        rec.stages.iter().flat_map(|s| s.skills.iter().cloned()).collect()
    };
    let plain = cat.recommend(&wf, &request("pair")).expect("plain");
    assert!(!all(&plain).contains("caveman"));
    let req = RecommendRequest { profile: "pair".into(), clis: vec![], with: vec!["caveman".into()] };
    let with = cat.recommend(&wf, &req).expect("with caveman");
    assert!(all(&with).contains("caveman"));
    let stage = with.stages.iter().find(|s| s.stage == "reproduce").expect("reproduce");
    assert!(!stage.skills.contains(&"caveman".to_string()), "caveman is scoped to the implementer role");
}

#[test]
fn hooks_are_reported_as_claude_only_on_other_clis() {
    // Codex builds here, so the hook-dependent skill lands on a CLI that ignores hooks.
    let clis = vec!["codex".into(), "claude".into()];
    let req = RecommendRequest { profile: "pair".into(), clis, with: vec!["planning-with-files".into()] };
    let rec = catalog().recommend(&preset("feature"), &req).expect("recommend");
    assert!(!rec.warnings.iter().any(|w| w.contains("hooks") && w.contains("on claude")), "{:?}", rec.warnings);
    assert!(
        rec.warnings.iter().any(|w| w.contains("planning-with-files") && w.contains("hooks") && w.contains("codex")),
        "{:?}",
        rec.warnings
    );
}

#[test]
fn minimal_catalog_parses() {
    let cat = parse_catalog_str(MINIMAL).expect("minimal catalog");
    assert_eq!(cat.entries().len(), 1);
}

#[test]
fn listing_cost_caps_each_description_per_cli() {
    let text = MINIMAL.replace("description_chars = 100", "description_chars = 5000");
    let cat = parse_catalog_str(&text).expect("parses");
    let wf = preset("bugfix");
    let rec = cat.recommend(&wf, &request("pair")).expect("recommend");
    let load = |p: &str| rec.providers.iter().find(|l| l.provider == p).expect("load").listing_chars;
    // claude truncates the description at 1536 characters; codex has no per-skill cap.
    assert!(load("claude") < 1600, "{}", load("claude"));
    assert!(load("codex") > 5000, "{}", load("codex"));
}

#[test]
fn budget_overflow_is_a_warning_not_an_error() {
    let text = MINIMAL.replace(
        "listing_chars = 8000\nrules_file = \"AGENTS.md\"\nevidence = \"https://learn",
        "listing_chars = 10\nrules_file = \"AGENTS.md\"\nevidence = \"https://learn",
    );
    let cat = parse_catalog_str(&text).expect("parses");
    let rec = cat.recommend(&preset("bugfix"), &request("pair")).expect("recommend");
    assert!(rec.warnings.iter().any(|w| w.contains("codex") && w.contains("budget")), "{:?}", rec.warnings);
}

#[test]
fn validation_rejects_bad_catalogs() {
    let bad_rev = MINIMAL.replace("0123456789abcdef0123456789abcdef01234567", "main");
    assert!(validation_errors(&bad_rev).iter().any(|e| e.contains("rev")));

    let dangling = MINIMAL.replace("source = \"up\"", "source = \"elsewhere\"");
    assert!(validation_errors(&dangling).iter().any(|e| e.contains("elsewhere")));

    let escape = MINIMAL.replace("path = \"skills/verify\"", "path = \"../etc\"");
    assert!(validation_errors(&escape).iter().any(|e| e.contains("path")));

    let one_way = format!(
        "{MINIMAL}\n[[entries]]\nid = \"other\"\nkind = \"skill\"\nsource = \"up\"\npath = \"skills/o\"\ntier = \"baseline\"\n\
         description_chars = 5\nwhy = \"x\"\nconflicts_with = [\"verify\"]\n"
    );
    assert!(validation_errors(&one_way).iter().any(|e| e.contains("symmetric")));

    let dup = format!(
        "{MINIMAL}\n[[entries]]\nid = \"verify\"\nkind = \"skill\"\nsource = \"up\"\npath = \"skills/v2\"\ntier = \"baseline\"\n\
         description_chars = 5\nwhy = \"x\"\n"
    );
    assert!(validation_errors(&dup).iter().any(|e| e.contains("duplicate")));

    let bad_slot = MINIMAL.replace("tester = 2, reviewer = 2", "tester = 3, reviewer = 2");
    assert!(validation_errors(&bad_slot).iter().any(|e| e.contains("slot")));
}

#[test]
fn unknown_keys_are_rejected() {
    let typo = MINIMAL.replace("why = \"evidence before claims\"", "why = \"x\"\nrole = [\"tester\"]");
    assert!(matches!(parse_catalog_str(&typo), Err(CatalogError::Parse(_))));
}
