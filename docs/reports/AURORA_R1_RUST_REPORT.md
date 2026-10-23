# Aurora Milestone R1: Rust workflow engine

**Date:** 2026-10-24
**Branch:** `feat/rust-workflow-engine`, local only (also pushed to the host checkout's
branch). **Not pushed to `origin`.**
**Environment:** all builds, tests and tool installs ran inside the `dev` distrobox, with the
working copy at `/opt/aurora` and `CARGO_HOME`, `TMPDIR` and `target/` kept off the host home
directory.
**Plan:** R1 "workflow engine first, in small steps". The TUI and skill core are recorded for
later (`docs/design/`).

## 1. Commits

| SHA | Step | Content |
|---|---|---|
| `5b44b29` | 0 | Workspace scaffold (`aurora-core`, `aurora-fs`, `aurora-cli`); ADR-0006; prior-art and design records; `deny.toml`; `THIRD_PARTY_NOTICES.md` |
| `9956edd` | 1 | Workflow model and strict parser; skill-registry reader |
| `f53a93c` | 2 | Provider specs; deterministic planner; Python-oracle plan goldens |
| `1a277a0` | 3 | `aurora workflow validate|show|plan`, `aurora providers`; built-in registry (fixes audit F6); live CLI parity script |
| `513c94f` | 4 | `aurora-fs`: boundaries, no-follow reads, atomic writes, flock lock, run store |
| `46cbb23` | 5 | Engine (start/submit/approve/resume/status); byte-identical handoffs; run CLI; cross-implementation run parity |
| `4593ed2` | 6.1 | Structured verdicts, concerns carried into later handoffs, recorded waivers |
| `9c4f71f` | 6.2 | Role capability requirements with per-provider enforcement strength |
| `827e743` | 6.3 | Six built-in presets; `workflow presets` and `workflow init` |
| `6cd8976` | review | Fixes from the independent R1 review, plus the README section for the Rust CLI |
| *(this commit)* | — | This report |

## 2. Verification

All commands below ran in the container.

| Check | Result |
|---|---|
| `cargo test --workspace` | **203 passed**, 0 failed |
| `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` | clean |
| `cargo deny check` (licences, advisories, bans, sources) | ok |
| `python3 -m unittest discover -s tests` (reference implementation) | **282 passed** (281 + 1 new tie-break test) |
| `scripts/parity.sh` (Python CLI vs Rust CLI JSON + exit codes) | **18/18 identical** |
| `scripts/parity-runs.sh` (both engines on the same run directories) | **18/18**: identical status JSON; Python accepts Rust-written bundles byte for byte; mixed-implementation run to COMPLETE; Python loads Rust runs carrying waivers; a Python-held lock makes the Rust writer wait |
| Golden tests | 20 plan goldens and 20 handoff goldens rendered by the Python oracle; a run record written by the Python engine round-trips |
| Mutation checks | Every safety and gate rule tested this way failed its suite when removed: role ≠ provider, cycle detection, tie-break, fallback under override, ancestor-symlink, sticky bit, `.system`, claim requirement, attempt exhaustion, approval drift check, and the `O_NOFOLLOW` directory walk |
| Host preservation | Read-only snapshot of `~/.claude/skills` and `~/.gemini` (274 nodes) is byte-identical to the one taken before the work; `~/.codex` and `~/.agents` are still absent; no `~/.cargo` writes after the move to the container; host repo tree clean |

## 3. What R1 delivers

- **The Python workflow engine, fully ported:**
  - strict schema v1 parser;
  - deterministic planner with multi-provider routing, single-provider override and declared
    fallback;
  - evidence-gated stage engine;
  - deterministic handoff bundles;
  - safe run store.

  It is schema-compatible in both directions with the Python implementation.
- **Research-driven additions.** All are additive to v1, documented in `workflow-schema.md` §7,
  and invisible in parity output when unused:
  - structured `pass | concerns | fail` verdicts;
  - concerns carried into downstream handoffs;
  - recorded waivers;
  - `requires = ["read_only" | "tool_restriction"]`, with honest hard/soft enforcement per
    provider;
  - six presets (`feature`, `bugfix`, `refactor`, `docs`, `security-review`, `spike`) over one
    provider-neutral role taxonomy.

## 4. Findings during R1

1. **The Python `ProcessLock` follows a symlinked `.awb.lock` and truncates its target.**
   Reproduced: a symlink to a file containing `victim-content` left that file empty. The Rust
   lock refuses the symlink. **The Python code was left unchanged (frozen reference); it needs a
   decision.**
2. **Tie-breaking was untested in both implementations:** every fixture was a linear chain.
   Added `tests/fixtures/workflows/parallel.toml`, with tests in Rust and Python.
3. **Fallback moved between two unavailable providers.** This was found and fixed in the Python
   milestone and carried into the port.
4. **The independent R1 review led to these fixes:**
   - stale evidence after a failed resubmit;
   - `waive` skipping unfinished dependencies;
   - a run directory left behind by a failed `start`;
   - a path-swap race fixed with descriptor-relative I/O (stronger than the Python original);
   - record strictness and panic-proofing.

   The reported broken-pipe panic was a false positive (`anstream` ignores `EPIPE`), and is now
   pinned by a test.
5. **Preset routing defect:** `security-review` routed security and its independent second look
   to the same provider. The preset tests caught it and it was fixed.

## 5. Known limitations and open items

- **No CLI is launched yet** (manual handoff only). The runner milestone must decide between
  ACP-first and native headless modes first (`docs/references/prior-art.md` §7).
- **Protected-project paths are still hardcoded** to `/home/asif/...` (audit F4). They should be
  configurable.
- **Codex skill discovery path:** current docs say `~/.agents/skills`. The skill core must
  reconcile this.
- **Handoff bundles embed the absolute run path,** so moving a run directory makes them look
  edited. Aurora warns and never overwrites.
- **A drifted PASSED stage cannot be re-opened** (v1 limitation). Waivers and resubmission
  cover the gate side.
- **Extensions are Rust-only:** definitions that use `verdict`/`requires` (including the
  presets) are rejected by the strict Python parser. Runs carrying gate data remain loadable by
  Python.
- **The Python code is not yet retired.** The retirement criteria in ADR-0006 also need the
  skill-core port.

## 6. Proposed next milestones (small steps, as before)

1. **Python lock fix decision.** Fix the frozen reference, or retire that code path.
2. **Skill core (Layer A) in Rust:**
   - SKILL.md parser adapted from `codex-rs` (Apache-2.0, with NOTICE);
   - agent path table seeded from vercel-labs/skills (MIT);
   - who-reads-what leakage matrix;
   - `aurora.toml` / `aurora.lock` and an ownership ledger;
   - dry-run, confirm, snapshot and undo for live installs (per the owner's decision);
   - porting the `awb.transactions` / `awb.operations` safety suites.
3. **TUI (`aurora-tui`),** following `docs/design/tui-vision.md`: skills × CLIs matrix, roles
   board, run pipeline.
4. **Runner:** one provider, headless, behind explicit per-run approval, with no bypass flags.
