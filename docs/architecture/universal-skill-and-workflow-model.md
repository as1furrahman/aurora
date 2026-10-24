# Universal Skill and Workflow Model

**Status:** implemented in the Rust workspace (2026-10-25)
**Related:** [provider-adapter-contract.md](provider-adapter-contract.md),
[workflow-schema.md](workflow-schema.md), and ADR-0001 through ADR-0007 in [`../adr/`](../adr/)

Aurora has two product paths: project-local skill management and evidence-gated workflow
orchestration. Both use provider-neutral metadata and filesystem safeguards.

```text
catalog/catalog.toml ── aurora-core::catalog ──┐
registry/skills.toml ── aurora-core::registry ─┤
SKILL.md ────────────── aurora-skills::skill ──┼─> aurora-app ── CLI / TUI
providers ───────────── aurora-core::providers ┤
workflow TOML ──────── aurora-core::workflow ──┤
runs and evidence ──── aurora-fs::engine ──────┘
```

## Skills

A skill is a directory with a `SKILL.md` entry file. The curated catalog in
`catalog/catalog.toml` records upstream source pins and review metadata. `registry/skills.toml`
describes the skills Aurora can install and their compatibility, dependencies, and overlays.
`aurora-skills` parses skill files, scans content, computes hashes, fetches pinned sources, plans
project-local installs, and applies or undoes those plans.

Aurora installs only into project skill directories: `.claude/skills/`, `.agents/skills/`, and
`.hermes/skills/`. It records managed content in the project’s `aurora.lock`; user-level skill
directories and arbitrary target paths are outside the product scope (ADR-0007).

## Providers and skill adapters

A provider is a coding CLI that can receive a workflow role: `claude`, `codex`, `antigravity`, or
`hermes`. Provider specifications live in `aurora-core::providers` and describe binary detection,
skill adapter identifiers, invocation guidance, documented capabilities, and forbidden flags.
Aurora currently supports manual handoff only; it detects provider binaries but does not launch
them.

Skill adapter identifiers describe which skill directories a provider reads. They do not implement
execution or trigger global skill discovery. Codex and Antigravity share `.agents/skills`; Claude
Code uses `.claude/skills`; Hermes uses `.hermes/skills` and can read trusted shared skills.

## Roles and stages

A role names a responsibility, such as planner, implementer, tester, or reviewer. It declares
required and recommended skills and may require capabilities such as `read_only`. A role does not
select a provider. Workflow routing policy controls provider eligibility, preference, explicit
routes, and fallback.

A stage binds a role to inputs, outputs, dependencies, and acceptance criteria. Stages form a
directed acyclic graph. The strict TOML parser and deterministic planner are in
`aurora-core::workflow` and are documented in [workflow-schema.md](workflow-schema.md).

## Workflows and runs

A workflow definition is a versioned TOML document containing roles, stages, and routing policy.
Planning is pure: the same workflow, registry, options, and provider detection produce the same
plan without filesystem writes.

Starting a workflow creates a run record and handoff bundle for the first eligible stage. The
operator gives that bundle to the assigned CLI, saves the requested outputs, and submits explicit
acceptance claims. `aurora-fs::engine` advances a stage only when its declared outputs exist,
criteria are claimed, and any required approval is recorded. Submitted evidence is hashed; later
changes are reported as drift.

The run store persists `run.json`, the workflow definition, handoffs, and artifacts beneath the
selected run root. The `.awb.lock` filename is retained for run-lock compatibility. Run schemas
and transitions are owned by `aurora-core::workflow::record` and `aurora-fs`.

## Filesystem policy

`aurora-fs` centralizes safe path validation, no-follow reads, staged atomic writes, persistent
cross-process locking, and run persistence. `aurora-skills::apply` uses a write-ahead operation
journal under `.aurora/ops/<id>/`; undo preserves replaced or removed content for recovery.
`aurora-app` composes these engines for both the CLI and TUI.

## Ownership map

| Concern | Rust owner | Persistent data |
|---|---|---|
| Curated skill sources and trust | `aurora-core::catalog` | `catalog/catalog.toml` |
| Skill compatibility registry | `aurora-core::registry` | `registry/skills.toml` |
| Skill parsing, scanning, planning, apply, and undo | `aurora-skills` | `aurora.lock`, `.aurora/ops/` |
| Provider metadata and routing | `aurora-core::providers`, `aurora-core::workflow::planner` | Workflow TOML |
| Workflow definitions and handoffs | `aurora-core::workflow` | User-authored workflow TOML |
| Run records, evidence gates, drift checks | `aurora-fs` | `run.json`, `handoff/`, `artifacts/` |
| CLI/TUI orchestration | `aurora-app`, `aurora-cli`, `aurora-tui` | Project and run roots |
