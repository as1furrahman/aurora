//! Skill-registry resolution, shared by the CLI and the TUI:
//! `--registry` > `$AURORA_REGISTRY_PATH` > `$AWB_REGISTRY_PATH` > the registry built into the binary.
//!
//! The built-in fallback means an installed binary always has a registry.

use std::path::{Path, PathBuf};

use aurora_core::registry::{Registry, parse_registry_str};

const BUILTIN_REGISTRY: &str = include_str!("../../../registry/skills.toml");

pub fn load(explicit: Option<&Path>) -> Result<Registry, String> {
    match resolve(explicit) {
        Some(path) => {
            if !path.is_file() {
                return Err(format!("Registry file does not exist: '{}'", path.display()));
            }
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("Cannot read registry file '{}': {e}", path.display()))?;
            parse_registry_str(&text).map_err(|e| e.to_string())
        }
        None => parse_registry_str(BUILTIN_REGISTRY).map_err(|e| format!("built-in registry is invalid: {e}")),
    }
}

fn resolve(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        return Some(path.to_path_buf());
    }
    ["AURORA_REGISTRY_PATH", "AWB_REGISTRY_PATH"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}
