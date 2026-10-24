# ADR-0003 — Run-record persistence and schema versioning

**Status:** Accepted; implemented in Rust (2026-10-25)

## Context
Workflow runs span hours or days and several people or tools. A run must survive interruption,
must never treat incomplete work as complete, and must detect tampering with its definition or its
accepted evidence.

## Decision
- One directory per run under an explicit, pre-existing `--run-root`. There is no implicit default
  location.
- `run.json` is the single source of truth. Every update is a read-modify-write under
  `aurora_fs::lock::ProcessLock(<run-dir>/.awb.lock)`, persisted with
  `aurora_fs::atomic::atomic_write`. The initial record is saved before any handoff bundle. After
  that, bundles are written before the record that references them.
- Every write path is validated with `validate_security_preconditions(path, [run_dir])`.
- `schema_version: 1` and `kind: "aurora.workflow-run"` are mandatory. The loader rejects unknown
  versions. Any future version change requires an explicit migration function plus tests. v1 files
  are never reinterpreted.
- Evidence hashes recorded at acceptance are re-verified on `status` and `resume`. Drift is
  reported and does not silently change state.
- The run lock filename `.awb.lock` is retained deliberately for compatibility with existing run
  directories. The retired prototype's `.awb.manifest.json` and `.awb.journal.json` files were not
  carried forward: ADR-0007 defines the Rust skill manager's project lock (`aurora.lock`) and
  operation journals under `.aurora/ops/<id>/`.

## Consequences
- Runs can be resumed by any later process, and bundle writes are idempotent.
- The run directory is plain files, so it can be inspected and reviewed without Aurora.
