# Skills install demo: the real CLIs load what Aurora installs

R2 Step 6. A maintainer prepares
[python-tabulate](https://github.com/astanin/python-tabulate) for the `bugfix` workflow. They use
the most common setup, where Claude Code builds and Codex tests and reviews, and then try a
Hermes-only setup.

No CLI is logged in and no model is called. Everything else is real:

- the catalog skills, fetched from GitHub at their pinned commits;
- the installed files;
- Codex's rendered prompt input (`codex debug prompt-input`);
- Hermes' skill index (`hermes skills list`).

Claude Code has no offline skill listing, so its copies are checked with `aurora skills inspect`.

## Run it

In the `dev` container:

```bash
source /opt/aurora-env.sh && source /opt/aurora-cli-env.sh
examples/skills-install-demo/run-demo.sh      # DEMO_DIR defaults to /opt/aurora-demo
```

## What it checks (21 checks)

1. **Recommend.** For a bug fix in pair mode, review runs on Codex with `code-review`.
2. **Plan.** It lists 14 skill copies and 2 rule blocks, and the project is byte-identical
   afterwards.
3. **Review gate.** Applying without reviewing the two community Ponytail skills is refused, and
   the message names what to read.
4. **Apply.**
   - Claude's skills land in `.claude/skills` and Codex's in `.agents/skills`.
   - No installed skill keeps `allowed-tools`.
   - The Karpathy rules are a managed block in both `CLAUDE.md` and `AGENTS.md`.
   - A re-plan reports everything unchanged.
5. **Codex (offline).**
   - It loads `code-review`, `find-bugs` and `reproduce-bug`.
   - It does not see the Claude-only `test-driven-development` or `diagnosing-bugs`.
   - It has the Karpathy rules.
6. **Handoffs.** `workflow start --profile pair` names each stage's skills in the CLI's own syntax
   (`$reproduce-bug` for Codex), and every skill named exists where that CLI reads.
7. **Hermes.** After `hermes skills trust`, Hermes lists all 11 installed skills as enabled
   project skills.
8. **Undo.**
   - Both projects match a fresh clone again.
   - The 14 removed copies were moved aside under `.aurora/ops/`, not deleted.

## What it taught us

- **Codex and Antigravity share `.agents/skills`.** With both in a setup, their skill sets merge.
  `aurora skills plan` reports this as a leak, because the directory cannot be split by role.
- **Hermes ships builtin skills named `systematic-debugging` and `test-driven-development`.**
  Aurora's project copies take precedence. The TUI should show such shadowing.
- **Claude Code cannot list skills offline.** Verifying its loading needs a logged-in session
  (R4 runner).
