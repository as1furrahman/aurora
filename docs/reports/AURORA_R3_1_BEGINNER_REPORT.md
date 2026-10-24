# Aurora R3.1 report: beginner-friendly TUI and a one-command install

**Branch:** `feat/rust-workflow-engine` (dev container working copy `/opt/aurora`, pushed to the
host repository's branch only; nothing pushed to GitHub or published).

**Date:** 2026-10-24.

**Owner's feedback:** "the wordings, the ui is too technical, the design and the explanations for
everything should be more novice friendly for those who're starting out with vibe coding for
example. fix this first. also the installation process should be simplified. for example using
curl and a single command to install the whole thing".

**Decisions** (asked 2026-10-24):

- **Release.** Prepare and test the release, and the owner publishes it.
- **Setup.** Add a guided setup on first launch.
- **Scope.** The plain-language rewrite covers the TUI, the installer and the README. CLI output
  keeps its precise wording.

## Outcome

A newcomer installs Aurora with one line:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/as1furrahman/aurora/releases/latest/download/aurora-cli-installer.sh | sh
```

They then run `aurora` in their project folder and answer three questions:

1. Which AI tools do you use? Installed tools are detected and ticked.
2. What are you working on?
3. Here's what Aurora will add.

They review and add the skills on the next page, press `n` to start a task, paste step 1's
instructions into its tool, and tick its checklist. Everything is in plain words (AI tool, job,
step, task, checklist). The engine's terms, source links and equivalent CLI commands are still
there, under a dimmed **More details** heading.

The same journey runs as section 8 of `scripts/tui-smoke.sh` in a real terminal:

- the setup;
- adding skills, including checking the two community ones;
- `n` to start a task;
- reading its instructions;
- step 1's checklist, gated on the files existing;
- `aurora workflow status` seeing the same task;
- undo;
- the project matching a fresh clone.

## Steps

| Step | Commit | What |
|---|---|---|
| 0 | `f5b8e9d` | Plain-language wording. `words.rs` holds tool and job names, task types, step states, and plain rewrites of plan reasons and warnings. The pages are renamed (Skills, Who does what, Add to project, Tasks) and each opens with a "what is this" line. The footer uses plain verbs. Help adds "How Aurora works in 30 seconds" and a glossary. Detail panes use a hanging indent (`ui::hang`). `plain_words.rs` fails if engine words reach a page outside "More details". |
| 1a | `9a5a087` | Fix: skill fetches on two threads in one process collided on the staging name (pid only). One failed with "File exists", and its cleanup deleted the other thread's checkout. A first-time user hit it on their first run (setup plan plus startup plan on a cold cache). A concurrent regression test was added. |
| 1b | `b8d16dd` | Guided setup on first launch (no `aurora.lock`); `t` reopens it. Tool detection moves to `aurora_app::tools` (`aurora providers` is unchanged). Also fixed: quick `P`/`p` presses built on the last delivered plan, not the last request (caught by the tmux smoke test). |
| 2 | `c16374a` | Start a task from the Tasks page: `n` shows the steps and tools, asks yes or no, writes `.aurora/workflows/<type>.toml` (keeping an edited one), starts a run in `.aurora/runs` and opens it. Profile routing (`profile_routes`) and registry loading move to `aurora-app`; the CLI uses them with unchanged output. Fixed: two tasks started in the same second collided on the run id. |
| 3a | `de66869` | Fix: the macOS builds would not compile, because `mode_t` and `st_dev` are platform-sized. Found by `cargo check`/`clippy` for the release targets. |
| 3b | `a0a0f17` | cargo-dist 0.32 release: `dist-workspace.toml`, `.github/workflows/release.yml`, version 0.3.0, a beginner-first README and `scripts/test-installer.sh`. |
| 4 | (this commit) | The beginner journey in `scripts/tui-smoke.sh`; this report; the `tui-vision.md` status. |

## Verification

All checks below ran in the `dev` container.

- **Rust:** 410 tests. `fmt`, `clippy -D warnings` and `cargo deny` are clean.
- **Python reference:** 282 tests.
- **Parity:** both scripts (plans, and runs in both directions).
- **Demos:** real-bugfix 25/25 and skills-install 21/21.
- **TUI in a real terminal (tmux):** `scripts/tui-smoke.sh` passes 40/40, three runs in a row.
  That is the 22 R3 checks with the new wording, 2 for the setup, and 16 for the beginner journey.
- **Plain words:** every page (every row of each), help, the yes/no dialogs, the setup screens,
  task steps and the checklist are rendered and checked for engine words.
- **Release targets:** `cargo clippy --workspace --all-targets -D warnings` passes for
  `aarch64-apple-darwin`, `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu` and
  `x86_64-unknown-linux-musl`. These are compile-level checks only.
- **Static musl build:** built and run in the container (`static-pie`, 7.4 MB).
- **Installer:** `scripts/test-installer.sh` passes 10/10 against a local stand-in for GitHub
  releases.
  - `curl | sh` into an empty home with a minimal PATH installs to `~/.local/bin`, and a new login
    shell finds `aurora`.
  - A tampered archive is refused with "checksum mismatch" and nothing is installed.
  - The installed `aurora` opens the guided setup and writes nothing into the project.
- **CI matrix:** from `dist plan`. Each target builds on its own native runner (`macos-14`,
  `macos-15-intel`, `ubuntu-22.04`, `ubuntu-22.04-arm`); the musl job installs `musl-tools`.

## Decisions and findings

- **Engine words are moved, not removed.** Beginners read the top of each pane; the details,
  links and equivalent commands are below "More details", so nothing is hidden from experienced
  users.
- **The setup owns the whole screen.** Showing the pages behind it only distracted on a first
  run. It writes nothing; the next page asks before adding anything.
- **Tasks load when the Tasks page opens, not at startup.** The list is always current, including
  tasks started from the CLI.
- **Install to `~/.local/bin`, not `~/.cargo/bin`.** Most people installing Aurora don't use
  Rust.
- **Linux gets glibc and static musl builds.** The installer uses musl on older distributions.
  Windows is left out, because the filesystem layer is Unix-only; the README points to WSL.
- **Five bugs were found by exercising real use**, not by reading code:
  - concurrent fetch staging;
  - stale state on quick key presses;
  - colliding run ids;
  - the macOS build;
  - a test-harness stale web server, which made the tamper test pass for the wrong reason. Now
    guarded.

## Open items for the owner

- **Licence files.** `Cargo.toml` declares MIT OR Apache-2.0, but the repository has no licence
  files. The MIT text needs a copyright holder, so this is left to the owner. Once added,
  cargo-dist ships them in every archive automatically.
- **macOS runtime is untested.** The code is type-checked for macOS but has not run there. The
  first release, or a Mac, should run `cargo test` before announcing macOS support.
- **Publishing.** Push the branch to GitHub and merge it. Then `git tag v0.3.0 && git push origin
  v0.3.0`, and check the release page and the one-line install.

## Next (recommendation)

Unchanged from R3: **R4, the runner** (one provider headless behind per-run approval, the bounded
Ralph loop, and the pair-mode review loop). In beginner terms: "Aurora runs the step for you, and
you approve".
