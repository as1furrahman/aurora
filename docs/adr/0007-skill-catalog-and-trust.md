# ADR-0007: Curated skill catalog, pinned content and the install trust model

**Status:** Accepted (2026-10-24)

## Context

Aurora assigns third-party agent skills to coding CLIs by role and stage. The skill ecosystem is
actively attacked:

- 341 of 2,857 ClawHub skills were malicious;
- skills with bundled scripts are 2.12× more likely to be vulnerable;
- five skill scanners have been bypassed.

Licences vary as well: CC-BY-SA, source-available, or none. Most users run one to three CLIs, and
those CLIs read skills from different, partly shared directories
(`docs/research/agent-skills-landscape-2026-10.md`).

## Decision

1. **A curated catalog, not vendored content.** `catalog/catalog.toml` names each skill by
   repository, **full commit SHA**, path, licence and a **content hash** of the skill directory
   (`sha256`). Aurora ships no third-party skill text. Entries have four kinds: `skill`, `rules`,
   `framework`, `infra`. Aurora installs only skills and rules; frameworks and infrastructure are
   documented, never installed.
2. **Integrity first.** A fetched skill must hash to the catalog pin, or nothing is planned or
   installed. Fetching runs git from an argument list with no config, hooks, credentials or
   prompts, over HTTPS only, with a timeout.
3. **Scanners are signals, not gates.** The static scanner sorts findings into two levels:
   - `Block`, which can never be overridden: symlinks, binaries, bytecode, archives, padding, and
     size limits.
   - `Review`: scripts, hooks, `` !`cmd` ``, broad `allowed-tools`, pipe-to-shell, encoded blobs
     and download links.

   Review findings need a human, but a catalog `reviewed` note covers them for that exact content.
   Community-trusted sources always need the user's `--reviewed`.
4. **Permission-preserving overlays.** The only edits Aurora makes to upstream files remove
   `allowed-tools` (always) and `hooks` (unless `--keep-hooks`). Both bypass a CLI's permission
   prompts. Each edit is line-based and verified by re-parsing.
5. **Project scope, lock-based ownership.** Skills are installed per project:
   - Claude Code: `.claude/skills`
   - Codex and Antigravity: the shared `.agents/skills`
   - Hermes: `.hermes/skills`

   `aurora.lock` records each installed copy and its hash. Aurora changes or removes only copies
   the lock records whose content is unchanged.
6. **Journaled, non-destructive apply.** A write-ahead journal under `.aurora/ops/<id>/` records
   every step, and each step re-checks its precondition. Failures roll back. Nothing is deleted:
   replaced and removed copies are moved into the operation's backup. Undo restores only what is
   unchanged since.
7. **Any CLI, any role.** Every preset role allows all four CLIs. CLI profiles (`solo`, `pair`,
   `trio`) map roles to CLI slots, and handoffs name each stage's skills in the CLI's invocation
   syntax.

## Consequences

- Updating a catalog entry means a new pin. The entry needs a new SHA, hash, measured description
  length and, where its files need review, a new review note.
- The shared `.agents/skills` directory cannot be split by role. The plan reports what each CLI
  will load and any cross-CLI leaks, rather than pretending to separate them.
- Global (user-level) installs are out of scope. The boundary policy blocks the real global agent
  directories.
- Projects gain `aurora.lock` (commit it) and `.aurora/` (local state; ignore it).
