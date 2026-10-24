# TUI Vision

## Status (R3.1, 2026-10-24)

**Beginner pass.** See `docs/reports/AURORA_R3_1_BEGINNER_REPORT.md`.

- Plain words throughout (AI tool, job, step, task, checklist), with engine terms under
  "More details". The tabs are now Skills, Who does what, Add to project and Tasks.
- A guided setup on first launch (`t` reopens it).
- `n` starts a task in the project's `.aurora/runs`.
- A one-line installer built with cargo-dist.

## Status (R3, 2026-10-24)

**Built.** See `docs/reports/AURORA_R3_TUI_REPORT.md`.

- The launch rules.
- Four tabs: **Skills** (the skills × CLIs matrix), **Roles** (the board, with `p` reassigning a
  CLI), **Install** (in the place of the planned Workflows tab: review, apply, undo) and **Runs**.
- Help generated from the key table, the live filter, confirm dialogs that default to No, toasts.
- Worker threads with a spinner; mouse support; the `NO_COLOR` and high-contrast themes.
- Snapshot tests and a tmux smoke test.

**Deferred.** The Workflows and Config tabs, the `:` / `Ctrl-P` command palette, the TOML
keymap with a user patch file, and editing workflow files in the TUI.

The original vision follows unchanged.


The goal is ASM's interaction model, without ASM's known weaknesses, on top of Aurora's
role/workflow core. Inputs:

- the ASM source walkthrough (2026-10-23);
- the Rust TUI UX research (lazygit, gitui, k9s, helix, yazi);
- the prior art in `../references/prior-art.md`.

## Launch

`aurora` with no arguments, attached to a TTY, opens the TUI:

- alternate screen;
- the terminal is restored on exit, on panic and on SIGINT (exit 130).

Any subcommand runs the scriptable CLI. If stdout is not a TTY and no subcommand is given, Aurora
prints help and exits 2.

## Screens (tabs `1`–`5`; list on the left, detail on the right; a single pane below ~100 columns)

1. **Skills**: a skills × CLIs matrix, one row per skill. Each CLI column shows one of
   `✓` linked, `©` copy, `≠` drift, `!` foreign, `✗` broken, `·` available. This replaces
   ASM's one-row-per-skill-per-tool layout. The detail pane shows:
   - frontmatter;
   - token estimate;
   - allowed-tools risk;
   - which CLIs read it (the leakage matrix);
   - the equivalent CLI command.
2. **Roles**: a board of role → assigned CLI → skills. Reassign a role's CLI with `p`. The
   board shows enforcement strength (`hard` / `soft` / `none`) for each requirement, such as a
   read-only reviewer.
3. **Workflows**: definitions and presets. Validate. Plan preview in three modes: routing,
   `--provider`, `--route`.
4. **Runs**: the stage pipeline, each stage showing state, provider, attempts and missing
   evidence. Actions: open handoff, copy prompt, submit evidence, approve, resume.
   Drift and reduced-independence badges.
5. **Config**: providers on/off and their paths, scopes, keymap and theme.

Overlays:

- `?` help, generated from the same keymap table as the footer;
- `:` / `Ctrl-P` command palette;
- confirm dialogs that list exact paths and default to **No**.

## Keys (ASM-compatible where sensible)

| Area | Keys |
|---|---|
| Moving around | `j`/`k`/`↑`/`↓`, `g`/`G`/Home/End, `Ctrl-d`/`Ctrl-u`, PgUp/PgDn. Wraps consistently in every list |
| Focus | `Tab`/`Shift-Tab` cycles focus; `1`–`5` switch tabs |
| Filter | `/` opens a live fuzzy filter with highlighted matches. `Enter` keeps it; `Esc` clears it |
| Esc | backs out one level: modal → filter → parent |
| Quit | `q` quits, from the top level only |
| Actions (ASM vocabulary) | `Enter` detail, `i` install/activate, `d` remove, `l` link, `s` sort, `S` scope, `r` refresh (the "updated" toast appears only after the scan finishes), `a` audit, `c` config |
| Config edits | `Ctrl-S` saves; `Esc` asks whether to save or discard. This fixes ASM's Esc-saves / q-discards inversion |

## Rules

- **Footer** shows the five to eight keys relevant to the focused pane.
- **No blocking:** long work runs on worker threads, with a spinner, `Ctrl-C` to cancel the job,
  and a result toast.
- **Colour is never the only signal.** Glyphs come first. `NO_COLOR` is honoured, the default is
  16 colours, and a high-contrast theme is available. Never hardcode a background.
- **Mouse** works for focus, select and scroll. **Resize** reflows the layout, with a
  "terminal too small" notice when needed.
- **Every TUI action has a CLI equivalent,** shown in the detail pane.
- **Destructive actions** show a dry-run diff first and offer undo through the transaction
  journal.

## Architecture

The structure follows gitui and serie (MIT, pattern only):

- One `App` model and one `Action` enum.
- Screens are components with `handle_key → Option<Action>`, `update(Action)` and
  `render(&self, Frame, Rect)`.
- A popup stack for modals, as in codex's view stack and gitui's `popup_stack`.
- A TOML keymap with a user patch file. Footer and help are generated from it, with a reverse
  action→key map (television).
- A synchronous crossterm loop. Workers send `Action`s back over an `mpsc` channel.
- Tests:
  - `TestBackend` with insta snapshots per screen;
  - `update()` transitions tested as pure functions;
  - a few PTY smoke tests.
