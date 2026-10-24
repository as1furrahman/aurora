//! The planner's reviewed static golden cases cover the fixture × option matrix. Expected JSON is
//! checked in and reviewed as test data rather than generated from the implementation under test.

mod common;

use aurora_core::registry::parse_registry_str;
use aurora_core::workflow::{Detection, PlanOptions, build_plan, parse_workflow_bytes};
use common::{read, repo_root};
use serde_json::Value;

#[test]
fn rust_plans_match_static_goldens() {
    let root = repo_root();
    let manifest: Value =
        serde_json::from_slice(&read(&root.join("tests/fixtures/parity/cases.json"))).expect("manifest");
    let cases = manifest.as_array().expect("array");
    assert!(cases.len() >= 18, "expected the full parity matrix, got {}", cases.len());

    for case in cases {
        let expected_path = case["expected"].as_str().expect("expected path");
        let registry = case["registry"].as_str().map(|p| {
            parse_registry_str(&String::from_utf8(read(&root.join(p))).expect("utf-8")).expect("valid registry")
        });
        let wf_path = root.join(case["workflow"].as_str().expect("workflow path"));
        let wf = parse_workflow_bytes(&read(&wf_path), registry.as_ref(), "parity").expect("valid workflow");
        let detected: Option<Detection> = case["detected"]
            .as_object()
            .map(|m| m.iter().map(|(k, v)| (k.clone(), v.as_bool().expect("bool"))).collect());
        let opts = PlanOptions {
            provider_override: case["provider_override"].as_str(),
            routes: case["routes"]
                .as_array()
                .expect("routes")
                .iter()
                .map(|pair| {
                    (pair[0].as_str().expect("stage").to_string(), pair[1].as_str().expect("provider").to_string())
                })
                .collect(),
            detected: detected.as_ref(),
            ..Default::default()
        };
        let plan = build_plan(&wf, registry.as_ref(), &opts).expect("plan builds");
        let actual = serde_json::to_value(&plan).expect("serialize");
        let expected: Value = serde_json::from_slice(&read(&root.join(expected_path))).expect("golden json");
        assert_eq!(actual, expected, "plan mismatch for {expected_path}");
    }
}
