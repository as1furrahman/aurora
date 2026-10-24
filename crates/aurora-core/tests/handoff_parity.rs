//! Handoff bundles are byte-identical to the Python renderer (ADR-0006). Goldens in
//! `tests/fixtures/handoff/` come from `render_handoff` in `awb/workflow/handoff.py`
//! (see `scripts/gen_run_goldens.py`), rendered for the fixed run path `/aurora-golden/run`.

mod common;

use std::path::Path;

use aurora_core::registry::parse_registry_str;
use aurora_core::workflow::handoff::render_handoff;
use aurora_core::workflow::{PlanOptions, build_plan, parse_workflow_bytes};
use common::{read, repo_root};
use serde_json::Value;

#[test]
fn rust_bundles_match_python_goldens() {
    let root = repo_root();
    let manifest: Value =
        serde_json::from_slice(&read(&root.join("tests/fixtures/handoff/cases.json"))).expect("manifest");
    let cases = manifest.as_array().expect("array");
    assert!(cases.len() >= 20, "expected the full handoff matrix");
    for case in cases {
        let registry_path = match case["registry"].as_str().expect("registry") {
            "repo" => "registry/skills.toml",
            _ => "tests/fixtures/parity/fixture-registry.toml",
        };
        let registry =
            parse_registry_str(&String::from_utf8(read(&root.join(registry_path))).expect("utf-8")).expect("registry");
        let wf = parse_workflow_bytes(&read(&root.join(case["workflow"].as_str().expect("wf"))), Some(&registry), "h")
            .expect("workflow");
        let opts = PlanOptions {
            provider_override: case["provider_override"].as_str(),
            routes: case["routes"]
                .as_array()
                .expect("routes")
                .iter()
                .map(|p| (p[0].as_str().expect("s").to_string(), p[1].as_str().expect("p").to_string()))
                .collect(),
            detected: None,
            ..Default::default()
        };
        let plan = build_plan(&wf, Some(&registry), &opts).expect("plan");
        let stage = plan.stage(case["stage"].as_str().expect("stage")).expect("stage plan");
        let rendered = render_handoff(&wf, stage, Path::new("/aurora-golden/run"), "golden");
        let expected = String::from_utf8(read(&root.join(case["expected"].as_str().expect("path")))).expect("utf-8");
        assert_eq!(rendered, expected, "bundle mismatch for {}", case["expected"]);
    }
}
