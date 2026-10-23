# Aurora — Universal Agent Skill Manager

Aurora (canonical CLI: `aurora`, compatibility alias: `awb`) is a universal, centralized manager for discovering, installing, validating, updating, removing, benchmarking, and rolling back reusable coding-agent skills and workflows across supported coding CLIs.

---

## Command Line Interface

Aurora provides both the canonical `aurora` command and the legacy `awb` compatibility alias:

```bash
# Non-mutating environment detection
aurora detect
# Or via alias
awb detect

# List canonical skills from registry
aurora list

# Validate registry syntax, schema, and dependency graphs
aurora validate

# Run system readiness and adapter diagnostics
aurora doctor

# Plan transactional skill operations (dry-run)
aurora install principles --target /path/to/sandbox --root /path/to/sandbox --dry-run

# Execute transactional installation under ProcessLock
aurora install principles --target /path/to/sandbox --root /path/to/sandbox

# Remove managed skill
aurora remove principles --target /path/to/sandbox --root /path/to/sandbox
```

---

## Role-Based Workflows (manual handoff)

Aurora plans and tracks workflows whose stages are owned by provider-neutral **roles** and routed to
coding CLIs (`claude`, `codex`, `antigravity`). Milestone 1 never launches a CLI. It writes a handoff
prompt per stage and accepts a stage only when its declared evidence exists.

```bash
aurora providers                                   # execution providers + PATH detection (no process run)
aurora workflow validate workflows/examples/plan-implement-review.toml
aurora workflow show     workflows/examples/plan-implement-review.toml
aurora workflow plan     workflows/examples/plan-implement-review.toml                    # multi-provider routing
aurora workflow plan     workflows/examples/plan-implement-review.toml --provider codex   # single-provider override
aurora workflow plan     workflows/examples/plan-implement-review.toml --route implement=claude

mkdir -p /tmp/aurora-runs
aurora workflow start  workflows/examples/plan-implement-review.toml --run-root /tmp/aurora-runs --run-id demo
aurora workflow submit --run /tmp/aurora-runs/demo --stage plan --met plan-written
aurora workflow status --run /tmp/aurora-runs/demo
aurora workflow resume --run /tmp/aurora-runs/demo
aurora workflow approve --run /tmp/aurora-runs/demo --stage final-verify --by "<name>"
```

See `docs/product-vision.md`, `docs/architecture/` and `docs/adr/` for the model and its guarantees.

---

## Architecture & Layers

- **Layer A — Manager Core:** Registry parser, version solver, capability discovery, operation planning, audit history, and transaction rollback engine.
- **Layer B — Canonical Registry:** Authoritative index of skill metadata, versions, triggers, and compatibility contracts (`registry/skills.toml`).
- **Layer C — Agent Adapters:** Modular adapters for Google Antigravity CLI, OpenAI Codex CLI, and future agents.
- **Layer D — Canonical Skills:** Core engineering baseline and conditional specialist skills.
- **Layer E — Verification & Benchmarks:** Adversarial safety tests, failure injection suites, and crash recovery audits.

---

## Safety-First Invariants

1. **Deterministic Path Boundaries:** Traversal outside declared roots is rejected; ancestor symlinks fail closed.
2. **Non-Blocking Descriptor Inspection:** Sensitive file operations use `O_NONBLOCK | O_NOFOLLOW` with descriptor `fstat` validation to eliminate FIFO and named-pipe blocking hazards.
3. **Descriptor-Relative Pinning:** Directory entries are opened and unlinked relative to pinned parent descriptors (`dir_fd`) using kernel `unlinkat(p_dfd, basename, AT_REMOVEDIR)`.
4. **Crash Durability:** State changes use write-ahead durability logging (`.awb.journal.json`), staged atomic writes with `os.replace`, and parent directory `fsync`.
5. **Mandatory Process Locking:** Every mutation runs under an exclusive `ProcessLock` (`.awb.lock`) with inode/device validation to prevent split-brain execution.
6. **Zero Third-Party Runtime Dependencies:** Implemented strictly in Python 3.11+ using standard library modules (including `tomllib`).

---

## Running Safety & Reliability Tests

```bash
python3 -m unittest discover -s tests -v
```
