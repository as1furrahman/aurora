//! Skill-registry parsing, validation, and dependency ordering.

mod common;

use aurora_core::registry::parse_registry_str;

fn err(text: &str) -> String {
    parse_registry_str(text).expect_err("registry should be invalid").to_string()
}

const HEADER: &str = "[registry]\nversion = \"1.0.0\"\nupdated_at = \"2026-10-23T00:00:00Z\"\n";

fn skill(id: &str, extra: &str) -> String {
    format!("\n[[skills]]\nid = \"{id}\"\nname = \"N\"\nversion = \"1.0.0\"\ndescription = \"d\"\n{extra}")
}

#[test]
fn repo_registry_parses() {
    let reg = common::repo_registry();
    assert_eq!(reg.skill_count(), 4);
    let principles = reg.get("principles").expect("principles present");
    assert!(principles.compatible_agents.iter().any(|a| a == "claude-code"));
}

#[test]
fn fixture_registry_parses() {
    let reg = common::registry();
    assert_eq!(
        reg.get("beta").map(|s| s.compatible_agents.clone()),
        Some(vec!["codex-cli".into(), "claude-code".into()])
    );
    assert!(reg.get("missing").is_none());
}

#[test]
fn rejects_empty_and_malformed() {
    assert!(err("   ").contains("empty"));
    assert!(err("[registry\n").contains("TOML"));
    assert!(err("x = 1\n").contains("[registry]"));
    assert!(err("[registry]\nupdated_at = \"t\"\n").contains("'version'"));
    assert!(err("[registry]\nversion = \"1.0\"\nupdated_at = \"t\"\n").contains("SemVer"));
    assert!(err("[registry]\nversion = \"1.0.0\"\n").contains("updated_at"));
    assert!(err(HEADER).contains("[[skills]]"));
}

#[test]
fn rejects_bad_skills() {
    assert!(err(&format!("{HEADER}{}", skill("Bad_Id", ""))).contains("lowercase"));
    assert!(err(&format!("{HEADER}{}{}", skill("a", ""), skill("a", ""))).contains("Duplicate skill ID"));
    assert!(
        err(&format!("{HEADER}{}", skill("a", "compatible_agents = [\"vim\"]"))).contains("unrecognized agent 'vim'")
    );
    assert!(err(&format!("{HEADER}{}", skill("a", "dependencies = [\"zz\"]"))).contains("does not exist"));
    assert!(err(&format!("{HEADER}{}", skill("a", "dependencies = [\"a\"]"))).contains("cannot depend on itself"));
    let cyc = format!("{HEADER}{}{}", skill("a", "dependencies = [\"b\"]"), skill("b", "dependencies = [\"a\"]"));
    assert!(err(&cyc).contains("a -> b -> a"));
}

#[test]
fn ids_reject_trailing_newline() {
    assert!(err(&format!("{HEADER}{}", skill("a\\n", ""))).contains("lowercase"));
}
