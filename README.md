# Aurora

**Aurora helps AI coding tools do better work.** It gives tools like Claude Code, Codex,
Antigravity and Hermes ready-made *skills*: instructions that teach them one kind of work well,
such as finding the real cause of a bug or writing tests first. It can also split your work into
steps, each done by one tool, with a quick check before moving on.

It's built for people starting out with AI coding ("vibe coding") as well as experienced
developers. Aurora never changes your project without asking first, and every change can be
undone.

## Quick start

**1. Install** (macOS and Linux). Paste this into a terminal:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/as1furrahman/aurora/releases/latest/download/aurora-cli-installer.sh | sh
```

Then **close the terminal and open a new one**, so it can find the `aurora` command.

**2. Open your project.** Go to your project's folder and start Aurora:

```sh
cd path/to/your-project
aurora
```

**3. Answer three questions.** Aurora asks which AI tools you use and what you're working on,
then shows exactly what it would add. Nothing is saved until you press `y`.

After that, Aurora has four pages (press `1` to `4`, or click them):

| Page | What it's for |
|---|---|
| **Skills** | The skills that fit your task, and which AI tool gets each one |
| **Who does what** | Which tool does each job. Using two tools means one checks the other's work |
| **Add to project** | Exactly what will be added. Read anything marked "check first", then press `a` |
| **Tasks** | Your work split into steps. Press `n` for a new task, `Enter` to see a step's instructions, `s` to tick its checklist |

Press `?` at any time for help and a short glossary, `t` to run the setup again, or `q` to
quit.

### Good to know

- **Undo:** on the *Add to project* page, `u` puts your project back the way it was before
  Aurora's last change. Files you edited yourself are left alone.
- **Where things go:** skills are copied into your project (for example `.claude/skills/` and
  `.agents/skills/`), with always-on rules in `CLAUDE.md` or `AGENTS.md`. `aurora.lock` records
  what Aurora added, and `.aurora/` holds its backups and your tasks. Commit the skills and
  `aurora.lock`; most people add `.aurora/` to `.gitignore`.
- **Community skills** are marked *check first*. Have a quick read (press `Enter`) before
  adding them.
- **Aurora never runs your AI tools for you.** It prepares the instructions, and you paste them
  into the tool you already use.
- **Extra protection:** set `AURORA_PROTECTED_PROJECTS` to an OS path-list of projects Aurora
  must never change. For example, on macOS/Linux: `export AURORA_PROTECTED_PROJECTS="$HOME/work/important:$HOME/work/client"`.

### Prefer to read the install script first?

That's a good habit. Download it, read it, then run it:

```sh
curl --proto '=https' --tlsv1.2 -LsSf -o aurora-installer.sh https://github.com/as1furrahman/aurora/releases/latest/download/aurora-cli-installer.sh
less aurora-installer.sh
sh aurora-installer.sh
```

The installer downloads the `aurora` program for your computer from this project's GitHub
releases and checks its SHA-256 checksum. You can also download the archive from the
[releases page](https://github.com/as1furrahman/aurora/releases) yourself, or build from source
with `cargo install --git https://github.com/as1furrahman/aurora aurora-cli`.

### Uninstall

```sh
rm "$(command -v aurora)"
```

Any skills Aurora added to a project stay there until you remove them: run `aurora`, go to
*Add to project* and press `u`, or delete the files.

**Windows** isn't supported yet. On Windows, use WSL (Ubuntu) and follow the Linux steps.

---

## For developers

Aurora is a Rust CLI and TUI for managing reusable coding-agent skills and evidence-gated workflows across supported coding CLIs.

### Rust implementation

The Rust workspace is the canonical implementation (ADR-0006). Aurora runs in **manual-handoff**
mode: it does not launch coding CLIs; it writes a handoff prompt for each role and checks the
evidence you submit.

```bash
cargo build --release                       # binary: target/release/aurora (MSRV 1.89)

aurora providers                            # claude / codex / antigravity (agy): PATH detection only
aurora doctor                              # registry, provider, staging and project write checks
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
the stage, and concerns are carried into later handoffs. Dev checks: `cargo fmt --check`,
`cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace`. Design
notes for the upcoming TUI and skill manager are in `docs/design/`.

#### Skills (Milestone R2: curated catalog and safe installs)

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

#### Terminal UI (Milestones R3 and R3.1)

`aurora` with no arguments opens the TUI in a terminal, and so does `aurora tui`. In scripts and
pipes it prints help instead. On first launch in a project (no `aurora.lock`) a guided setup asks
which AI tools you use (detected on PATH) and what you're working on; `t` reopens it.

```bash
aurora tui --project . --run-root ~/aurora-runs      # --preset bugfix (default), --clis claude,codex
```

Without `--run-root`, tasks live in the project's `.aurora/runs`.

| Page | What it shows | Keys |
|---|---|---|
| **1 Skills** | Each skill × each AI tool: `✓` added, `+` will add, `?` check first, `↑` will update, `≠` you changed it, `!` yours (left alone), `◐` shared folder, `·` not used; room left in each tool's skill list | `i` add/remove optional, `r` refresh |
| **2 Who does what** | Step → job → AI tool → skills, and how strongly each tool can be kept read-only | `p` another tool for this job, `P` 1, 2 or 3 tools, `w` task type |
| **3 Add to project** | Exactly what will change, what to check first, warnings | `Enter` read it, `r` mark as checked, `a` add (asks; only `y` confirms), `u` undo |
| **4 Tasks** | Tasks and their steps | `n` new task (asks), `Enter` open / show a step's instructions, `s` checklist, `A` give your OK, `R` check task, `d` archive task (asks), `y` copy path |

Keys that work everywhere:

- `1`–`4` switch pages, and `Tab` moves focus between the list and the details pane.
- `/` searches the list.
- `?` shows how Aurora works, a glossary and every key; the keys come from the same table as the
  footer.
- `Esc` goes back one level, and `q` quits.

The mouse wheel and clicks also work. `NO_COLOR` turns colour off, and
`AURORA_THEME=high-contrast` switches to a high-contrast theme. The TUI uses plain words (AI
tool, job, step, task, checklist); the engine's terms, source links and the equivalent CLI
commands are under **More details** in each details pane. Real-terminal check:
`scripts/tui-smoke.sh`.

#### Releases

Releases are built by [cargo-dist](https://github.com/axodotdev/cargo-dist) (MIT OR
Apache-2.0) from `dist-workspace.toml`: macOS (Apple Silicon and Intel) and Linux (x86_64 and
arm64 glibc, plus x86_64 static musl, which the installer uses on older distributions), each
with a SHA-256 checksum, and a shell installer. Pushing a tag `vX.Y.Z` that matches the
workspace version runs `.github/workflows/release.yml`, which builds every target on its own
runner and publishes a GitHub release. `scripts/test-installer.sh` tests the installer end to
end against a local stand-in for GitHub releases (run `dist build` first).

### Command Line Interface

Use the `aurora` command:

```bash
# Check which supported AI tools are available (does not run them)
aurora providers

# Check the registry, installed providers, staging leftovers and current-directory write access
aurora doctor

# Browse the pinned, curated skill catalog (read-only)
aurora catalog list

# Validate a workflow definition before starting it
aurora workflow validate workflows/examples/plan-implement-review.toml

# See what skills Aurora would add (dry run)
aurora skills plan --preset bugfix --project .

# Add the planned skills after reviewing any flagged files
aurora skills apply --preset bugfix --project . --reviewed <entry>

# Restore Aurora's most recent change without overwriting your edits
aurora skills undo --project .
```

`aurora doctor` does not launch provider tools or delete leftovers. It checks the current
directory and creates then removes a temporary file to verify write access. It exits nonzero when
the configured registry is invalid, staging debris remains, the directory cannot be inspected, or
the write probe fails. Missing provider CLIs are informational because manual handoffs remain
available.

---

### Role-Based Workflows (manual handoff)

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

List and archive tasks managed in a project's default `.aurora/runs` directory. Archiving keeps
the task files under `.aurora/archive/runs`:

```bash
aurora task list --project .
aurora task list --project . --json
aurora task list --project . --archived --json  # find an archived task id to restore
aurora task archive <task-id> --project .   # asks for confirmation
aurora task archive <task-id> --project . --yes  # non-interactive
aurora task restore <task-id> --project .   # restores an archived task
```

See `docs/product-vision.md`, `docs/architecture/` and `docs/adr/` for the model and its guarantees.

---

### Architecture & Crates

- **`aurora-core`:** workflow parsing and planning, provider metadata, skill registry, and curated
  catalog.
- **`aurora-fs`:** safe filesystem operations, cross-process locking, run persistence, evidence
  gates, and drift detection.
- **`aurora-skills`:** skill parsing and scanning, pinned fetching, install planning, transactional
  apply, and undo.
- **`aurora-app`:** shared orchestration used by the CLI and TUI.
- **`aurora-cli` and `aurora-tui`:** scriptable commands and interactive project UI.
- **Tests:** Rust unit and integration tests, including static compatibility fixtures and
  filesystem failure-injection coverage.

---

### Safety-First Invariants

1. **Deterministic Path Boundaries:** Traversal outside declared roots is rejected; ancestor symlinks fail closed.
2. **No-Follow Filesystem Access:** Sensitive paths use descriptor-relative operations with `O_NOFOLLOW` checks.
3. **Atomic Writes:** Files are staged, synced, and renamed atomically within the destination directory.
4. **Crash Recovery:** Skill operations use durable journals and can be undone after interruption.
5. **Cross-Process Locking:** Mutations use a kernel lock to prevent concurrent writers.
6. **Native Runtime:** The product is built and distributed from the Rust workspace without a Python runtime.

---

### Running Safety & Reliability Tests

```bash
cargo test --workspace
```
