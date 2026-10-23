# Aurora — Universal Agent Skill Manager

Aurora (canonical CLI: `aurora`, compatibility alias: `awb`) is a universal, centralized manager for discovering, installing, validating, updating, removing, benchmarking, and rolling back reusable coding-agent skills and workflows across supported coding CLIs.

---

## Rust implementation (Milestones R1–R3)

Aurora is being rewritten in Rust (ADR-0006). The Python package below is the reference
implementation until parity. In R1 the Rust `aurora` binary covers the **workflow engine**. It
runs in **manual-handoff** mode: Aurora never launches a coding CLI; it writes a handoff prompt
for each role and checks the evidence you submit.

```bash
cargo build --release                       # binary: target/release/aurora (MSRV 1.89)

aurora providers                            # claude / codex / antigravity (agy): PATH detection only
aurora workflow presets                     # feature, bugfix, refactor, docs, security-review, spike
aurora workflow init feature                # writes ./feature.toml (never overwrites)
aurora workflow plan feature.toml           # roles -> CLIs (preferred, with declared fallback)
aurora workflow plan feature.toml --provider codex          # every role on one CLI (reduced independence reported)
aurora workflow plan feature.toml --route implement=claude  # move one stage to another CLI

mkdir -p ~/aurora-runs
aurora workflow start feature.toml --run-root ~/aurora-runs --run-id my-feature
#   -> paste ~/aurora-runs/my-feature/handoff/intake.md into the assigned CLI, save its outputs, then:
aurora workflow submit --run ~/aurora-runs/my-feature --stage intake --met criteria-testable
aurora workflow status --run ~/aurora-runs/my-feature
aurora workflow waive  --run ... --stage S --reason "..." --by NAME   # recorded human decision
aurora workflow approve --run ... --stage accept --by NAME
```

A stage passes only when every declared output exists and every acceptance criterion is
explicitly claimed. Exit codes never count. Reviewer verdicts (`pass | concerns | fail`) gate
the stage, and concerns are carried into later handoffs. Dev checks: `cargo test`,
`scripts/parity.sh` and `scripts/parity-runs.sh`, which compare the Rust and Python
implementations. Design notes for the upcoming TUI and skill manager are in `docs/design/`.

### Skills (Milestone R2: curated catalog and safe installs)

Aurora recommends popular, maintained agent skills per workflow stage for the CLIs you actually use.
It installs them into a project with safeguards. The catalog pins every skill to a commit and a
content hash, and ships no third-party skill text (ADR-0007,
`docs/research/agent-skills-landscape-2026-10.md`).

```bash
aurora catalog list                                   # skills, rules, frameworks, infra; trust and roles
aurora catalog recommend --preset bugfix              # pair (claude + codex) by default
aurora catalog recommend --preset bugfix --clis claude          # solo: reduced independence reported
aurora skills fetch code-review                       # fetch at the pinned commit, verify, scan
aurora skills plan  --preset bugfix --project .       # dry run: what would change, what each CLI loads
aurora skills apply --preset bugfix --project .       # asks first; --reviewed ENTRY for flagged skills
aurora skills undo  --project .                       # restores; nothing is ever deleted
aurora workflow start bugfix.toml --profile pair --run-root ~/aurora-runs   # handoffs name the skills
```

Safeguards:

- **Integrity:** fetched content must match the pin.
- **Scanning:** the scanner blocks symlinks, binaries, archives and padding, and flags scripts,
  hooks and `!`cmd`` for review.
- **No pre-approved tools:** `allowed-tools` and `hooks` are removed from installed copies.
- **Ownership:** Aurora only changes what `aurora.lock` says it installed and that is unchanged.
- **Undo:** every apply is journaled under `.aurora/ops/` and can be undone.

Commit `aurora.lock`; ignore `.aurora/`.

### Terminal UI (Milestone R3)

`aurora` with no arguments opens the TUI in a terminal, and so does `aurora tui`. In scripts and
pipes it prints help instead.

```bash
aurora tui --project . --run-root ~/aurora-runs      # --preset bugfix (default), --clis claude,codex
```

| Tab | What it shows | Keys |
|---|---|---|
| **1 Skills** | Each skill × each CLI: `✓` installed, `+` create, `?` after review, `↑` update, `≠` modified, `!` foreign, `◐` via a shared dir, `·` not wanted; budget bars | `i` opt-in, `r` re-plan |
| **2 Roles** | Stage → role → CLI → skills; enforcement strength for read-only roles | `p` another CLI for this role, `P` profile, `w` workflow |
| **3 Install** | The plan's actions, review state, warnings | `Enter` read flagged files, `r` reviewed, `a` apply (asks; only `y` confirms), `u` undo |
| **4 Runs** | Runs and their stage pipelines | `Enter` open / read handoff, `s` checkbox submit, `A` approve, `R` resume, `y` copy path |

Keys that work everywhere:

- `1`–`4` switch tabs, and `Tab` moves focus between the list and the detail pane.
- `/` filters the list.
- `?` shows help for every key; it is generated from the same table as the footer.
- `Esc` backs out one level, and `q` quits.

The mouse wheel and clicks also work. `NO_COLOR` turns colour off, and
`AURORA_THEME=high-contrast` switches to a high-contrast theme. Each detail pane shows the
equivalent CLI command. Real-terminal check: `scripts/tui-smoke.sh`.

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
