//! Which AI coding tools are installed. Detection is a PATH lookup only: no binary is ever run.

use std::collections::BTreeMap;

use aurora_core::providers::{PROVIDERS, ProviderSpec};

/// Path of `spec.binary` on PATH, if any.
pub fn locate(spec: &ProviderSpec) -> Option<String> {
    which::which(spec.binary).ok().map(|p| p.display().to_string())
}

/// Provider id → installed flag, sorted by id (the planner's `detected` input).
pub fn detect_all() -> BTreeMap<String, bool> {
    PROVIDERS.iter().map(|p| (p.id.to_string(), locate(p).is_some())).collect()
}

/// Ids of the installed providers, sorted.
pub fn installed() -> Vec<String> {
    detect_all().into_iter().filter(|(_, on)| *on).map(|(id, _)| id).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_is_reported_and_installed_is_a_subset() {
        let all = detect_all();
        assert_eq!(all.len(), PROVIDERS.len());
        assert!(installed().iter().all(|id| all[id]));
    }
}
