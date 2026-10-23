# Provider and Skill-Storage Adapter Contract

**Status:** accepted for Milestone 1 (2026-10-23). See ADR-0002 for the decision to split the two
contracts.

Aurora uses **two independent contracts** for each agent. The fact that a skill-storage adapter
exists for an agent says nothing about whether Aurora can execute that agent, and Aurora's code
never assumes it does.

## 1. Skill-storage adapter: `awb.adapters.SkillStorageAdapter`

This is an alias of `awb.adapters.base.BaseAdapter`. Every method must be non-mutating with respect
to real agent directories.

| Member | Contract |
|---|---|
| `name` | Stable adapter id (`claude-code`, `codex-cli`, `antigravity-cli`, `antigravity-workspace`, `antigravity-ide`) |
| `display_name` | Human-readable label |
| `target_type` | `cli`, `workspace` or `ide` |
| `discovery_path` | The directory this adapter reads skills from |
| `is_detected()` | Read-only probe (binary on `PATH` and/or directory present). Never spawns a process. |
| `list_installed_skills()` | Read-only listing. Rejects hidden entries, symlinks, `.system`, and entries without a regular-file `SKILL.md`. |
| `validate_conformance(sandbox)` | Structural self-test. Writes **only** inside the supplied sandbox. |

Agent-specific paths, formats and exclusions live **only** inside adapter classes. Shared install and
remove logic (`awb.operations`, `awb.transactions`, `awb.safety`) is never duplicated per agent.

## 2. Execution provider: `awb.providers.ExecutionProvider`

| Member | Contract |
|---|---|
| `spec` | A frozen `ProviderSpec` (below) |
| `detect()` | `shutil.which(binary)` only, returning a `ProviderDetection`. It never runs the binary, so it reports no version in Milestone 1. |
| `build_handoff(...)` | Returns the role prompt text for manual mode. Pure function, no I/O. |
| `execute(...)` | **Raises `ProviderNotEnabledError` in Milestone 1** (ADR-0005). |

`ProviderSpec` fields: `id`, `display_name`, `binary`, `skill_adapters` (adapter ids whose skills
the CLI loads), `invocation_modes` (Milestone 1: `("manual-handoff",)`), `headless_enabled`
(`False`), `documented_headless` (a short description of the documented non-interactive form, for
future use only), `forbidden_flags` (permission-bypass flags Aurora must never emit),
`docs_urls`, `verified_on`.

### Responsibilities a future headless implementation must cover

These items are specified now and **not implemented**:

- **Invocation:** an argument vector only, never a shell string. The prompt goes in through stdin
  or a file, not interpolated into a command line.
- **Working directory:** an explicit `cwd`, validated with `validate_security_preconditions`
  against the run's allowed roots.
- **Environment:** an explicit allow-list. Credentials are inherited from the user's own
  configuration and never written by Aurora.
- **Output capture:** stdout and stderr go to files under the run directory. Structured output is
  used where the CLI documents it.
- **Status:** the exit code is recorded but is **never** treated as stage acceptance.
- **Timeout and cancellation:** a wall-clock limit from the stage definition. Cancellation sends
  SIGTERM, then SIGKILL after a grace period. Both are recorded.
- **Permissions:** run the CLI under its default or most restrictive documented mode. Flags in
  `forbidden_flags` are rejected at construction time unless a per-run, per-flag user authorization
  is recorded.
- **Failure classification:** one of `not-installed`, `invocation-error`, `timeout`, `cancelled`,
  `nonzero-exit`, `output-missing`. Only declared fallback rules may react to these.

## 3. Per-provider facts (verified 2026-10-23)

Sources are cited with the access date. "Local" means checked on the audit host. These facts inform
the specs and future work. **Milestone 1 uses none of the invocation flags.**

### Claude Code (`claude`)
- Local: `claude --version` → `2.1.309 (Claude Code)`. `claude --help` lists `-p/--print`,
  `--output-format`, `--permission-mode`, `--add-dir`, `--bare`, `--dangerously-skip-permissions`,
  `--allow-dangerously-skip-permissions`.
- Non-interactive: `claude -p "<prompt>"`, which also reads stdin. `--output-format text|json|stream-json`.
  `--max-turns`, `--resume <id>`, `--no-session-persistence`. Exit code 0 on success, non-zero on
  failure, 143 on SIGTERM. No cwd flag is documented, so the process inherits its working directory.
  — https://code.claude.com/docs/en/headless, https://code.claude.com/docs/en/cli-reference
- Permissions: `--permission-mode default|acceptEdits|plan|auto|dontAsk|bypassPermissions|manual`.
  `--dangerously-skip-permissions` is equivalent to `bypassPermissions` ("Only use this mode in
  isolated environments"). — https://code.claude.com/docs/en/permissions
- Skills: `SKILL.md`. Personal `~/.claude/skills/<name>/`, project `.claude/skills/<name>/`, also
  nested and plugin locations. `description` is recommended; `name` defaults to the directory
  name. — https://code.claude.com/docs/en/skills
- **Forbidden flags:** `--dangerously-skip-permissions`, `--allow-dangerously-skip-permissions`,
  `--permission-mode bypassPermissions`.

### OpenAI Codex CLI (`codex`)
- Local: not installed on the audit host.
- Non-interactive: `codex exec "<prompt>"`, or the prompt from stdin / `codex exec -`. `--json`
  gives NDJSON events. `-o/--output-last-message <path>`. `-C/--cd <path>` sets the workspace root.
  The default sandbox is read-only. Numeric exit codes are not documented.
  — https://developers.openai.com/codex/noninteractive (308 → https://learn.chatgpt.com/docs/non-interactive-mode),
  https://developers.openai.com/codex/cli/reference (308 → https://learn.chatgpt.com/docs/developer-commands?surface=cli)
- Permissions: `--sandbox read-only|workspace-write|danger-full-access`, `--ask-for-approval
  on-request|never`. `--dangerously-bypass-approvals-and-sandbox` (alias `--yolo`) is described as
  "Dangerous—only use inside an isolated runner". `--full-auto` is no longer supported.
- Skills: `SKILL.md`, with required frontmatter `name` and `description`. Locations: `.agents/skills`
  in the CWD, the parent directory and the repo root, plus `$HOME/.agents/skills` (user),
  `/etc/codex/skills` (admin), and bundled system skills.
  — https://developers.openai.com/codex/skills (308 → https://learn.chatgpt.com/docs/build-skills)
- **Discrepancy:** the existing `codex-cli` adapter reads `~/.codex/skills`, which is not among the
  locations documented on that page. The adapter is left unchanged in Milestone 1 (its tests and
  `.system` protections belong to earlier phases). `~/.agents/skills` is added to the global block
  list. Reconciling the Codex discovery path is listed as next-milestone work.
- **Forbidden flags:** `--dangerously-bypass-approvals-and-sandbox`, `--yolo`,
  `--sandbox danger-full-access`, `--ask-for-approval never`.

### Google Antigravity CLI (`antigravity`, binary `agy`)
- Local: `agy` is not installed on the audit host. `gemini` 0.63.0 is installed, but it is the Gemini
  CLI, a different product that the Antigravity CLI replaced on 2026-06-18 for unpaid and Google One
  tiers. — https://developers.googleblog.com/an-important-update-transitioning-gemini-cli-to-antigravity-cli/
- Version: `agy --version`. — https://antigravity.google/docs/getting-started?tab=cli
- Non-interactive: `agy -p "<prompt>"` (`--print`/`--prompt`), `--output-format text|json|stream-json`,
  `--print-timeout` (the docs disagree on its default: 5m on one page, 0s on another), `--add-dir`.
  No cwd flag is documented. Exit code 0 on success. In headless mode, tools that need approval are
  soft-denied with exit code 0. — https://antigravity.google/docs/cli/headless/,
  https://antigravity.google/docs/cli/reference/
- Permissions: `--mode accept-edits|auto|plan`, allow/deny/ask rules in
  `~/.gemini/antigravity-cli/settings.json`, and `--sandbox`. `--dangerously-skip-permissions`
  auto-approves all tool requests. — https://antigravity.google/docs/cli/commands/permissions/,
  https://antigravity.google/docs/sandbox/
- Skills: `SKILL.md`, with `description` required and `name` optional. CLI global:
  `~/.gemini/antigravity-cli/skills/`. Workspace (all surfaces): `<root>/.agents/skills/` (legacy
  `.agent/skills`). App/IDE global: `~/.gemini/config/skills/`. — https://antigravity.google/docs/skills
- These confirm the existing `antigravity-cli`, `antigravity-workspace` and `antigravity-ide` adapter
  paths.
- **Forbidden flags:** `--dangerously-skip-permissions`.

### Shared directory hazard
`<repo>/.agents/skills` is a **workspace** skill directory for Antigravity *and* a **repo-scope**
skill directory for Codex. Installing into it affects both agents. A future project-scope install
must report every agent that reads the target directory.

## 4. Compatibility rule

A stage's required skill is compatible with provider P when at least one of P's
`skill_adapters` appears in that skill's `compatible_agents`. Otherwise the plan reports an
incompatibility, naming the skill, the provider and the adapters it checked. Aurora never converts a
skill to another agent's format.
