//! Shared fixtures for aurora-core integration tests. Mirrors `tests/workflow_helpers.py`.
#![allow(dead_code)]

use std::path::PathBuf;

use aurora_core::registry::{Registry, parse_registry_str};

/// Repository root (two levels above this crate).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn example_path() -> PathBuf {
    repo_root().join("workflows/examples/plan-implement-review.toml")
}

pub fn fixture_path(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures/workflows").join(name)
}

pub fn read(path: &PathBuf) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

pub fn repo_registry() -> Registry {
    let text = String::from_utf8(read(&repo_root().join("registry/skills.toml"))).expect("utf-8 registry");
    parse_registry_str(&text).expect("repo registry is valid")
}

/// Two stages: build (role builder) -> check (role checker, independent of build).
pub const MINIMAL: &str = r#"
schema_version = 1

[workflow]
id = "mini"
version = "1.0.0"
name = "Minimal"
assurance = "basic"

[roles.builder]
responsibility = "Build it."
required_skills = ["alpha"]

[roles.checker]
responsibility = "Check it."
required_skills = ["beta"]

[[stages]]
id = "build"
role = "builder"
outputs = ["out.md"]

  [[stages.acceptance]]
  id = "built"
  description = "out.md exists."
  evidence = ["out.md"]

[[stages]]
id = "check"
role = "checker"
depends_on = ["build"]
inputs = ["build/out.md"]
outputs = ["check.md"]
independent_of = ["build"]

  [[stages.acceptance]]
  id = "checked"
  description = "check.md exists."
  evidence = ["check.md"]

[routing]
allow_override = true

[routing.roles.builder]
eligible = ["antigravity", "codex", "claude"]
preferred = "antigravity"

[routing.roles.checker]
eligible = ["codex", "claude"]
preferred = "codex"
"#;

/// alpha: every agent; beta: Codex and Claude only; gamma: Claude only.
pub const REGISTRY_TOML: &str = r#"
[registry]
version = "1.0.0"
updated_at = "2026-10-23T00:00:00Z"

[[skills]]
id = "alpha"
name = "Alpha"
version = "1.0.0"
description = "a"
compatible_agents = ["universal"]
files = ["skills/alpha/SKILL.md"]

[[skills]]
id = "beta"
name = "Beta"
version = "1.0.0"
description = "b"
compatible_agents = ["codex-cli", "claude-code"]
files = ["skills/beta/SKILL.md"]

[[skills]]
id = "gamma"
name = "Gamma"
version = "1.0.0"
description = "g"
compatible_agents = ["claude-code"]
files = ["skills/gamma/SKILL.md"]
"#;

pub fn registry() -> Registry {
    parse_registry_str(REGISTRY_TOML).expect("fixture registry is valid")
}
