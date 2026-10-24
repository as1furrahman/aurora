//! Execution-provider specs: manual-handoff only, no bypass flags (ADR-0005).

use aurora_core::providers::{PROVIDER_IDS, PROVIDERS, get_provider, is_known_provider};
use aurora_core::registry::ALLOWED_AGENTS;

#[test]
fn known_providers_and_binaries() {
    assert_eq!(PROVIDER_IDS, ["antigravity", "claude", "codex", "hermes"]);
    let ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id).collect();
    assert_eq!(ids, PROVIDER_IDS);
    assert_eq!(get_provider("claude").expect("claude").binary, "claude");
    assert_eq!(get_provider("codex").expect("codex").binary, "codex");
    assert_eq!(get_provider("antigravity").expect("antigravity").binary, "agy");
    assert_eq!(get_provider("hermes").expect("hermes").binary, "hermes");
    assert!(is_known_provider("codex") && !is_known_provider("gpt-cli"));
}

#[test]
fn unknown_provider_fails_clearly() {
    let msg = get_provider("gpt-cli").expect_err("unknown").to_string();
    assert!(msg.contains("gpt-cli") && msg.contains("claude"), "{msg}");
}

#[test]
fn specs_are_manual_only_and_documented() {
    for p in &PROVIDERS {
        assert_eq!(p.invocation_modes, ["manual-handoff"]);
        assert!(!p.headless_enabled);
        assert!(!p.forbidden_flags.is_empty());
        assert!(!p.docs_urls.is_empty());
        // An ISO date recording when the facts were last checked against the docs or the CLI.
        assert!(p.verified_on.len() == 10 && p.verified_on.starts_with("2026-"), "{}", p.verified_on);
    }
}

#[test]
fn provider_skill_adapters_are_registry_agents() {
    for p in &PROVIDERS {
        assert!(!p.skill_adapters.is_empty());
        for adapter in p.skill_adapters {
            assert!(ALLOWED_AGENTS.contains(adapter), "{} -> {adapter}", p.id);
        }
    }
}

#[test]
fn execute_is_not_enabled() {
    for p in &PROVIDERS {
        let err = p.execute().expect_err("execution disabled");
        assert!(err.to_string().contains(p.id));
    }
}

#[test]
fn handoff_text_never_contains_forbidden_flags() {
    for p in &PROVIDERS {
        let text = p.build_handoff("Do the planning.");
        assert!(text.starts_with("Do the planning.\n\n## How to run this stage with "));
        assert!(text.contains(p.run_hint));
        for other in &PROVIDERS {
            for flag in other.forbidden_flags {
                assert!(!text.contains(flag), "{} handoff contains {flag}", p.id);
            }
        }
    }
}

#[test]
fn hermes_spec_matches_the_verified_cli() {
    // Verified against `hermes --help` / `hermes chat --help` (Hermes Agent v0.22.5) on 2026-10-24.
    let hermes = get_provider("hermes").expect("hermes");
    assert_eq!(hermes.skill_adapters, ["hermes"]);
    assert!(hermes.forbidden_flags.contains(&"--yolo") && hermes.forbidden_flags.contains(&"--accept-hooks"));
    assert_eq!(hermes.enforcement("read_only").expect("read_only").strength, aurora_core::providers::Strength::Soft);
    assert_eq!(hermes.enforcement("tool_restriction").expect("tools").strength, aurora_core::providers::Strength::Hard);
    assert!(ALLOWED_AGENTS.contains(&"hermes"));
}

#[test]
fn codex_forbids_its_newer_bypass_flag() {
    // Present in `codex exec --help` (codex-cli 0.164.0).
    assert!(get_provider("codex").expect("codex").forbidden_flags.contains(&"--dangerously-bypass-hook-trust"));
}
