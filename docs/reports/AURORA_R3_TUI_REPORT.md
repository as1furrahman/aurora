# Aurora R3 report: the terminal UI

**Branch:** `feat/rust-workflow-engine` (dev container working copy `/opt/aurora`, pushed to the
host repository's branch).

**Date:** 2026-10-24.

**Owner's request:** "lets create the tui first now".

## Outcome

`aurora` with no arguments, in a terminal, now opens a TUI, as does `aurora tui`. It is modelled
on luongnv89/asm's interaction model and sits over the R1 workflow engine and the R2 skill
catalog. Its four tabs cover the whole loop:

1. **Skills.** A skills × CLIs matrix of what the install plan will do on each CLI, a detail pane
   with provenance and review state, and listing-budget bars.
2. **Roles.** Stage → role → CLI → skills. Any CLI can take any role in one key, and the screen
   shows how strongly that CLI enforces what the role requires.
3. **Install.** Review flagged files in a viewer, then apply behind a confirm dialog that defaults
   to No, then undo.
4. **Runs.** Evidence-gated runs with a stage pipeline, the handoff viewer, a checkbox submit form,
   and named approval.

The TUI adds no mutation logic. Every action goes through the same `aurora-app` and engine
functions as the CLI, and each detail pane shows the equivalent command.

## Steps

| Step | Commit | What |
|---|---|---|
| 0 | `3823cfc` | `aurora-app` crate: the plan/apply/undo orchestration and fetch layer move out of the binary; the CLI becomes a thin printer (no behaviour change: all tests and both demos unchanged) |
| 1 | `bbade12` | `aurora-tui` skeleton: one `App`, one `Action`, pure `update`; one key table that drives keys, footer and help; tabs, focus, filter, help; terminal restored on exit and panic; launch wiring; also fixed `aurora \| head` panicking on a closed pipe |
| 2 | `5c1d4f4` | Skills tab over the real install plan; worker thread with generation-checked results; opt-in toggles |
| 3 | `7f5b32c` | Roles tab: `p` gives a role's slot to the next free CLI, `P` switches profile, `w` switches workflow; enforcement strength and slot-mates shown |
| 4 | `d9c139c` | Install tab: verified-copy viewer, `r` reviewed, `a` apply (only `y` confirms), `u` undo, toasts and a persistent "last:" line; the worker re-checks the boundary and review gate |
| 5 | `bdc8773` | `aurora_app::runs` and the Runs tab: list, pipeline, handoff viewer, checkbox submit, named approval, resume, OSC 52 copy |
| 6 | `ed0762b` | Mouse (wheel, tab and row clicks), high-contrast theme, `scripts/tui-smoke.sh` |
| docs | (this commit) | this report, `docs/design/tui-vision.md` status, README TUI section |

## Verification

**Automated checks:**

- **Rust:** 380 tests (43 in the TUI). Clippy (`-D warnings`), fmt and `cargo deny` are clean.
- **Python reference suite:** 282 tests.
- **Rust-vs-Python parity:** CLI 18/18, runs 18/18.

**TUI tests.** The model is tested through pure transitions, and rendering through ratatui
`TestBackend` snapshots. The fixtures are offline: a local git mirror and a test catalog, planned
with the real `plan_install`. Fixed git dates and an insta filter keep the snapshots reproducible.
The tests include:

- a real apply → undo round trip on disk;
- the worker refusing a hand-built Apply that skips review;
- real runs started with the R1 engine: a failing submit, a passing submit, approval completing
  the run.

**Mutation checks.** Each of these deliberate breakages was caught by a test:

- the per-CLI ownership check in the matrix;
- Enter confirming the dialog;
- the app skipping the review gate;
- the worker skipping its re-check.

**Real terminal.** `scripts/tui-smoke.sh` runs the real binary in tmux with real keys and SGR
mouse events, against a python-tabulate clone and the built-in catalog. It passes 22/22:

- launch and plan load; help; filter;
- `p`/`P` on the Roles tab;
- review → confirm (Enter means No) → apply → undo, with the project identical to a fresh clone;
- a checkbox submit that passes the gate;
- a mouse tab click;
- `q` exiting 0 with the terminal restored.

The completed `examples/real-bugfix-demo` run also renders correctly in the Runs tab: 5/5 across
codex, hermes, antigravity and claude.

**Host safety.** The host agent-directory snapshot is unchanged after all R3 work.

## Decisions and findings

- **The TUI is a front end, not a second engine.** Work is queued as `Command`s and run by
  `execute()` on a worker thread. The tests call the same `execute()`.
- **Safety is checked twice.** The model refuses apply while any skill is unreviewed, and the
  worker re-checks the project boundary and the review gate before writing.
- **Roles share profile slots.** In the pair profile, tester and reviewer are both slot 2, so `p`
  on review also moves reproduce and accept. The detail pane says so ("this CLI also plays
  tester").
- **Mouse row clicks select only while the list is not scrolled.** That way the clicked row is
  always the row that gets selected, never a guess.
- **One pre-existing bug was found and fixed:** `aurora | head` panicked on the closed pipe.
- **Python `ProcessLock` symlink bug: accepted, not fixed (owner, 2026-10-24).** It is low
  severity. It affects only the frozen Python oracle, needs a symlink planted in a directory the
  user already owns, and can only truncate a user-owned file. The Rust implementation refuses the
  case.

## Deferred

- **Not built yet:** the Workflows and Config tabs, the command palette, a user keymap file, and
  editing workflow files in the TUI.
- **Remembering reviews across sessions.** The TUI's "reviewed" marks, like `--reviewed`, last
  for one session.
- **Panic restore under a real PTY.** It relies on `ratatui::init`'s documented panic hook; no
  test triggers a panic on purpose.

## Next (recommendation)

- **R4, the runner.** Launch one provider headless, behind per-run approval, with argv-only
  invocation. Include the bounded Ralph loop (fresh context, iteration and token budgets,
  test-based exit) and the pair-mode review loop with capped rounds. The Runs tab becomes the
  place where a stage is started and watched.
- **R5.** Retire the Python reference, then unify the legacy registry with the catalog. Also make
  the protected-path list configurable.
