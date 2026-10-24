use std::fs;

use assert_cmd::Command;
use serde_json::Value;

#[test]
fn doctor_accepts_valid_registry_when_provider_clis_are_absent() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("bin");

    let output = Command::cargo_bin("aurora")
        .expect("binary")
        .env("PATH", &bin)
        .current_dir(temp.path())
        .env_remove("AURORA_REGISTRY_PATH")
        .env_remove("AWB_REGISTRY_PATH")
        .env("NO_COLOR", "1")
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor");

    assert_eq!(output.status.code(), Some(0));
    let result: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(result["healthy"], true);
    assert_eq!(result["registry"]["ok"], true);
    assert_eq!(result["write_access"]["ok"], true);
    assert!(result["providers"].as_array().expect("providers").iter().all(|provider| provider["installed"] == false));
    assert!(
        fs::read_dir(temp.path()).expect("project entries").all(|entry| !entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".aurora_doctor_probe_"))
    );
}

#[test]
fn doctor_reports_registry_failure_without_treating_missing_providers_as_errors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("bin");
    let invalid_registry = temp.path().join("skills.toml");
    fs::write(&invalid_registry, "not valid registry data").expect("registry");

    let output = Command::cargo_bin("aurora")
        .expect("binary")
        .env("PATH", &bin)
        .current_dir(temp.path())
        .env("AURORA_REGISTRY_PATH", &invalid_registry)
        .env_remove("AWB_REGISTRY_PATH")
        .env("NO_COLOR", "1")
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor");

    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(result["healthy"], false);
    assert_eq!(result["registry"]["ok"], false);
    assert_eq!(result["providers"].as_array().expect("providers").len(), 4);
    assert!(result["providers"].as_array().expect("providers").iter().all(|provider| provider["installed"] == false));
}

#[test]
fn doctor_reports_leftover_staging_files_without_removing_them() {
    let temp = tempfile::tempdir().expect("tempdir");
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).expect("bin");
    fs::write(temp.path().join(".awb_stage_abandoned"), "kept").expect("stage file");
    fs::create_dir(temp.path().join(".aurora_stage_abandoned")).expect("stage directory");

    let output = Command::cargo_bin("aurora")
        .expect("binary")
        .env("PATH", &bin)
        .current_dir(temp.path())
        .env_remove("AURORA_REGISTRY_PATH")
        .env_remove("AWB_REGISTRY_PATH")
        .env("NO_COLOR", "1")
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor");

    assert_eq!(output.status.code(), Some(1));
    let result: Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(result["staging"]["clean"], false);
    assert_eq!(result["staging"]["paths"], serde_json::json!([".aurora_stage_abandoned", ".awb_stage_abandoned"]));
    assert!(temp.path().join(".awb_stage_abandoned").is_file(), "doctor must not remove staging files");
}
