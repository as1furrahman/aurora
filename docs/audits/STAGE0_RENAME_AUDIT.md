# Stage 0 — Independent Rename Audit (Agent Workbench → Aurora)

**Date:** 2026-10-23
**Auditor:** Claude Opus 5.5 (Claude Code)
**Audited commit:** `7c4be93b3b3d1143fc653ddffbcea52ce5d775ac` ("chore: rename Agent Workbench to Aurora")
**Audit branch:** `feat/aurora-workflow-slice` (created from `7c4be93`, no code changes during audit)
**Raw evidence:** `docs/audits/evidence/stage0/`

**Verdict:** The rename end state is **functionally sound in a source checkout** (150/150 tests, both
entry points work, `AURORA_*` precedence over `AWB_*` holds). Several claims in
`docs/AURORA_RENAME_REPORT.md` and in the handoff **do not match this checkout or host**, and one
pre-existing packaging defect makes an installed wheel unable to load its registry. None of these
block the next milestone; all are recorded below.

---

## 1. Repository preflight

| Item | Reported | Observed | Match |
|---|---|---|---|
| Repository root | `/home/asif/projects/agent-workbench` | `/home/thesriz/Development/aurora` (`/home/asif` does not exist on this host) | ❌ |
| Branch | `chore/rename-agent-workbench-to-aurora` | same | ✅ |
| HEAD | `7c4be93b…` | `7c4be93b3b3d1143fc653ddffbcea52ce5d775ac` | ✅ |
| Working tree | — | clean (no tracked/untracked changes) | ✅ |
| Remotes | "no remote configured" | `origin https://github.com/as1furrahman/aurora.git`; `origin/chore/rename-agent-workbench-to-aurora` exists (already pushed) | ❌ |
| Rename report | `AURORA_RENAME_REPORT.md` | `docs/AURORA_RENAME_REPORT.md` | ⚠️ path differs |
| History | parent → rename diff | `git rev-list --count HEAD` = **1**; `7c4be93` is a **root commit** adding 60 files / 12,400 lines | ❌ |

Commands:
```
git rev-parse --show-toplevel; git branch --show-current; git rev-parse HEAD
git status --porcelain=v1 -uall; git remote -v; git log --all --oneline; git rev-list --count HEAD
git show --stat HEAD
```

## 2. Findings

### F1 — No pre-rename history exists (audit limitation)
Because `7c4be93` has no parent, there is no parent-to-commit diff. The report's §2 "Files Changed &
Summary of Diff" cannot be verified from Git. This audit therefore verifies the **end state** only.
The earlier `PHASE*_REPORT.md` files are included in the same root commit and are likewise unverifiable
as history.

### F2 — Remote exists and branch is pushed
Contradicts both the rename report and the handoff ("no remote configured; commit local only").
This milestone does **not** push; all new commits remain local.

### F3 — Protected-asset baseline is not verifiable on this host
`preservation_baseline.json` records 346 nodes under four roots, all of which are absent here:
`/home/asif/projects/{token-efficient-workflow,dwl,dwl-native-bar-lab}` and
`/home/asif/.codex/skills/.system` (and `~/.codex` does not exist for the current user).
The "346/346 PRISTINE" result **cannot be reproduced** and is reported here as *unverifiable*, not as
pristine or as drifted.

### F4 — Protection lists are hardcoded to another user's home
`awb/safety/boundaries.py:21-25` hardcodes `/home/asif/projects/...`. The check is purely path-based
(`Path.resolve()` works on non-existent paths), so it still rejects those literal paths on any host —
`tests/test_cli.py::test_cmd_install_rejects_protected_project_targets` passes for the right reason
(observed error: "conflicts with protected project directory"). But on this host it protects nothing
that exists. Making the protected list host-configurable is out of scope for this milestone and is
listed as an open risk.

### F5 — Claude Code skill directory is not guarded
`GLOBAL_AGENT_SKILL_PATHS` covers `~/.codex/skills`, `~/.gemini/antigravity-cli/skills`,
`~/.gemini/config/skills` but not `~/.claude/skills`, which **exists on this host**. No Claude Code
adapter exists yet, so no code path currently targets it, but it must be added to the global block
list before any Claude adapter is introduced (done in Stage 2).

### F6 — Installed wheel cannot find its registry (pre-existing packaging defect)
`Config.load()` defaults to `<package parent>/registry/skills.toml`. The wheel contains only `awb/*`
— no `registry/` or `skills/`. Built and installed into a scratch venv:
```
$ aurora --version   -> Aurora (aurora) 0.1.0        [exit=0]
$ awb --version      -> Aurora (awb) 0.1.0           [exit=0]
$ aurora list        -> Error loading registry: Registry file does not exist:
                        '.../venv/lib/python3.14/site-packages/registry/skills.toml'  [exit=1]
```
Entry points themselves are correct (`entry_points.txt`: `aurora = awb.cli:main`, `awb = awb.cli:main`).
Workaround today: run from a checkout or set `AURORA_REGISTRY_PATH`. Proposed for the next milestone.

### F7 — `doctor` overstates health
`awb/cli.py` `cmd_doctor` always ends with "Host environment is healthy" and returns 0. Its
"Protected Assets Preservation Invariant" check only tests whether
`/tmp/full_preservation_baseline.json` exists; it never hashes anything. Observed output on this host:
"Preservation Baseline: None active … Host environment is healthy." Not changed in this milestone
(behavioral change to an existing command); recorded as an open risk.

### F8 — Environment-variable precedence semantics (documented, not a defect)
`AURORA_*` wins over `AWB_*` (verified). Implemented with `or`, so an **empty** `AURORA_X=""` is
treated as unset and falls through to `AWB_X` (verified with `AURORA_REGISTRY_PATH=` +
`AWB_REGISTRY_PATH=/b` → `/b`). For allowed roots, `AURORA_ALLOWED_ROOTS` replaces (does not merge
with) `AWB_ALLOWED_ROOTS`. Both are reasonable "empty means unset" semantics; documented rather than
changed.

### F9 — Residual legacy naming (cosmetic)
User-visible: `update`/`remove` help text says "AWB-managed". Docstrings/comments still say "Agent
Workbench" in `errors.py`, `safety/locking.py`, `safety/filesystem.py`, `transactions/engine.py`,
`operations/{__init__,installer,remover,manifest}.py`, `adapters/base.py`, and the
`registry/skills.toml` header. Per the handoff, the `awb` package and `.awb.*` file names are
intentionally preserved; these strings are low-risk and left for a cosmetic follow-up.

### F10 — `CodexCLIAdapter.validate_conformance` writes into its sandbox
It creates `.system/SKILL.md` and a symlink inside the supplied sandbox while the base contract
describes conformance as "non-mutating". The writes are confined to the caller's sandbox; the
docstring is inaccurate. Recorded; not changed.

## 3. Rename end-state checks

| Check | Command | Result |
|---|---|---|
| Package metadata | `pyproject.toml` | `name = "aurora"`, `version = "0.1.0"`, `dependencies = []`, scripts `aurora` + `awb` → `awb.cli:main` ✅ |
| Canonical help | `python3 -m awb --help` | `usage: aurora …` ✅ |
| Version | `python3 -m awb --version` / `python3 -m awb.cli --version` | `Aurora (aurora) 0.1.0` ✅ |
| Alias prog by argv[0] | `sys.argv=["/usr/bin/awb","--version"]; main()` | `Aurora (awb) 0.1.0` ✅ |
| Detect | `python3 -m awb detect` / `--json` | 4 adapters, all NOT DETECTED on this host, exit 0 ✅ |
| Registry | `python3 -m awb list` / `validate` | 4 skills, acyclic, exit 0 ✅ |
| Doctor | `python3 -m awb doctor` | exit 0 (see F7) ⚠️ |
| Env precedence | see `evidence/stage0/stage0_env.log` | ✅ (see F8) |
| Wheel | `pip wheel . --no-deps` in scratch venv | builds; entry points ✅; registry missing ❌ (F6) |
| Durable names | grep | `.awb.lock`, `.awb.manifest.json`, `.awb.journal.json`, `awb/` unchanged ✅ |

## 4. Test suite

```
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -v
Ran 150 tests in 1.090s
OK
```
150 passed, 0 failures, 0 errors, 0 skips (Python 3.14.8). Matches the reported count.
Full log: `evidence/stage0/stage0_tests.log`.

## 5. Host preservation snapshot (replacement for the unverifiable baseline)

A read-only snapshot (type, mode, size, sha256 of every node) was taken **before any work** of the
agent directories that exist on this host:

| Root | State |
|---|---|
| `~/.claude/skills` | present |
| `~/.gemini/settings.json` | present |
| `~/.gemini/config`, `~/.gemini/antigravity-cli`, `~/.codex`, `~/.agents` | absent |

274 nodes recorded. The snapshot is re-taken and diffed at the end of the milestone (see the
milestone report). The snapshot file is kept outside the repository because it describes the user's
private configuration.

## 6. Gate status

- Baseline healthy → proceed to Stage 1.
- Gate C (live install/update/remove on real agent directories) remains **BLOCKED**.
