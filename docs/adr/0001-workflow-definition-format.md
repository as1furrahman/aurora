# ADR-0001 — Workflow definitions in TOML, run records in JSON

**Status:** Accepted; implemented in Rust (2026-10-25)

## Context
Workflow definitions are written by people and must have a clear, versioned schema. Run records are
written by Aurora and updated as stages advance, so they need a stable serialization format.

## Decision
- Workflow definitions are **TOML**, parsed and validated by `aurora-core::workflow` using the Rust
  `toml` crate. This matches `registry/skills.toml` and `catalog/catalog.toml`.
- Run records are **JSON**, serialized by `serde_json` with schema version 1.
- Both formats carry an integer schema version. The current workflow parser accepts version `1`.

## Consequences
- The Rust workspace uses the `toml` and `serde_json` crates for parsing and serialization.
- A run cannot rewrite its definition in place. The run pins a byte copy (`workflow.toml`) and its
  sha256.
- YAML was not selected; TOML is used consistently for human-authored definitions and registry
  data.
