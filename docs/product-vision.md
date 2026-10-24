# Aurora — Product Vision

**Status:** accepted for Milestone 1 (2026-10-23)

## Problem

People who use several coding agents (Claude Code, OpenAI Codex CLI, Google Antigravity) face two
related problems:

1. **Skills are scattered.** Each agent discovers skills from its own directories, with its own
   conventions. Keeping a consistent, trustworthy set of skills across agents is manual and
   error-prone, and it is hard to know what is installed where, from which source, and whether it
   was modified.
2. **"Done" is unverifiable.** A real task needs several responsibilities: planning,
   implementation, testing, independent review, remediation, and final verification. Agents report
   success when a command exits 0 or a model says "done". Nothing tracks whether each
   responsibility was carried out, by which tool, and with what evidence.

## Goal

Aurora is both a **universal skill manager** and a **role-based workflow orchestrator** for coding
CLIs.

- **Layer A: skill management.** A canonical skill library with provenance, version, content hash,
  compatibility and per-agent installation state. It offers uniform inventory, validation,
  plan/dry-run, transactional install/update/remove, history and rollback, all behind strict
  filesystem safety controls.
- **Layer B: workflow orchestration.** A declarative workflow describes the stages of a task, the
  **role** responsible for each stage, the skills that role needs, the **providers** (CLIs)
  eligible to perform it, and the **acceptance criteria and evidence** that must exist before the
  stage counts as passed. Aurora plans the run, routes each role to a provider (or one provider for
  everything), produces the handoff for each stage, and tracks state until the acceptance gates are
  met.

## Definition of systemic task completion

A workflow run is **complete** only when every non-optional stage is `PASSED` and every optional
stage is `PASSED` or `NOT_APPLICABLE`. A stage is `PASSED` only when **all** of the following hold:

1. All of its dependencies are `PASSED` or `NOT_APPLICABLE`.
2. Every declared output artifact exists inside the run's artifact directory as a non-empty regular
   file (not a symlink). Its sha256 is recorded.
3. Every acceptance criterion has been explicitly claimed as met, and each artifact that criterion
   names as evidence is present.
4. If the stage requires human approval, a human approval has been recorded.

A process exit code, or an agent saying it finished, is **never sufficient** by itself.

When implementation and review run on the same provider, Aurora reports **reduced independence**.
It never claims independent verification in that case.

## Primary use cases

1. *"What skills does each of my agents have, and are they the ones I approved?"* Inventory,
   detect, validate (Layer A).
2. *"Run this feature through plan → implement → test → review → fix → verify, with Claude
   planning, Antigravity implementing and Codex reviewing."* Multi-provider routing.
3. *"I only have Codex today. Run the whole workflow through Codex, but keep every role's gates."*
   Single-provider override (`--provider codex`).
4. *"I don't want Aurora launching agents yet. Give me the exact prompt for each stage and I'll paste
   it, then tell Aurora what came back."* Manual handoff mode.
5. *"My run was interrupted. Pick it up without redoing finished stages or overwriting edits."*
   Resume.

## Non-goals (for now)

- **Launching coding CLIs automatically.** Milestone 1 is plan and manual-handoff only. Headless
  execution comes later, after invocation, permissions, timeout and cancellation, environment
  isolation, output capture and approval boundaries are specified and tested (ADR-0005).
- **Live mutation of real agent skill directories.** Gate C stays blocked.
- **Being a skill marketplace or package registry.** No third-party skill packs are bundled.
- **A general workflow engine.** No arbitrary scripting, loops or conditional DAG rewriting. The
  model is a static dependency graph with declared gates.
- **Lossy conversion of skills between agent formats.** Incompatibility is reported, not papered
  over.
- **Unnecessary runtime dependencies.** Cargo dependencies are kept focused and reviewed against
  maintenance, license, and security policy; the distributed product does not require a Python
  runtime.

## Principles

- **Role ≠ provider.** Roles describe responsibilities. Providers are execution tools. Role
  definitions never name a provider.
- **Visible decisions.** Every provider assignment, override and fallback is recorded with its
  reason. Aurora never switches providers silently.
- **Fail closed.** Unknown providers, incompatible skills, missing evidence and unsafe paths
  produce clear errors or `BLOCKED` states, never guesses.
- **Never bypass agent permission controls.** Aurora will not emit or use flags that auto-approve
  agent actions (for example `--dangerously-skip-permissions`, `--yolo`) unless the user explicitly
  authorizes that exact behavior.
- **Smallest architecture that works.** Add a capability only when there is a concrete use for it.
