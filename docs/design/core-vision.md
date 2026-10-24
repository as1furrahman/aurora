# Skill Core Vision (recorded for a later milestone; not built in R1)

## Canonical skill

- Agent Skills spec, using only the six portable fields: `name` (matches the directory),
  `description`, `license`, `compatibility`, `metadata`, `allowed-tools`.
- Aurora's own tags go under `metadata`:
  - `aurora-roles: "implementer tester"`
  - `aurora-category: testing`
- **Per-agent overlays** are generated at install time. Examples:
  - Claude fields (`context`, `disallowed-tools`, `when_to_use`);
  - Codex `agents/openai.yaml`;
  - OpenCode keys under `metadata`.

  This follows rulesync's per-target blocks plus a `targets:` filter. The canonical copy never
  carries agent-specific keys, because the Claude Skills API rejects them.
- The parser is copied from openai/codex `skills/src/parser.rs` (Apache-2.0, NOTICE). It repairs
  malformed scalar YAML, and uses the serde-saphyr backend.
- **Validation runs in two layers.**
  - Spec lint: name rules, sizes, relative links one level deep.
  - Per-agent lint: description budget against each agent's cap (Claude listing about 1,536
    characters, Codex skills list about 2% of context or 8k characters), dangling links,
    executable bits on `scripts/`.

## Where skills go: the leakage matrix

- The agent path table is data (`agents.toml`), seeded from vercel-labs/skills `docs/agents.md`
  (MIT, attributed) and corrected against the vendor docs (see `../references/prior-art.md` §3).
- Aurora computes **who reads what** for every target directory. If installing skill S into
  directory D for role R on CLI X would also expose S to CLIs Y and Z, Aurora warns.
- Aurora prefers agent-exclusive directories: `.claude/skills`, `.gemini/skills`,
  `.opencode/skills`, `.cursor/skills`, `.github/skills`, `~/.gemini/antigravity-cli/skills`.
- Codex has no exclusive project directory. Per-role skill sets are projected through **native
  hooks** instead:
  - Claude subagent `skills:` lists;
  - Codex `.codex/agents/*.toml` with `[[skills.config]] enabled=false`;
  - OpenCode `permission.skill`.

## Installs with safeguards (owner decision 2026-10-23)

Every install follows this sequence:

1. dry-run diff;
2. explicit confirmation (`--yes` is required when not on a TTY);
3. pre-operation snapshot / journal entry;
4. transactional apply, using the existing `awb.transactions` design;
5. `aurora undo <op-id>`.

Ownership comes only from a machine-local **ledger** (`$XDG_STATE_HOME/aurora/ledger.json`), never
from directory contents. Remove and rollback only touch entries Aurora created whose hash still
matches. Drifted entries are reported, never deleted.

Never touched:

- vendor `.system` folders under both `~/.codex/skills` and `~/.agents/skills`;
- plugin caches (`~/.claude/plugins`, `~/.gemini/antigravity-cli/plugins`);
- managed-settings directories;
- unmanaged ("foreign") skills.

Install states follow skillm: linked / copy / drift / foreign / broken. `aurora doctor --fix
--dry-run` reconciles them.

## Manifest and lock

- `aurora.toml` (declared) + `aurora.lock` (generated, sorted, **no timestamps**, committed for
  project scope).
- Content hash = SHA-256 over sorted relative paths plus bytes, excluding `.git` and
  `node_modules` (the Vercel `local-lock` approach).
- `$HOME` is kept unexpanded in the lock (aps).
- `[[deployment]]` rows record agent, scope, target, method (`symlink | copy | rendered`),
  `rendered_hash` and role.

## Library, bundles, roles

- **Library → activate / deactivate** per CLI and scope (asm). Disable instead of delete.
- **Role bundles:** a role's `required_skills` / `recommended_skills` are a bundle. Assigning a
  role to a CLI activates exactly that bundle for that CLI, through exclusive directories or
  native hooks.
- **Sources:** local, git (pinned rev), archive. Limits on size and file count (Vercel: 10 MiB
  download, 25 MiB extracted, 1000 files).
- **Security scan** before install: shell, network, credential and base64 patterns, with SARIF
  output. Never run install scripts.

## Port from Python

`awb.safety` (boundaries, no-follow descriptor reads, locking), `awb.transactions` (journal,
rollback, recovery) and `awb.operations` (ownership manifest, drift checks, pruning) are ported
with their tests (SEC-01…SEC-11) before any live install is enabled.

## Future crates

- `aurora-skills`: parser, validation, registry.
- `aurora-proc`: spawning, process groups, timeouts.
- `aurora-providers`: per-CLI adapters → `RunEvent`.
- `aurora-acp`: behind a feature flag.
- `aurora-git`: worktrees via the git binary.
- `aurora-runner`: budgets, stuck detection, merge queue.
- `aurora-tui`.
