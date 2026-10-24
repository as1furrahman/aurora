# Rust Stack

**Researched:** 2026-10-23. Crate versions are from crates.io on that date.
**Toolchain:** rustc/cargo 1.99 (Fedora packages). MSRV `1.89` (std `File::lock`). Edition 2024.

## Milestone R1 dependencies

| Crate | Version | Use |
|---|---|---|
| serde, serde_json | 1.x | plan and run-record models |
| toml | 1.x | parse workflow definitions into `toml::Table` for validation that collects every error |
| thiserror | 2.x | typed errors in core |
| sha2 | 0.11 | evidence and definition hashes |
| jiff | 0.2 | UTC timestamps |
| which | 8.x | PATH detection of `claude` / `codex` / `agy`. A binary is never executed |
| libc | 0.2 | `O_NOFOLLOW`, `O_NONBLOCK`, `fstat`, in `aurora-fs` only |
| clap | 4.6 (derive) | CLI |
| anstream / anstyle | 1.x | colour that honours `NO_COLOR` / `CLICOLOR` |
| dev: tempfile, assert_cmd, predicates, assert_fs, insta, proptest | latest | tests |

## Later milestones

| Need | Choice | Notes |
|---|---|---|
| TUI | ratatui 0.30 + crossterm 0.29 | `ratatui::init` restores the terminal on panic. Widgets: `tui-input`, `ratatui-textarea` (not stalled `tui-textarea`), `tui-tree-widget`, `throbber-widgets-tui`, `tui-popup` |
| Fuzzy filter | nucleo-matcher 0.3 | used by helix. `fuzzy-matcher` is unmaintained |
| Prompts outside the TUI | inquire 0.9 | never prompt when stdin/stdout is not a TTY, or with `--yes` / `--json` |
| Progress | indicatif 0.18 | hidden on non-TTY and with `--json` |
| Error rendering | miette 7 at the CLI/TUI edge | labelled spans for frontmatter errors |
| SKILL.md YAML | **serde-saphyr 2** | `serde_yaml` is deprecated. `serde_yml` is RUSTSEC-2025-0068 (unsound, archived) |
| Format-preserving config edits | toml_edit 0.25 | user config and Codex `config.toml` |
| Directory-scoped filesystem access | cap-std 4 (optional) | `openat2`/`RESOLVE_BENEATH` on Linux |
| Paths | etcetera (XDG), camino, fs-err | |
| Process supervision | tokio::process + process-wrap; codex `process_group.rs` (copied) | runner only |
| Git worktrees | shell out to `git` | as codex does. Avoids libgit2 |
| ACP | agent-client-protocol 3.x behind a feature | pin exactly; majors are moving fast |
| Release | dist (cargo-dist) | installers, Homebrew, checksums |

## Rules

- `aurora-core` depends on no terminal, async or filesystem-mutation crates.
- All filesystem mutation goes through `aurora-fs`.
- Run `cargo deny check` (licences and advisories) when `cargo-deny` is available.
- Every copied file carries a "Modified from <repo>@<sha> (<licence>)" header and an entry in
  `THIRD_PARTY_NOTICES.md`.
