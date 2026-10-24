# ADR-0006: Rewrite Aurora in Rust, workflow engine first

**Status:** Accepted; implementation completed (2026-10-25)

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

1. **Rust replaces Python in this repository.** The Python prototype was retained as a reference
   oracle during the port and has now been retired. Rust is the sole implementation and source of
   truth.
2. **Milestone R1 ported the workflow engine in small commits:**
   - model and parser;
   - providers and planner;
   - read-only CLI;
   - run store;
   - engine and run CLI;
   - research-driven additions.

   The TUI and skill core were delivered in later milestones.
3. **Workspace:**
   - `aurora-core` is pure domain logic. It has no terminal dependencies and does no run-dir I/O.
   - `aurora-fs` holds the only filesystem-mutating code: boundaries, no-follow reads, atomic
     writes, locks, and the run store.
   - `aurora-cli` is the binary `aurora`.
   - Later crates delivered: `aurora-app`, `aurora-tui`, and `aurora-skills`.
4. **Migration compatibility contract with the prototype:**
   - Same workflow TOML schema v1, and the same fixtures (`workflows/examples/`,
     `tests/fixtures/workflows/`).
   - Same plan JSON field names, and the same CLI flags and exit codes (0, 1, 3).
   - `run.json` retained the same keys and semantics, allowing Rust to resume prototype-created
     runs. Byte identity was not required because the serializers handle non-ASCII differently.
   - Handoff bundles were byte-identical during migration.
5. **Durable names are kept:**
   - `.awb.lock` remains the run-lock filename for compatibility with existing run directories.
   - The skill manager uses `aurora.lock` and write-ahead journals under `.aurora/ops/<id>/`.
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

## Retirement record

The prototype retirement is complete:

- Rust tests cover workflow, filesystem, skill scanning, installation, and TUI behavior. Prototype-
  only diagnostics, global skill discovery, and arbitrary-target installation were excluded above.
- CLI, handoff, and run-store parity checks passed during the migration. Their differential scripts
  and Python reference tests have since been retired; reviewed static fixtures remain for Rust
  compatibility tests.
- Rust workflow and skill-core implementations are delivered in `aurora-core`, `aurora-fs`, and
  `aurora-skills`.
- Python source, tests, packaging metadata, and runtime invocations have been removed. Historical
  reports and this ADR retain references documenting the migration.

### Prototype-only features outside the Rust compatibility contract

The Python CLI's `doctor` behavior and global skill-directory inventory were not Rust migration
requirements. Aurora later added a narrower Rust-native `aurora doctor` for the configured
registry, current-directory write access and staging debris, plus provider binaries on `PATH`. It
does not inventory global skill directories; global user-level skill management remains out of
scope under [ADR-0007](0007-skill-catalog-and-trust.md). Likewise, prototype installation to
arbitrary target directories is superseded by the project-local, lock-owned install model in
ADR-0007.

## Consequences

- Rust is the sole maintained implementation. Static fixtures and Rust integration tests protect
  compatibility with existing workflow and run records.
