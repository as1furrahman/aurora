# ADR-0002 — Separate skill-storage adapters from execution providers

**Status:** Accepted (2026-10-23)

## Context
The existing `awb/adapters/` classes describe where an agent *discovers skills*. Workflow
orchestration needs to know how an agent *is invoked*: modes, permissions, output, failure. These
facts change independently. For example, `<repo>/.agents/skills` is read by both Antigravity and
Codex, and a CLI can be detectable long before Aurora can execute it safely.

## Decision
- `awb.adapters.SkillStorageAdapter` (an alias of `BaseAdapter`) covers paths, scopes, inventory and
  conformance only. It has no invocation methods.
- `awb.providers.ExecutionProvider` + `ProviderSpec` cover detection, handoff rendering and, in the
  future, execution. They have no `discovery_path`.
- A `ProviderSpec.skill_adapters` tuple is the only link between the two. Compatibility checks
  go through it (provider → adapter ids → `compatible_agents`).
- Provider ids (`claude`, `codex`, `antigravity`) are deliberately distinct from adapter ids
  (`claude-code`, `codex-cli`, `antigravity-cli`, …).

## Consequences
- Tests assert the separation structurally (`tests/test_providers.py`).
- Adding an agent means adding up to two small classes, with no changes to shared safety or
  workflow code.
- The existing adapter class name `BaseAdapter` is kept for compatibility.
