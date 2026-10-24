# Universal Skill and Workflow Model

**Status:** accepted for Milestone 1 (2026-10-23)
**Related:** [provider-adapter-contract.md](provider-adapter-contract.md),
[workflow-schema.md](workflow-schema.md), ADR-0001…0005 in [`../adr/`](../adr/)

Aurora keeps seven concepts separate. Each has one owner module, and none of them leaks into
another.

```
            ┌────────────── Layer A: skill management ──────────────┐
  Skill ───▶│ registry/skills.toml  →  awb.registry                  │
            │ SkillStorageAdapter   →  awb.adapters (per agent dir)  │
            └───────────────────────────────────────────────────────┘
                     ▲ compatibility (compatible_agents)
                     │
            ┌────────────── Layer B: workflow orchestration ────────┐
  Role ────▶│ WorkflowDefinition (roles, stages, gates) → awb.workflow│
  Stage     │ Plan (deterministic routing)              → planner    │
  Provider ▶│ ProviderSpec / ExecutionProvider          → awb.providers│
  Run       │ RunRecord + artifacts + evidence          → runs/engine│
            └───────────────────────────────────────────────────────┘
  Policy ── safety boundaries (awb.safety) apply to every write in both layers
```

## 1. Skill

A reusable instruction package: a directory whose entry file is `SKILL.md`. All three target agents
use the Agent Skills `SKILL.md` convention. They differ in frontmatter requirements and discovery
paths (see the provider contract).

The canonical metadata lives in `registry/skills.toml` (`awb.registry.schema.SkillDefinition`):
`id`, `name`, `version`, `description`, `dependencies`, `compatible_agents`, `triggers`, `files`,
`category`.

Planned metadata, not yet in the schema. The landscape doc explains the reasoning.

| Field | Purpose |
|---|---|
| `source` | provenance: `local`, `git:<url>`, … |
| `revision` | pinned commit or tag when the source is a VCS |
| `content_sha256` | hash over the skill's files, for drift detection (the install manifest already records per-file hashes) |
| `license` | SPDX id when known |
| `roles` / `tags` | roles or task types the skill supports |
| `validation` | last validation and security-scan status |

`compatible_agents` holds **skill-storage adapter ids** (`codex-cli`, `claude-code`,
`antigravity-cli`, …), not provider ids. A provider maps to one or more adapter ids (see §3).

## 2. Skill-storage adapter (Layer A)

Defined in `awb/adapters/base.py` (`BaseAdapter`, exported as `SkillStorageAdapter`). It answers
three questions: *where does agent X discover skills, what is installed there, and is a given
directory structurally safe to manage?* It has **no** knowledge of how to run the agent.

## 3. Provider (execution tool)

A coding CLI that can perform a role: `claude` (Claude Code), `codex` (OpenAI Codex CLI),
`antigravity` (Google Antigravity CLI, binary `agy`). Defined in `awb/providers/` as an immutable
`ProviderSpec` plus an `ExecutionProvider` contract. A provider spec lists the skill-storage adapter
ids whose skills that CLI can load:

| Provider | Skill-storage adapters |
|---|---|
| `claude` | `claude-code` |
| `codex` | `codex-cli` |
| `antigravity` | `antigravity-cli`, `antigravity-workspace` |

In Milestone 1 every provider supports only the `manual-handoff` invocation mode. `execute()` raises
`ProviderNotEnabledError` (ADR-0005).

## 4. Role

A **responsibility**, such as planner, implementer, tester, reviewer, documenter. A role declares
what it is responsible for and which skills it requires or recommends. **A role never names a
provider**, and the workflow parser rejects provider keys inside a role. Which providers may play a
role lives in the workflow's separate `[routing]` section.

## 5. Stage

One step of a workflow. A stage binds a role to inputs, outputs, dependencies and acceptance
criteria, plus retry, approval and independence rules. Stages form a directed acyclic graph through
`depends_on`.

## 6. Workflow definition and plan

A **workflow definition** (TOML, ADR-0001) is a versioned, declarative document: roles + stages +
routing. A **plan** is the deterministic result of applying an execution policy to a definition:

- **multi-provider routing**: each role's preferred or eligible provider, plus explicit
  `--route stage=provider` overrides;
- **single-provider override**: `--provider <id>` assigns that provider to every stage whose role
  allows it;
- **manual handoff**: Milestone 1 always produces handoff bundles, whatever the routing.

Planning is pure. Given the same definition, registry, options and set of detected providers, it
produces the same plan, and it writes nothing.

## 7. Run, artifacts, evidence

A **run** is one execution of a plan under an explicit run root:
`<run-root>/<run-id>/run.json` plus handoff bundles and an artifact tree. The run record
(ADR-0003) is the single source of truth for stage state. **Evidence** is a file inside the run's
artifact directory whose sha256 is recorded when it is submitted. An acceptance criterion is
satisfied only by an explicit claim *and* the presence of the evidence artifacts it names.

## 8. Policy

Cross-cutting rules enforced in code rather than by convention:

- `awb.safety.boundaries.validate_security_preconditions` before any write: allowed roots,
  protected projects, global agent directories, sensitive system paths, ownership and permission
  checks.
- `awb.safety.locking.ProcessLock` around every mutation, including run-record updates.
- `awb.safety.filesystem.SafeFS.safe_atomic_write` for every persisted file.
- Gate C: no writes to real agent skill directories without separate authorization.
- No provider fallback unless declared, and every fallback is recorded.
- Aurora never generates commands containing permission-bypass flags.

## Ownership map

| Concept | Module | Persisted as |
|---|---|---|
| Skill metadata | `awb.registry` | `registry/skills.toml` |
| Installed-skill ownership | `awb.operations.manifest` | `.awb.manifest.json` per skill |
| Skill-storage adapter | `awb.adapters` | — |
| Provider | `awb.providers` | — (static specs) |
| Workflow definition | `awb.workflow.parser` / `model` | `*.toml` (user-authored) |
| Plan | `awb.workflow.planner` | embedded in `run.json` |
| Run, state, evidence | `awb.workflow.runs` / `engine` | `run.json`, `handoff/`, `artifacts/` |
