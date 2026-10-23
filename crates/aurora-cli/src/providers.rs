//! `aurora providers` and PATH detection. Detection is a lookup only: no binary is ever run.

use std::collections::BTreeMap;

use anstream::println;
use aurora_core::providers::{PROVIDERS, ProviderSpec};
use serde_json::json;

use crate::style::{DIM, OK, paint};

/// Path of `spec.binary` on PATH, if any.
pub fn locate(spec: &ProviderSpec) -> Option<String> {
    which::which(spec.binary).ok().map(|p| p.display().to_string())
}

/// Provider id → installed flag, sorted by id (the planner's `detected` input).
pub fn detect_all() -> BTreeMap<String, bool> {
    PROVIDERS.iter().map(|p| (p.id.to_string(), locate(p).is_some())).collect()
}

pub fn run(as_json: bool) -> u8 {
    let rows: Vec<serde_json::Value> = PROVIDERS
        .iter()
        .map(|p| {
            let path = locate(p);
            json!({
                "id": p.id,
                "name": p.display_name,
                "binary": p.binary,
                "installed": path.is_some(),
                "path": path,
                "skill_adapters": p.skill_adapters,
                "invocation_modes": p.invocation_modes,
                "headless_enabled": p.headless_enabled,
                "documented_headless": p.documented_headless,
                "forbidden_flags": p.forbidden_flags,
                "docs_urls": p.docs_urls,
                "verified_on": p.verified_on,
            })
        })
        .collect();

    if as_json {
        println!("{}", serde_json::to_string_pretty(&rows).unwrap_or_default());
        return 0;
    }

    println!("Aurora — Execution Providers (manual handoff only; no process is launched):");
    println!("{}", "-".repeat(65));
    for (p, row) in PROVIDERS.iter().zip(&rows) {
        let status = if row["installed"] == true { paint(OK, "[INSTALLED]") } else { paint(DIM, "[NOT FOUND]") };
        println!(" {status:<12} {} [{}] (binary: {})", p.display_name, p.id, p.binary);
        println!("   Skill adapters:   {}", p.skill_adapters.join(", "));
        println!("   Invocation modes: {} (headless: disabled)", p.invocation_modes.join(", "));
        println!();
    }
    0
}
