# Skill-Manager Landscape

**Status:** Historical comparison; superseded by [prior-art.md](prior-art.md) and the current
[Rust architecture](../architecture/universal-skill-and-workflow-model.md).
**Accessed:** 2026-10-23
**Method:** a documentation-level review of each project's public README. Neither repository was
cloned or audited line by line, and **no code was copied**. The *Observed* columns report only what
the README states. A blank means the README does not state it, **not** that the project lacks the
feature.

Sources:
- ASM: https://github.com/luongnv89/asm (MIT, stated in the README). Node.js 22+ CLI with optional TUI.
- Skills Manager: https://github.com/xingkongliang/skills-manager (MIT, stated in the README).
  Tauri 2 / Rust / React desktop app plus a CLI. Metadata is stored in SQLite.

Part of the ASM README was extracted as a summary rather than verbatim. Items from that part are
marked †.

## Observed features

| Capability | ASM (README) | Skills Manager (README) |
|---|---|---|
| Positioning | "universal skill manager for AI coding agents"; "a support tool, not a process supervisor" | "manage, sync, and organize AI agent skills across 50+ coding tools" |
| Agents | 21 built-in providers with global and project paths (incl. Claude Code, Codex, Google Antigravity, Gemini CLI) | 56 agents listed (incl. Claude Code, Codex, Antigravity); custom tools can be added |
| Storage model | per-provider directories (global + project); optional local library with `asm activate` per provider | central repo (default `~/.skills-manager`), deployed by symlink or copy; global, project and linked workspaces |
| Inventory / inspect | `asm list --json`, `asm get <skill>` | `skills list/show/status` |
| Search | `asm search` (`--fuzzy`, `--tag`) | marketplace keyword search |
| Install / update / remove | `asm install github:user/repo` or by name; `update`, `outdated`, `uninstall` | install from Git, folder, zip, or skills.sh; `check`, `update --all`, `remove --yes` |
| Validation / security | install "validates frontmatter, scans security"; `asm audit`, `audit overlap`, `audit security`; `asm eval` | — |
| Context / cost analysis | `asm stats --tokens`, `asm audit residency` | — |
| Bundles / presets | `asm bundle install <name>` | named presets ("a one-time copy, not a live sync") |
| Sync across tools | — | multi-tool sync by symlink or copy; project sync in either direction |
| History / backup / rollback | — | Git-based backup and snapshots; restore first snapshots the current state |
| Activity log | — | install, remove, update and sync operations recorded locally |
| Machine-readable output | `--json`, `--machine`, `--yes` | `--json`, structured error envelope, `--dry-run` on some commands |
| Provenance / pinning | `github:owner/repo` sources; registry installs use a "pinned commit"; version, license, compatibility frontmatter † | "source metadata", update tracking for Git skills |
| Workflow / roles | skill dependency acquire/release by the calling agent; no roles mentioned | agents can call the CLI to manage skills; presets are grouping only; no roles mentioned |

## What Aurora takes as inspiration (independent design)

| Idea | Seen in | Aurora decision |
|---|---|---|
| Provider adapters with global and project scopes | both | `SkillStorageAdapter` per agent directory, kept separate from `ExecutionProvider` (ADR-0002) |
| Scriptable CLI with JSON output | both | `--json` on `detect`, `providers`, `workflow show/plan/status` |
| Central canonical library | Skills Manager | `registry/skills.toml` + `skills/`; provenance, hash, license fields planned (model doc §1) |
| Validation and security scanning | ASM | existing strict registry validation; a content security scan is a candidate for a future milestone |
| Bundles / presets | both | a workflow role's `required_skills` / `recommended_skills` acts as a role-scoped bundle; standalone bundles deferred |
| Reversible changes and history | Skills Manager | `aurora-skills::apply` journals operations under `.aurora/ops/` and supports undo; Git-based backup not adopted |
| Context-cost observability | ASM | deferred |
| Pinned revisions | ASM | planned `revision` field |

## Where Aurora's scope differs

Neither README describes **role-based workflows with acceptance gates and evidence**, routing of
roles to different CLIs, or a single-provider override. This is the part of Aurora that this
landscape review did not find documented elsewhere. Even so, because the review covered READMEs
only, this doc makes no claim that the projects lack such features.

## Third-party code and licenses

Both projects are MIT-licensed according to their READMEs. Milestone 1 reuses **no** third-party
code. If code is reused later, the commit must record the source repository, the commit, the license
text, and the attribution in a `THIRD_PARTY_NOTICES` file, and the reuse must be approved in an ADR.
