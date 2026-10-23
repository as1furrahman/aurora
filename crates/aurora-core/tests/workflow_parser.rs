//! Strict workflow definition parsing (schema v1). Ported case-by-case from
//! `tests/test_workflow_parser.py`; uses the same fixtures.

mod common;

use aurora_core::registry::Registry;
use aurora_core::workflow::{StageState, WorkflowValidationError, parse_workflow_bytes, parse_workflow_str};
use common::{MINIMAL, example_path, fixture_path, read, registry, repo_registry};
use sha2::{Digest, Sha256};

fn parse(text: &str) -> Result<aurora_core::workflow::WorkflowDefinition, WorkflowValidationError> {
    parse_workflow_str(text, Some(&registry()), "<test>")
}

fn invalid_with(text: &str, reg: Option<&Registry>, fragments: &[&str]) -> WorkflowValidationError {
    let err = parse_workflow_str(text, reg, "<test>").expect_err("definition should be invalid");
    let message = err.to_string();
    for fragment in fragments {
        assert!(message.contains(fragment), "missing {fragment:?} in:\n{message}");
    }
    err
}

fn invalid(text: &str, fragments: &[&str]) -> WorkflowValidationError {
    let reg = registry();
    invalid_with(text, Some(&reg), fragments)
}

// --- valid definitions ------------------------------------------------------

#[test]
fn example_workflow_validates_against_repo_registry() {
    let raw = read(&example_path());
    let wf = parse_workflow_bytes(&raw, Some(&repo_registry()), "example").expect("example is valid");
    assert_eq!(wf.id, "plan-implement-review");
    let ids: Vec<&str> = wf.stages.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(ids, ["plan", "implement", "test", "review", "remediate", "final-verify"]);
    let expected: String = Sha256::digest(&raw).iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(wf.sha256, expected);
    assert!(wf.stage("remediate").expect("stage").optional);
    assert!(wf.stage("final-verify").expect("stage").requires_approval);
    assert!(wf.warnings.is_empty());
}

#[test]
fn minimal_defaults() {
    let wf = parse(MINIMAL).expect("valid");
    let build = wf.stage("build").expect("stage");
    assert_eq!(build.max_attempts, 2);
    assert_eq!(build.on_exhausted, "escalate");
    assert!(!build.optional);
    assert!(!build.requires_approval);
    assert_eq!(wf.routing.role("builder").expect("routing").preferred, "antigravity");
}

#[test]
fn preferred_defaults_to_first_eligible() {
    let text = MINIMAL
        .replace("eligible = [\"codex\", \"claude\"]\npreferred = \"codex\"", "eligible = [\"claude\", \"codex\"]");
    let wf = parse(&text).expect("valid");
    assert_eq!(wf.routing.role("checker").expect("routing").preferred, "claude");
}

#[test]
fn roles_carry_no_provider_information() {
    // Structural: Role has exactly these fields; providers live only in RoutingPolicy.
    let wf = parse(MINIMAL).expect("valid");
    let role = wf.role("builder").expect("role");
    let aurora_core::workflow::Role { id, responsibility, required_skills, recommended_skills } = role.clone();
    assert_eq!((id.as_str(), responsibility.as_str()), ("builder", "Build it."));
    assert_eq!(required_skills, vec!["alpha".to_string()]);
    assert!(recommended_skills.is_empty());
}

#[test]
fn stage_states_cover_lifecycle() {
    let mut names: Vec<&str> = StageState::ALL.iter().map(|s| s.as_str()).collect();
    names.sort_unstable();
    let mut expected = ["PENDING", "READY", "RUNNING", "PASSED", "FAILED", "BLOCKED", "SKIPPED", "NOT_APPLICABLE"];
    expected.sort_unstable();
    assert_eq!(names, expected);
}

#[test]
fn registry_is_optional() {
    let wf =
        parse_workflow_str(&MINIMAL.replace("\"alpha\"", "\"not-in-any-registry\""), None, "<test>").expect("valid");
    assert_eq!(wf.id, "mini");
}

// --- role != provider -------------------------------------------------------

#[test]
fn rejects_provider_key_in_role() {
    let raw = read(&fixture_path("provider-in-role.toml"));
    let err = parse_workflow_bytes(&raw, None, "provider-in-role.toml").expect_err("invalid");
    let msg = err.to_string();
    assert!(msg.contains("roles.builder") && msg.contains("role != provider"), "{msg}");
}

#[test]
fn rejects_model_key_in_role() {
    invalid(
        &MINIMAL.replace("responsibility = \"Build it.\"", "responsibility = \"Build it.\"\nmodel = \"x\""),
        &["role != provider"],
    );
}

// --- structure --------------------------------------------------------------

#[test]
fn rejects_unsupported_schema_version() {
    invalid(&MINIMAL.replace("schema_version = 1", "schema_version = 2"), &["schema_version"]);
}

#[test]
fn rejects_unknown_keys() {
    invalid(&MINIMAL.replace("[workflow]", "colour = 1\n[workflow]"), &["unknown key", "colour"]);
    invalid(&MINIMAL.replace("role = \"builder\"\n", "role = \"builder\"\ntimeout = 3\n"), &["timeout"]);
}

#[test]
fn rejects_invalid_toml() {
    invalid("schema_version = = 1", &["TOML"]);
}

#[test]
fn rejects_bad_ids_and_semver() {
    invalid(&MINIMAL.replace("id = \"mini\"", "id = \"Mini Flow\""), &["workflow.id"]);
    invalid(&MINIMAL.replace("version = \"1.0.0\"\nname", "version = \"1.0\"\nname"), &["semver"]);
}

#[test]
fn rejects_bad_assurance() {
    invalid(&MINIMAL.replace("\"basic\"", "\"extreme\""), &["assurance"]);
}

#[test]
fn rejects_duplicate_stage_ids() {
    let text = MINIMAL.replace("id = \"check\"", "id = \"build\"").replace("depends_on = [\"build\"]\n", "");
    invalid(&text, &["duplicate stage id 'build'"]);
}

#[test]
fn rejects_dangling_role() {
    invalid(&MINIMAL.replace("role = \"checker\"", "role = \"ghost\""), &["unknown role 'ghost'"]);
}

#[test]
fn rejects_dangling_dependency() {
    invalid(&MINIMAL.replace("depends_on = [\"build\"]", "depends_on = [\"nope\"]"), &["unknown stage 'nope'"]);
}

#[test]
fn rejects_cycle_with_path() {
    let raw = read(&fixture_path("cycle.toml"));
    let msg = parse_workflow_bytes(&raw, None, "cycle.toml").expect_err("invalid").to_string();
    assert!(msg.contains("cycle") && msg.contains("a -> b -> c -> a"), "{msg}");
}

#[test]
fn rejects_stage_without_acceptance() {
    let text = MINIMAL.replace(
        "  [[stages.acceptance]]\n  id = \"checked\"\n  description = \"check.md exists.\"\n  evidence = [\"check.md\"]\n",
        "",
    );
    invalid(&text, &["stages.check", "acceptance"]);
}

#[test]
fn rejects_evidence_not_in_outputs() {
    invalid(&MINIMAL.replace("evidence = [\"out.md\"]", "evidence = [\"other.md\"]"), &["other.md", "outputs"]);
}

#[test]
fn rejects_empty_evidence() {
    invalid(&MINIMAL.replace("evidence = [\"out.md\"]", "evidence = []"), &["evidence"]);
}

#[test]
fn rejects_unsafe_output_names() {
    for bad in ["../escape.md", "sub/dir.md", ".hidden", "NOT_APPLICABLE.md", ""] {
        invalid(&MINIMAL.replace("outputs = [\"out.md\"]", &format!("outputs = [\"{bad}\"]")), &["output"]);
    }
}

#[test]
fn rejects_input_from_non_dependency() {
    invalid(
        &MINIMAL.replace("inputs = [\"build/out.md\"]", "inputs = [\"check/check.md\"]"),
        &["input 'check/check.md'"],
    );
    invalid(
        &MINIMAL.replace("inputs = [\"build/out.md\"]", "inputs = [\"build/missing.md\"]"),
        &["input 'build/missing.md'"],
    );
}

#[test]
fn rejects_bad_retry_policy() {
    invalid(&MINIMAL.replace("role = \"builder\"\n", "role = \"builder\"\nmax_attempts = 0\n"), &["max_attempts"]);
    invalid(&MINIMAL.replace("role = \"builder\"\n", "role = \"builder\"\nmax_attempts = 11\n"), &["max_attempts"]);
    invalid(
        &MINIMAL.replace("role = \"builder\"\n", "role = \"builder\"\non_exhausted = \"retry\"\n"),
        &["on_exhausted"],
    );
}

#[test]
fn rejects_bad_independence_reference() {
    invalid(&MINIMAL.replace("independent_of = [\"build\"]", "independent_of = [\"check\"]"), &["independent_of"]);
    invalid(&MINIMAL.replace("independent_of = [\"build\"]", "independent_of = [\"zzz\"]"), &["independent_of"]);
}

#[test]
fn rejects_wrong_types() {
    invalid(&MINIMAL.replace("depends_on = [\"build\"]", "depends_on = \"build\""), &["depends_on"]);
    invalid(&MINIMAL.replace("allow_override = true", "allow_override = \"yes\""), &["allow_override"]);
}

// --- skills -----------------------------------------------------------------

#[test]
fn rejects_unknown_required_skill() {
    invalid(&MINIMAL.replace("required_skills = [\"alpha\"]", "required_skills = [\"zeta\"]"), &["zeta"]);
}

#[test]
fn warns_on_unknown_recommended_skill() {
    let text = MINIMAL
        .replace("required_skills = [\"alpha\"]", "required_skills = [\"alpha\"]\nrecommended_skills = [\"zeta\"]");
    let wf = parse(&text).expect("valid");
    assert_eq!(wf.warnings.len(), 1);
    assert!(wf.warnings[0].contains("zeta"));
}

// --- routing ----------------------------------------------------------------

#[test]
fn rejects_missing_role_routing() {
    let text =
        MINIMAL.replace("[routing.roles.checker]\neligible = [\"codex\", \"claude\"]\npreferred = \"codex\"\n", "");
    invalid(&text, &["routing.roles.checker"]);
}

#[test]
fn rejects_routing_for_undeclared_role() {
    invalid(&format!("{MINIMAL}\n[routing.roles.ghost]\neligible = [\"codex\"]\n"), &["ghost"]);
}

#[test]
fn rejects_unknown_provider() {
    invalid(
        &MINIMAL.replace("eligible = [\"codex\", \"claude\"]", "eligible = [\"codex\", \"gpt-cli\"]"),
        &["gpt-cli"],
    );
}

#[test]
fn rejects_empty_or_duplicate_eligible() {
    invalid(
        &MINIMAL.replace("eligible = [\"codex\", \"claude\"]\npreferred = \"codex\"", "eligible = []"),
        &["eligible"],
    );
    invalid(&MINIMAL.replace("eligible = [\"codex\", \"claude\"]", "eligible = [\"codex\", \"codex\"]"), &["eligible"]);
}

#[test]
fn rejects_preferred_not_eligible() {
    invalid(&MINIMAL.replace("preferred = \"codex\"", "preferred = \"antigravity\""), &["preferred"]);
}

#[test]
fn rejects_bad_fallback_rules() {
    let base = |to: &str, on: &str| {
        format!("{MINIMAL}\n[[routing.fallback]]\nrole = \"checker\"\nfrom = \"codex\"\nto = \"{to}\"\non = [{on}]\n")
    };
    invalid(&base("antigravity", "\"unavailable\""), &["fallback", "eligible"]);
    invalid(&base("claude", "\"crashed\""), &["fallback", "crashed"]);
    invalid(&base("codex", "\"unavailable\""), &["fallback", "same"]);
    invalid(&base("claude", ""), &["fallback", "on"]);
    let ok = parse(&base("claude", "\"incompatible\"")).expect("valid");
    assert_eq!(ok.routing.fallback[0].to_provider, "claude");
}

#[test]
fn collects_multiple_errors() {
    let text = MINIMAL.replace("role = \"checker\"", "role = \"ghost\"").replace("\"basic\"", "\"extreme\"");
    let err = invalid(&text, &["ghost", "assurance"]);
    assert!(err.errors.len() >= 2);
}

// --- review regressions (ported from TestParserReviewRegressions) ----------

#[test]
fn ids_and_outputs_reject_trailing_newline() {
    for (old, new) in [
        ("id = \"build\"", "id = \"build\\n\""),
        ("outputs = [\"out.md\"]", "outputs = [\"out.md\\n\"]"),
        ("id = \"mini\"", "id = \"mini\\n\""),
    ] {
        assert!(parse(&MINIMAL.replace(old, new)).is_err(), "{new}");
    }
}

#[test]
fn optional_stage_cannot_require_approval() {
    let text = MINIMAL.replace(
        "independent_of = [\"build\"]\n",
        "independent_of = [\"build\"]\noptional = true\nrequires_approval = true\n",
    );
    invalid(&text, &["requires_approval"]);
}

#[test]
fn rejects_non_utf8_bytes() {
    let err = parse_workflow_bytes(&[0xff, 0xfe, 0x00], None, "bin").expect_err("invalid");
    assert!(err.to_string().contains("UTF-8"));
}
