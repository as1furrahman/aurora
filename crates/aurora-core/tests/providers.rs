//! Execution-provider specs: manual-handoff only, no bypass flags (ADR-0005). Ported from the
//! provider half of `tests/test_providers.py`.

use aurora_core::providers::{PROVIDER_IDS, PROVIDERS, get_provider, is_known_provider};
use aurora_core::registry::ALLOWED_AGENTS;

#[test]
fn known_providers_and_binaries() {
    assert_eq!(PROVIDER_IDS, ["antigravity", "claude", "codex"]);
    let ids: Vec<&str> = PROVIDERS.iter().map(|p| p.id).collect();
    assert_eq!(ids, PROVIDER_IDS);
    assert_eq!(get_provider("claude").expect("claude").binary, "claude");
    assert_eq!(get_provider("codex").expect("codex").binary, "codex");
    assert_eq!(get_provider("antigravity").expect("antigravity").binary, "agy");
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
        assert_eq!(p.verified_on, "2026-10-23");
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
