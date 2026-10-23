# ADR-0001 — Workflow definitions in TOML, run records in JSON

**Status:** Accepted (2026-10-23)

## Context
Workflow definitions are written by people and must be parsed without third-party dependencies.
Run records are written by Aurora, updated often, and must round-trip exactly.

## Decision
- Workflow definitions are **TOML**, parsed with `tomllib` (Python 3.11+ standard library). This
  matches `registry/skills.toml`.
- Run records are **JSON**. The standard library cannot write TOML, and JSON round-trips exactly.
- Both formats carry an integer `schema_version`. Milestone 1 accepts `1` only.

## Consequences
- No new dependencies.
- A run cannot rewrite its definition in place. The run pins a byte copy (`workflow.toml`) and its
  sha256.
- YAML was rejected: there is no standard-library parser, and its type coercions are ambiguous.
