# Provider and Skill-Storage Adapter Contract

**Status:** implemented in the Rust workspace. The provider facts below are a dated research
snapshot; see [universal-skill-and-workflow-model.md](universal-skill-and-workflow-model.md) for
the current crate ownership map. ADR-0002 records why the two concepts stay separate.

Aurora uses **two independent contracts** for each agent. The fact that a skill-storage adapter
exists for an agent says nothing about whether Aurora can execute that agent, and Aurora's code
never assumes it does.

## 1. Skill-storage adapters

Rust represents adapter compatibility as stable identifiers in
`aurora_core::providers::ProviderSpec::skill_adapters`. The skill planner maps providers to
project-local skill directories in `aurora_skills::plan::skill_dir`; there is no runtime adapter
trait, global skill inventory, or user-level directory mutation.

| Provider | Adapter identifiers | Project skill directory |
|---|---|---|
| `claude` | `claude-code` | `.claude/skills/` |
| `codex` | `codex-cli` | `.agents/skills/` |
| `antigravity` | `antigravity-cli`, `antigravity-workspace` | `.agents/skills/` |
| `hermes` | `hermes` | `.hermes/skills/` |

The shared `.agents/skills/` directory is visible to both Codex and Antigravity. Hermes can also
read that directory in trusted repositories. Skill parsing, scanning, planning, applying, and undo
are handled by `aurora-skills` rather than provider-specific adapters.

The provider skill locations described below are vendor documentation, not Aurora's scan scope.
Aurora only installs into the project directories in the table and does not inventory global
skill directories.

## 2. Execution providers

Provider specifications live in `aurora_core::providers::ProviderSpec`. They are static metadata;
`aurora_app::tools` checks whether each provider binary is present on `PATH` without launching it.
Workflow handoffs are rendered by `aurora-core::workflow`. Aurora does not execute provider CLIs.

The spec records `id`, `display_name`, `binary`, `skill_adapters`, `skill_invocation`, `run_hint`,
`documented_headless`, `invocation_modes`, `headless_enabled`, capability `enforcement`,
`forbidden_flags`, `docs_urls`, and `verified_on`. All supported providers use `manual-handoff`;
`headless_enabled` is false. Permission-bypass flags are never emitted in handoffs.

### Responsibilities a future headless implementation must cover

These items remain future design requirements and are **not implemented**:

- **Invocation:** an argument vector only, never a shell string. The prompt goes in through stdin
  or a file, not interpolated into a command line.
- **Working directory:** an explicit `cwd`, validated with `aurora_fs::boundaries::validate_security_preconditions`
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

## 3. Per-provider facts (research snapshot from 2026-10-23)

Sources are cited with the access date. "Local" means checked on the audit host. These facts inform
provider metadata and future work; they are not instructions to launch a provider. The Hermes
provider was added later and is described in [workflow-schema.md](workflow-schema.md#76-hermes-provider).

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
- **Research note:** the documented Codex global skill paths differ from paths observed by the
  original prototype. The Rust product does not inspect either set of global directories; its
  project install location is `.agents/skills/`.
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
- The Rust project install paths are listed in §1. The global paths above are documented for
  provider context only and are not scanned or modified by Aurora.
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
