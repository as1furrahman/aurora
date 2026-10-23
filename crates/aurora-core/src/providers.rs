//! Execution providers (coding CLIs). Step 1 needs only the known ids; specs follow in Step 2.

/// Known provider ids, sorted.
pub const PROVIDER_IDS: [&str; 3] = ["antigravity", "claude", "codex"];

pub fn is_known_provider(id: &str) -> bool {
    PROVIDER_IDS.contains(&id)
}
