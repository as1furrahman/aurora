# Prior Art and Reuse Map

**Researched:** 2026-10-23, by six parallel research passes over the web, GitHub source, crates.io
and official docs.

**Supersedes:** [`skill-manager-landscape.md`](skill-manager-landscape.md), which covered only
the asm and skills-manager READMEs. That doc is kept for history.

**Rule:** features listed for another project are what its README or source showed on that date.
A missing feature is never claimed. Items marked (?) were not verified. Star counts are
approximate, from the GitHub API on that date.

## 1. Coding-agent CLI orchestrators

| Project | Stack / licence | How agents run | Roles and profiles | Completion signal | Permission default | Status |
|---|---|---|---|---|---|---|
| [AWS CLI Agent Orchestrator](https://github.com/awslabs/cli-agent-orchestrator) (CAO) | Python / Apache-2.0 | tmux sessions + MCP `handoff`/`assign`/`send_message` | Markdown + YAML profiles: `role` ≠ `provider`, `allowedTools`, delegation ceilings | Regex over screen output, per provider | bypass flags | active |
| [Vibe Kanban](https://github.com/BloopAI/vibe-kanban) | Rust + web / Apache-2.0 | Claude `-p --output-format stream-json --permission-prompt-tool=stdio`; `codex app-server`; Gemini via ACP | none | protocol events | configurable approvals | **sunsetting** |
| [Agent Client Protocol](https://github.com/agentclientprotocol/agent-client-protocol) + [Rust SDK](https://github.com/agentclientprotocol/rust-sdk) | Apache-2.0, crate 3.5.0 | stdio JSON-RPC: `session/prompt`, `session/update`, `session/request_permission` | n/a | `stopReason` | permission requests go to the client | active, fast-moving majors |
| [Bernstein](https://github.com/sipyourdrink-ltd/bernstein) | Python / Apache-2.0 | CLI adapters | roles per task | **"janitor" checks files, globs, tests and content; never trusts agent claims** | policy engine | active |
| [metaswarm](https://github.com/dsifry/metaswarm) | shell / MIT | `codex exec --json`, gemini | 18 personas | the orchestrator re-validates | reviewer is read-only; **always a different model** | active |
| [codex-plugin-cc](https://github.com/openai/codex-plugin-cc) | JS / Apache-2.0 | Codex app server, read-only | review / adversarial review | Stop-hook gate | read-only | active |
| [Gas Town](https://github.com/gastownhall/gastown) | Go / MIT | tmux + hooks | Mayor / Polecat / Witness / Refinery | `gt done` + Witness stuck detection | per-CLI flags | active |
| [ccmanager](https://github.com/kbwo/ccmanager) | TS / MIT | PTY | presets | per-CLI state detector | optional LLM auto-approve | active |
| [Just Every Code](https://github.com/just-every/code) | Rust (codex fork) / Apache-2.0 | built in + `[[agents]]` TOML | agent entries | n/a | read-only flag | active |
| [Goose](https://github.com/aaif-goose/goose) | Rust / Apache-2.0 | ACP providers (CLI providers deprecated) | recipes + sub-recipes | `max_turns`, `RetryConfig.checks` | mode → permission-mode table | active |
| [claude-squad](https://github.com/smtg-ai/claude-squad) | Go / **AGPL-3.0** | tmux + worktree | command profiles | none | `--autoyes` | active |
| [coder/agentapi](https://github.com/coder/agentapi) | Go / MIT | terminal emulator scraping | none | stable / running | n/a | **archived** |
| [Roo Code Boomerang](https://github.com/RooCodeInc/Roo-Code) | TS / Apache-2.0 | `new_task(mode)` → `attempt_completion(summary)` | modes | summary only | n/a | **archived** |

Lessons:

- **Screen-scraping breaks.** agentapi is archived, and CAO keeps per-version regexes.
- **CLI flags drift.** Codex removed `--full-auto`, and Gemini renamed `--experimental-acp` to
  `--acp`.
- **Most tools default to permission-bypass flags.** Aurora will not.

## 2. Skill, rule and config managers

| Project | Stack / licence | Storage | Lock / manifest | Notable |
|---|---|---|---|---|
| [vercel-labs/skills](https://github.com/vercel-labs/skills) (`npx skills`) | TS / MIT | canonical `.agents/skills` + relative symlinks, copy fallback | `skills-lock.json` (timestamp-free content hash) | **best-maintained 67-agent path table** ([docs/agents.md](https://github.com/vercel-labs/skills/blob/main/docs/agents.md)); download size limits |
| [luongnv89/asm](https://github.com/luongnv89/asm) | TS (Ink) / MIT | provider dirs + library/activate | registry pinned to commit | the TUI we model; security audit; bundles. **Path table is wrong for Cursor/Copilot (rule dirs).** |
| [xingkongliang/skills-manager](https://github.com/xingkongliang/skills-manager) | Rust + Tauri / MIT | central `~/.skills-manager`, SQLite | per-skill meta + content hash | `path_guard.rs`, `content_hash.rs`, `TARGET_CONFLICT` refusal |
| [skillm](https://lib.rs/crates/skillm) | Rust / MIT | hub + symlinks | `config.json` with `content_hash` | install states **linked / copy / drift / foreign / broken**; system skills ignored |
| [803/skills-supply](https://github.com/803/skills-supply) | MIT | git copy, local symlink | `agents.toml` + `.sk-state.json` per agent root | ownership: removes only what it installed, errors on foreign |
| [intellectronica/ruler](https://github.com/intellectronica/ruler) | TS / MIT | `.ruler/` copied out | `ruler.toml` | `.bak` backups, `ruler revert`, `--dry-run` |
| [dyoshikawa/rulesync](https://github.com/dyoshikawa/rulesync) | TS / MIT | `.rulesync/` → generated files | — | **per-tool frontmatter blocks + `targets:` filter** |
| [westonplatter/aps](https://docs.rs/aps) | Rust / BSD-3 | symlink or copy | `aps.lock.yaml` (sha256, `$HOME` kept unexpanded) | backups, `validate --strict` |
| [skref](https://docs.rs/crate/skref/latest) | Rust / Apache-2.0 | — | — | Agent Skills spec validator, `--allow-claude-fields` |
| [agentskills/skills-ref](https://github.com/agentskills/agentskills) | Python / Apache-2.0 | — | — | reference validator (demonstration only) |

## 3. Per-agent skill directories

Checked against official docs. The Vercel table (MIT) is the base, corrected where the vendor docs
differ.

| Agent | Global | Project | Format notes | Source |
|---|---|---|---|---|
| Claude Code | `~/.claude/skills/` | `.claude/skills/` (walks up; nested dirs load lazily) | **does not read `.agents/skills`**; extra frontmatter fields are ignored by the CLI but rejected by the Skills API | https://code.claude.com/docs/en/skills |
| Codex | `~/.agents/skills/` | `.agents/skills/` (cwd, parents, repo root) | `/etc/codex/skills` (admin), bundled `.system`; `agents/openai.yaml` | https://learn.chatgpt.com/docs/build-skills |
| Antigravity CLI (`agy`) | `~/.gemini/antigravity-cli/skills/` | `.agents/skills/` (legacy `.agent/skills`) | `name` optional | https://antigravity.google/docs/skills |
| Antigravity IDE | `~/.gemini/config/skills/` | `.agents/skills/` | — | same |
| Gemini CLI | `~/.gemini/skills/`, `~/.agents/skills/` | `.gemini/skills/`, `.agents/skills/` | `.agents` wins within a tier | https://geminicli.com/docs/cli/skills/ |
| GitHub Copilot | `~/.copilot/skills/`, `~/.agents/skills/` | `.github/skills/`, `.claude/skills/`, `.agents/skills/` | varies by surface | https://docs.github.com/en/copilot/concepts/agents/about-agent-skills |
| OpenCode | `~/.config/opencode/skills/` | `.opencode/skills/` | also reads `.claude/skills` and `.agents/skills`; per-agent `permission.skill` | https://opencode.ai/docs/skills/ |
| Cursor | `~/.cursor/skills/`, `~/.agents/skills/` | `.cursor/skills/`, `.agents/skills/` | also reads `.claude/skills` and `.codex/skills` | https://cursor.com/docs/context/skills |

Known disagreements:

- **Codex.** Several tools and Aurora's retired prototype used `~/.codex/skills`; the official
  docs say `~/.agents/skills`. Aurora's Rust installer uses the project-local `.agents/skills/`
  directory and does not enumerate either global path (ADR-0007).
- **Antigravity.** asm and skills-manager use stale paths.
- **Shared directories.** `.agents/skills` is read by about six CLIs, and `.claude/skills` by
  Claude, Copilot, Cursor and OpenCode. **Codex has no exclusive project skill directory.**

## 4. Methodologies (detail in the research summary in the design docs)

| Source | What Aurora adopts |
|---|---|
| Spec Kit, Kiro | ambiguity markers as a hard gate; Kiro's "unchanged behaviour" section for bug fixes; Quick mode for small changes |
| BMAD, Stage-Gate | gate outcome enum: pass / concerns / fail / waived / kill |
| V-model, BMAD trace | traceability: each criterion maps to tests and evidence |
| Superpowers | two-stage review (spec compliance, then quality); verification before completion |
| Scrum DoD, RACI | Definition-of-Done checklist; exactly one accountable role per stage |
| Self-preference bias research ([2404.13076](https://arxiv.org/abs/2404.13076), [2310.01798](https://arxiv.org/abs/2310.01798)) | the reviewer runs on a different provider by default; the reviewer gets the spec and diff, not the implementer's summary |
| Anthropic multi-agent post | about 15× tokens for multi-agent work; explicit objectives and boundaries; outputs written to files |

## 5. Reuse map

| Aurora component | Reference | Reuse mode | Obligation |
|---|---|---|---|
| SKILL.md frontmatter parse, including YAML repair | openai/codex `codex-rs/skills/src/parser.rs` | **copy and modify** (swap `serde_yaml` for serde-saphyr) | Apache-2.0: LICENSE, codex NOTICE, modification header |
| Skill roots and loading | codex `skills/src/loading.rs`, `ext/skills/src/host_roots.rs` | mirror the pattern | — |
| AGENTS.md discovery | codex `core/src/agents_md.rs` | mirror the algorithm | — |
| Process group, kill tree, PDEATHSIG | codex `utils/pty/src/process_group.rs`; `process-wrap` | copy + dependency | Apache NOTICE; process-wrap MIT/Apache |
| Run event schema | codex `exec/src/exec_events.rs` | mirror | — |
| Provider trait, spawn/cancel | vibe-kanban `crates/executors` | mirror the pattern (project is sunsetting) | Apache attribution if copied |
| Agent profiles, enforcement-strength table | CAO docs | spec inspiration | — |
| Evidence "janitor" signals | Bernstein | spec inspiration | — |
| Retry and success checks | goose `RetryConfig` / `SuccessCheck` | schema inspiration | — |
| Permission mode mapping | goose ACP providers table | data inspiration, **bypass rows dropped** | — |
| ACP transport | `agent-client-protocol` 3.x | optional dependency, pinned exactly | Apache-2.0 |
| Agent path table | vercel-labs/skills `docs/agents.md` | **data, with attribution**, corrected | MIT notice |
| Path guard, content hash | skills-manager `path_guard.rs`, `content_hash.rs` | pattern or small copy | MIT notice |
| Install states | skillm | pattern | — |
| Lock and ledger | Vercel lock + aps lock + skills-supply state | design | — |
| TUI structure | gitui (`components`, `cmdbar`, `popup_stack`) + serie (`keybind.rs`, `view/*`) | pattern, small copies | MIT notice |
| TUI tests | ratatui `TestBackend` + insta | dev-dependency | — |

**Do not copy code from:**

- claude-squad (AGPL-3.0)
- oh-my-openagent (SUL-1.0, source-available)
- Task Master (MIT + Commons Clause)
- Claude Code Agent Farm (MIT with rider)
- PAL MCP server (licence unclear)
- Tessl (unclear)
- claude-code-templates catalog content (upstream licences vary)

## 6. Gaps Aurora owns

1. **Role-based assignment.** No tool maps roles to skill sets per CLI. Presets, bundles and
   groups are topical, not role-based.
2. **A who-reads-what leakage matrix.** Because directories are shared, per-CLI skill sets need a
   computed matrix plus native per-agent hooks:
   - Claude subagent `skills:`;
   - Codex `.codex/agents/*.toml` with `[[skills.config]] enabled=false`;
   - OpenCode `permission.skill`.
3. **Manual handoff as a first-class mode.** Every tool above automates.
4. **Per-criterion evidence gates and mechanically enforced reviewer independence,** with an audit
   trail.
5. **Provider selection that respects enforcement strength.** CAO documents how strongly each CLI
   enforces restrictions but does not block mismatches.
6. **A native single-binary Rust orchestrator.** Vibe Kanban, the only serious Rust one, is
   sunsetting.
7. **Explicit protection for vendor and system directories,** plus transactional multi-agent apply
   with journalled undo.

## 7. Open question (decided in the runner milestone)

Two options for headless execution:

- **ACP first:** a uniform `request_permission` and structured events. Needs Node adapters for
  Claude and Codex. `agy` support is unconfirmed.
- **Native structured modes first:** `claude -p --output-format stream-json`, `codex exec --json`,
  `agy -p --output-format stream-json`. ACP would sit behind a cargo feature.

Both normalise into one `RunEvent` stream. Neither uses screen-scraping or bypass flags.
