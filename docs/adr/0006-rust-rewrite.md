# ADR-0006: Rewrite Aurora in Rust, workflow engine first

**Status:** Accepted (2026-10-23)

## Context

Aurora is to become a single-binary, role-based agent skill manager and workflow orchestrator.
It needs an intuitive TUI modelled on luongnv89/asm, a scriptable CLI, and strong filesystem
safety.

The Python prototype (`awb/`) proved the design: 281 tests, evidence-gated stages, routing versus
single-provider override, and safe run records. The owner chose Rust as the long-term
implementation. The reasons:

- one static binary, so there is nothing to install for a Python runtime;
- the ratatui ecosystem for the TUI;
- typed safety for path and permission logic;
- most reusable prior art is Rust: openai/codex `codex-rs`, goose, vibe-kanban executors, and
  the `agent-client-protocol` crate. See `docs/references/prior-art.md`.

## Decision

1. **Rust replaces Python in this repository.** The Python code stays, unchanged, as the
   **reference oracle** until the Rust port reaches parity. After that it is retired.
2. **Milestone R1 ports the workflow engine only, in small commits:**
   - model and parser;
   - providers and planner;
   - read-only CLI;
   - run store;
   - engine and run CLI;
   - research-driven additions.

   The TUI and skill core are recorded in `docs/design/` and come later.
3. **Workspace:**
   - `aurora-core` is pure domain logic. It has no terminal dependencies and does no run-dir I/O.
   - `aurora-fs` holds the only filesystem-mutating code: boundaries, no-follow reads, atomic
     writes, locks, and the run store.
   - `aurora-cli` is the binary `aurora`.
   - Later crates: `aurora-tui`, `aurora-skills`, `aurora-proc`, `aurora-providers`,
     `aurora-runner`, and `aurora-acp` (behind a feature flag).
4. **Compatibility contract with the Python implementation:**
   - Same workflow TOML schema v1, and the same fixtures (`workflows/examples/`,
     `tests/fixtures/workflows/`).
   - Same plan JSON field names, and the same CLI flags and exit codes (0, 1, 3).
   - `run.json` is **schema-compatible** (same keys and semantics), so either implementation can
     resume the other's run. Byte identity is not promised, because Python escapes non-ASCII and
     serde does not.
   - Handoff bundles are byte-identical.
5. **Durable names are kept:**
   - `.awb.lock` for run locks.
   - `.awb.manifest.json` and `.awb.journal.json` when the skill core is ported.
6. **Toolchain:**
   - edition 2024;
   - MSRV 1.89, the floor for std `File::lock`;
   - no async runtime in R1;
   - `unsafe_code = "deny"` workspace-wide, with documented exceptions only;
   - minimal libc use, isolated in `aurora-fs`.
7. **Licence hygiene:**
   - Dependencies must be under licences in the `deny.toml` allow-list.
   - Copied code keeps its licence, its NOTICE, and a "Modified from <repo>@<sha> (<licence>)"
     header, and is listed in `THIRD_PARTY_NOTICES.md`.
   - Projects under AGPL, source-available or unclear licences are never copied.

## Retirement criteria for the Python code

Each item needs evidence in a report:

- every Python workflow test has a Rust equivalent, or a documented reason why it does not apply;
- the parity script shows no JSON differences across the fixture × mode matrix;
- a cross-implementation run (Python start → Rust submit → Python finish) passes;
- the skill-core port, a later milestone, covers the `awb.operations`, `awb.transactions` and
  `awb.safety` test suites.

## Consequences

- For a while there are two implementations. The parity script keeps them honest, and any
  divergence is a bug in one of them.
- The Python CLI remains the reference behaviour until retirement.
