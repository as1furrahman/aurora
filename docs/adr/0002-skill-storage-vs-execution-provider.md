# ADR-0002 — Separate skill-storage adapters from execution providers

**Status:** Accepted; implemented in Rust (2026-10-25)

## Context
Skill installation needs to know where an agent *discovers skills*. Workflow orchestration needs to
know how an agent *is invoked*: modes, permissions, output, and failure. These facts change
independently. For example, `<repo>/.agents/skills` is read by both Antigravity and Codex, and a CLI
can be detectable long before Aurora can execute it safely.

## Decision
- Skill-storage adapters are stable identifiers and project-local directory mappings in
  `aurora-skills::plan`; they do not run coding CLIs or inventory global skill directories.
- `aurora-core::providers::ProviderSpec` describes provider detection, invocation guidance,
  capability enforcement, and forbidden flags. `aurora-app::tools` checks `PATH` without launching
  a provider; workflow handoffs are rendered separately.
- `ProviderSpec.skill_adapters` links providers to adapter identifiers. Compatibility checks use
  those identifiers against each skill's `compatible_agents` metadata.
- Provider ids (`claude`, `codex`, `antigravity`, `hermes`) remain distinct from adapter ids
  (`claude-code`, `codex-cli`, `antigravity-cli`, `antigravity-workspace`, `hermes`).

## Consequences
- Rust tests cover provider metadata and skill compatibility independently.
- Adding a provider updates its static spec and the project-skill directory mapping without
  duplicating filesystem safety or workflow logic.
