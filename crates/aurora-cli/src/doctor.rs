//! Read-only host diagnostics for `aurora doctor`.

use anstream::println;
use aurora_core::providers::PROVIDERS;
use serde_json::json;
use std::fs::OpenOptions;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::registry;

const WRITE_PROBE_PREFIX: &str = ".aurora_doctor_probe_";
static WRITE_PROBE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Check registry validity and report which supported execution CLIs are on PATH.
/// Missing provider CLIs are informational: Aurora can still plan manual handoffs.
pub fn run(as_json: bool) -> u8 {
    let registry = registry::load(None);
    let staging = scan_staging(Path::new("."));
    let write_access = probe_write_access(Path::new("."));
    let healthy = registry.is_ok() && staging.as_ref().is_ok_and(Vec::is_empty) && write_access.is_ok();
    let provider_rows: Vec<_> = PROVIDERS
        .iter()
        .map(|spec| {
            let path = aurora_app::tools::locate(spec);
            json!({
                "id": spec.id,
                "binary": spec.binary,
                "installed": path.is_some(),
                "path": path,
            })
        })
        .collect();
    let result = json!({
        "healthy": healthy,
        "registry": {
            "ok": registry.is_ok(),
            "error": registry.err(),
        },
        "staging": match &staging {
            Ok(paths) => json!({ "clean": paths.is_empty(), "paths": paths }),
            Err(error) => json!({ "clean": false, "error": error }),
        },
        "write_access": match &write_access {
            Ok(()) => json!({ "ok": true }),
            Err(error) => json!({ "ok": false, "error": error }),
        },
        "providers": provider_rows,
    });

    if as_json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap_or_default());
    } else {
        if result["registry"]["ok"] == true {
            println!("[OK] Skill registry is valid");
        } else {
            println!("[ERROR] Skill registry: {}", result["registry"]["error"]);
        }
        match &staging {
            Ok(paths) if paths.is_empty() => println!("[OK] No leftover staging files in the current directory"),
            Ok(paths) => {
                println!("[WARN] Leftover staging files in the current directory:");
                for path in paths {
                    println!("  {path}");
                }
            }
            Err(error) => println!("[ERROR] Cannot inspect staging files: {error}"),
        }
        match &write_access {
            Ok(()) => println!("[OK] Current directory is writable (temporary probe removed)"),
            Err(error) => println!("[ERROR] Current directory write check failed: {error}"),
        }
        println!("Execution providers (missing CLIs are optional for manual handoffs):");
        for provider in result["providers"].as_array().expect("provider list") {
            let state = if provider["installed"] == true { "found" } else { "not found" };
            println!("  {}: {state}", provider["id"].as_str().unwrap_or("unknown"));
        }
    }

    if result["healthy"] == true { 0 } else { 1 }
}

fn scan_staging(root: &Path) -> Result<Vec<String>, String> {
    let entries = std::fs::read_dir(root).map_err(|error| error.to_string())?;
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(aurora_fs::atomic::STAGE_PREFIX)
            || name.starts_with(aurora_fs::dirs::STAGE_PREFIX)
            || name.starts_with(WRITE_PROBE_PREFIX)
        {
            paths.push(name.into_owned());
        }
    }
    paths.sort();
    Ok(paths)
}

fn probe_write_access(root: &Path) -> Result<(), String> {
    let name =
        format!("{WRITE_PROBE_PREFIX}{}_{}", std::process::id(), WRITE_PROBE_COUNTER.fetch_add(1, Ordering::Relaxed));
    let path = root.join(name);
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("cannot create a temporary probe: {error}"))?;
    drop(file);
    std::fs::remove_file(&path).map_err(|error| format!("cannot remove temporary probe '{}': {error}", path.display()))
}
