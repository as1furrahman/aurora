# Aurora Milestone 1: Report

**Date:** 2026-10-23
**Implementer:** Claude Opus 5.5 (Claude Code)
**Branch:** `feat/aurora-workflow-slice`, from `7c4be93`. **Local only, not pushed.**
**Handoff:** `AURORA_OPUS_55_HANDOFF.md`
**Gate C:** remains **BLOCKED**. No live install, update or remove was performed.

## 1. Commits

| SHA | Scope |
|---|---|
| `1cffd0f` | Stage 0: independent rename audit (`docs/audits/STAGE0_RENAME_AUDIT.md`) |
| `5c8e6a9` | Stage 1: product vision, architecture, workflow schema, landscape, ADR-0001…0005 |
| `6b023d2` | Stage 2: `awb.providers` execution contract, Claude Code skill adapter, global-path hardening |
| `c501de1` | Stages 3–4: strict workflow parser, deterministic planner (routing / override / fallback), `workflow validate|show|plan` |
| `aa0b952` | Stage 3: evidence-gated runs, handoff bundles, resume, `workflow start|status|submit|approve|resume` |
| `7e36fd2` | Fixes from the independent code review (9 regression tests) and the README section |
| *(this commit)* | This report and final evidence logs |

The handoff asked for one commit per stage. Two deviations:

- The routing tests (Stage 4) were written *before* the planner and committed with it, as TDD
  requires.
- The run-side tests (evidence, resume) were committed with the engine.

## 2. Deliverables against the handoff

| # | Deliverable | Where |
|---|---|---|
| 1 | Independent rename audit with reproducible evidence | `docs/audits/STAGE0_RENAME_AUDIT.md`, `docs/audits/evidence/stage0/` |
| 2 | Product vision and architecture/schema docs informed by ASM and Skills Manager | `docs/product-vision.md`, `docs/architecture/*.md`, `docs/references/skill-manager-landscape.md`, `docs/adr/` |
| 3 | Documented, tested split between skill-storage adapters and execution providers | ADR-0002, `docs/architecture/provider-adapter-contract.md`, `awb/providers/`, `tests/test_providers.py` |
| 4 | Workflow definition and plan/manual-handoff vertical slice | `awb/workflow/`, `workflows/examples/plan-implement-review.toml` |
| 5 | Tests for multi-provider routing vs single-provider override | `tests/test_workflow_routing.py`, `tests/test_workflow_cli.py` |
| 6 | Full test report and protected-asset preservation report | §3 and §4 below |
| 7 | Unsupported capabilities, open risks, next milestone | §5–§7 below |

Each Stage 4 requirement maps to these tests:

| Requirement | Test(s) |
|---|---|
| Each stage gets its selected provider; unsupported or unknown providers fail clearly | `TestMultiProviderRouting.test_each_stage_gets_its_preferred_provider`, `test_route_overrides_one_stage_only`, `test_unknown_provider_fails_clearly`, `test_ineligible_route_fails_clearly` |
| `--provider` maps every eligible stage and keeps roles and gates | `TestSingleProviderOverride.test_override_maps_every_eligible_stage`, `test_override_preserves_roles_and_gates`, `test_override_to_ineligible_role_blocks_instead_of_switching` |
| Incompatible required skills are reported, with no lossy conversion | `TestCapabilityAndFallback.test_incompatible_required_skill_blocks_with_report` |
| Fallback is deterministic, declared and recorded, never silent | `test_declared_fallback_is_applied_and_recorded`, `test_fallback_disabled_under_override_and_route`, `test_unavailable_fallback_requires_detection_and_a_rule`, `test_unavailable_without_rule_is_warning_not_switch`, `test_fallback_skips_targets_with_the_same_problem` |
| A shared provider is never labeled independent | `test_single_provider_reports_reduced_independence`, `test_multi_provider_review_is_distinct_unless_shared`, `test_independence_never_claims_more_than_provider_distinctness` |
| A stage cannot pass without outputs and evidence | `TestAcceptanceGates.*` (missing, unclaimed, empty and symlinked outputs; retry; exhaustion) |
| Interrupted or failed runs resume without repeating unsafe side effects | `TestResumeAndIntegrity.*`, `TestReviewRegressions.test_failed_start_leaves_a_resumable_or_no_run` |

## 3. Test report

Command, with Python 3.14.8:

```
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests -v
Ran 281 tests in 1.834s
OK
```

| Suite | Tests |
|---|---|
| Baseline (unchanged, all 150 test ids present and passing) | 150 |
| `test_providers.py` | 22 |
| `test_workflow_parser.py` | 36 |
| `test_workflow_routing.py` | 23 |
| `test_workflow_cli.py` | 11 |
| `test_workflow_engine.py` | 39 |
| **Total** | **281**: 0 failures, 0 errors, 0 skips |

The only baseline test edited is `test_cli.py::test_cmd_detect_json`. It asserted exactly 4
adapters; it now asserts the exact set of 5 adapter ids.

### SEC-01…SEC-11 preservation

The safety modules (`awb/safety/filesystem.py`, `locking.py`, `transactions/`, `operations/`) are
**unchanged**. The only safety edit adds entries to `GLOBAL_AGENT_SKILL_PATHS`, which makes the
rules stricter. All baseline tests covering these properties pass unchanged:

- lock inode persistence
- non-tautological allowed roots
- `CREATE_DIR` and rollback directory recreation
- ownership and drift checks
- transactional pruning
- KeyboardInterrupt rollback
- journal recovery
- constrained recovery reporting
- strict registry validation
- `.system` isolation

The new workflow code reuses those primitives (`validate_security_preconditions`,
`SafeFS.safe_atomic_write` / `safe_open_read`, `ProcessLock`) and does not duplicate them.

### Mutation checks

- With the two new global-path entries removed in-process, the 3 hardening tests fail. They pass with
  the entries in place.
- Every new test file was run first and failed (RED) before its implementation existed.

### End-to-end (real CLI, scratch run root)

Evidence: `docs/audits/evidence/stage3/stage3_e2e.log`, `docs/audits/evidence/stage5/final_e2e.log`.

- A submission with no evidence leads to `FAILED` (exit 1).
- A submission with evidence leads to `PASSED`.
- An optional stage can become `NOT_APPLICABLE` with a justification file.
- The approval gate results in `PASSED (approved)` and a run that is `COMPLETE`.
- A single-provider run reports reduced independence for `review` and `final-verify`.

## 4. Protected-asset preservation

- **The 346-node baseline cannot be verified on this host.** All four protected roots
  (`/home/asif/projects/{token-efficient-workflow,dwl,dwl-native-bar-lab}`,
  `~/.codex/skills/.system`) are absent. It is reported as unverifiable, not as pristine.
- **Host snapshot in its place.** I took a read-only sha256/mode/type snapshot of the agent
  directories that exist here (`~/.claude/skills`, `~/.gemini/settings.json`; `~/.gemini/config`,
  `~/.gemini/antigravity-cli`, `~/.codex`, `~/.agents` are absent). It covers 274 nodes and was taken
  before any work, mid-way, after Stage 5, and at the end. **All snapshots are byte-identical.**
- **Files created and removed outside the repository:** none. Mutation tests ran only in
  `tempfile` directories under `/tmp`. Demo runs and the wheel build ran in the session scratchpad.
  The `build/`, `aurora.egg-info/` and `__pycache__/` directories my runs created in the repo were
  removed.
- No external skill packs were added.

## 5. Unsupported providers and capabilities

- **Headless execution for all three providers.** `ExecutionProvider.execute()` raises
  `ProviderNotEnabledError`. Only `manual-handoff` exists (ADR-0005).
- **Provider version detection.** It would require running the binary, so it is not done.
- **Codex discovery path.** Current docs list `.agents/skills` (repo) and `~/.agents/skills` (user).
  The existing `codex-cli` adapter still reads `~/.codex/skills`, and it is unchanged and flagged
  here.
- **Claude Code project, nested and plugin skill scopes.** Only personal `~/.claude/skills` is
  discovered.
- **Antigravity.** The skill-format parity of the `antigravity-ide` legacy path
  (`~/.gemini/antigravity/skills`) is not modeled.
- **Shared `.agents/skills` directory.** Antigravity (workspace) and Codex (repo) both read it.
  This is documented but not yet surfaced during a project-scope install, which doesn't exist yet.
- **Planned skill metadata** (`source`, `revision`, `content_sha256`, `license`, `roles`,
  `validation`) is specified in the model doc. It is not yet in the registry schema.
- **Workflow v1 limits:**
  - `SKIPPED` is reserved but never produced.
  - A stage cannot be re-opened after its evidence drifts.
  - There are no conditional stages.
  - Bundles embed the absolute run path.

## 6. Open risks

1. **Protection lists are hardcoded** to `/home/asif/...` (`awb/safety/boundaries.py`). On any other
   host they protect nothing that exists. They should become host-configurable, with a
   fail-closed default.
2. **The remote already exists and was pushed** (`origin` = github.com/as1furrahman/aurora). This
   contradicts the rename report. Nothing was pushed in this milestone.
3. **There is no pre-rename history.** `7c4be93` is a root commit, so earlier phase reports cannot
   be verified from Git.
4. **An installed wheel cannot find `registry/skills.toml`** (audit F6). `aurora list` and
   `aurora validate` fail outside a checkout unless `AURORA_REGISTRY_PATH` is set. This also
   affects `workflow validate`, `show`, `plan` and `start` from an installed package.
5. **`aurora doctor` always reports "healthy"** (audit F7).
6. **Provider facts come from docs fetched on 2026-10-23.** Two Codex URLs redirect to a new
   domain, and Antigravity's docs disagree on the `--print-timeout` default. Re-verify before
   enabling execution.

## 7. Proposed next milestone

1. Fix packaging (F6) by shipping the registry and skills as package data, or by resolving them
   explicitly. Add an installed-wheel smoke test.
2. Make protected paths configurable (with a fail-closed default) and turn `doctor` into a real
   baseline verifier.
3. Reconcile the Codex skill discovery path with the current docs, while keeping `.system` isolation.
4. Add a stage re-open transition for drifted or reviewer-rejected evidence, and invalidate
   downstream stages.
5. **Then** headless execution for **one** provider:
   - an argument vector only, with no shell
   - a validated `cwd`
   - an environment allow-list
   - output captured under the run directory
   - a timeout followed by SIGTERM, then SIGKILL
   - forbidden flags refused
   - explicit per-run user approval
   - the exit code is recorded but never treated as acceptance

   This is specified in `provider-adapter-contract.md` §2.
