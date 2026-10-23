# Aurora R2 report: curated skill catalog and safe installs

**Branch:** `feat/rust-workflow-engine` (dev container working copy `/opt/aurora`, pushed to the
host repository's branch).

**Dates:** 2026-10-24.

**Owner's choice:** "skill catalog + safe installs, then TUI".

## Outcome

Aurora can now do six things it couldn't before R2:

1. Recommend popular, maintained skills per workflow stage for the CLIs a user actually has
   (`solo`, `pair`, `trio`).
2. Fetch those skills from GitHub at pinned commits and verify their content.
3. Scan the skills for risk and gate installation on human review where it's needed.
4. Install the skills into a project for each CLI, with permission-bypassing frontmatter removed.
5. Undo any install without deleting anything.
6. Name the installed skills in each stage's handoff, in that CLI's syntax.

The real CLIs load what Aurora installs. Codex shows exactly its role's skills in its prompt
input, and Hermes lists all of them once the repository is trusted. Both were checked offline,
without logins.

## Steps

| Step | Commit | What |
|---|---|---|
| research | `4d44245` | `docs/research/agent-skills-landscape-2026-10.md`: general vs stage skills, the four entry kinds, conflicts, CLI usage, supply-chain risk, context budgets |
| 0 | `d1699cf` | `catalog/catalog.toml` (41 entries, 17 pinned repositories), `aurora catalog list/show/recommend`, solo/pair/trio profiles |
| 1 | `7407c37` | `aurora-skills` crate: SKILL.md parser (Codex's prose repair, Apache-2.0, attributed), Agent Skills spec checks, per-CLI portability, listing cost; `aurora skills inspect` |
| 2a | `495da72` | no-follow collection, timestamp-free content hash, static scanner; `aurora skills scan` |
| 2b | `a642297` | hardened git fetch at pinned SHAs, content-addressed cache, mandatory `sha256` pins, `reviewed` notes, readiness; `aurora skills fetch` |
| 3a | `efab459` | overlays: remove `allowed-tools` (always) and `hooks` (unless kept), verified by re-parse |
| 3b | `d7abad6` | pure install plan, lock-based ownership, visibility and leak matrix, rules blocks; `aurora skills plan` |
| 4a | `b5ac4c8` | `aurora_fs::dirs`: fd-relative, `O_NOFOLLOW`, `RENAME_NOREPLACE` tree primitives |
| 4b | `6f4c43a` | journaled apply and non-destructive undo |
| 4c | `5c0c916` | `aurora skills apply` / `undo` with confirmation and the review gate |
| 5 | `5ab1758` | `workflow plan/start --profile/--clis/--with`, stage skills in handoffs, any CLI for any preset role, incompatibility fix hint |
| 6 | (this commit) | `examples/skills-install-demo` (21 checks); ADR-0007; this report |

## Verification

**Automated checks:**

- **Rust:** 336 tests. `cargo clippy -D warnings`, `cargo fmt --check` and `cargo deny check`
  are clean.
- **Python reference suite:** 282 tests.
- **Rust-vs-Python parity:** CLI 18/18, runs 18/18. Run parity was re-run after the Hermes commit,
  which R1 had left unchecked.

**Mutation checks.** Each of these deliberate breakages was caught by a test, apart from one the
compiler rejected:

- scanner rules: pipe-to-shell, padding, symlinks, broad `allowed-tools`;
- the plan's ownership rules: drift, foreign, leaks, prune (the compiler's exhaustive match
  rejects dropping the foreign rule);
- apply/undo guarantees: user edits, pre-step re-check, rollback, crash.

**Real-world corpus.** All 285 SKILL.md files in the 11 catalogued repositories were parsed, with
0 repairs needed. They were scanned with full trees: 186 clean, 97 needing review, 2 blocked (by
policy: 5+ MiB of fonts, and a `.tar.gz`). The overlay was applied to 290 files: 61 changed,
0 refused.

**Live fetch.** All 35 installable entries were fetched from GitHub at their pins in 68 s. Every
hash matched the full-clone measurement, and tampering with a cached copy is refused.

**Demos:**

- `examples/real-bugfix-demo`: 25/25 checks on python-tabulate #241. The preset needs no edit,
  the pair profile routes the run, and the incompatibility block shows its fix.
- `examples/skills-install-demo`: 21/21 checks covering plan, review gate, apply, the Codex prompt
  input, `hermes skills list`, handoffs, and undo back to a fresh clone.
- The host agent directories were snapshotted before and after: unchanged. No cache was written
  to the host home.

## Decisions made on the evidence

- **`planning-with-files` moved from baseline to opt-in.** It declares five session hooks and
  16 scripts.
- **Trail of Bits `insecure-defaults` is not in the catalog.** It does not exist at HEAD;
  `sharp-edges` is used instead.
- **Pipe-to-shell is Review, not Block.** rustup/uv installers and security skills quoting
  attacks use it. Markdown tables and target-less prose no longer match.
- **`` !`cmd` `` detection follows Claude Code's documented form.** Code spans that end in `!`,
  like `` `ensure!` ``, are prose.
- **The review gate judges the installed copy.** Findings an overlay removes don't need review;
  integrity is still checked on the upstream copy.
- **No Codex manual-only overlay.** Every manual-only catalog skill already ships
  `allow_implicit_invocation: false`.
- **Lock records are keyed by destination.** One record per installed copy keeps refused updates
  consistent.

## Known limitations

1. **Project scope only.** Global installs into `~/.claude/skills` and the like are blocked by
   the boundary policy, by design.
2. **Claude Code loading is not verified live.** It has no offline listing, so the copies are
   checked with the spec checker. A live check needs a logged-in session (R4).
3. **Antigravity details are assumptions.** The skill-invocation syntax is undocumented, so
   handoffs say "ask for it by name". The listing budget is assumed equal to Codex's.
4. **`.agents/skills` is shared** by Codex, Antigravity, Hermes (in trusted repos) and other
   tools. It is reported, but it can't be split by role.
5. **Hermes builtin skills shadow catalog names** (`systematic-debugging`,
   `test-driven-development`). Project copies win, but Aurora does not report the shadowing yet.
6. **Reviews are not remembered.** `--reviewed` acknowledgements apply per run of `apply`; they
   are not recorded in the lock.
7. **No garbage collection** for the fetch cache or for `.aurora/ops/` backups.
8. **Two skill systems coexist.** One is the legacy registry (`registry/skills.toml`, Aurora's own
   skills, shared with the Python oracle); the other is the catalog. Unifying them waits until
   the Python reference is retired.
9. **Profile runs are Rust-only.** Presets already were in R1 (`requires`, `verdict`), and Hermes
   in their eligible lists is one more Rust-only element.
10. **The scanner is heuristic.** A clean scan is a signal; the catalog's review notes and the
    user's review remain the real gate.
11. **Accepted (owner, 2026-10-24):** the frozen Python `ProcessLock` follows a symlinked
    `.awb.lock` and truncates its target. It is low severity: it affects only the test oracle,
    needs a planted symlink in a user-owned directory, and can only truncate a user-owned file.
    Rust refuses the case. It is left as is.

## Next: R3 TUI

The R2 commands are the TUI's data layer. All of them have `--json` output, and the plan, the
visibility matrix and the readiness checks are pure functions. Proposed first screens:

- **Skills × CLIs matrix.** It shows the plan's visibility, budget bars, leaks and Hermes
  shadowing.
- **Roles board.** One key allows a CLI for a role, or switches the profile.
- **Install review.** It walks through flagged files before `apply`, then records the review.
- **Run pipeline.** It includes a checkbox submit form for acceptance criteria.
