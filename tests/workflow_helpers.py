"""Shared fixtures for workflow tests. All paths are repository-local or temporary."""

from pathlib import Path

from awb.registry.parser import parse_registry_string

REPO_ROOT = Path(__file__).resolve().parent.parent
EXAMPLE = REPO_ROOT / "workflows" / "examples" / "plan-implement-review.toml"
FIXTURES = Path(__file__).resolve().parent / "fixtures" / "workflows"

# Two stages: build (role builder) -> check (role checker, independent of build).
MINIMAL = '''
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
'''

# alpha: every agent; beta: Codex and Claude only; gamma: Claude only.
REGISTRY = parse_registry_string('''
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
''')
