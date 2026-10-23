# Workflow Schema (v1)

**Status:** implemented in Milestone 1 (`awb/workflow/`).
**Example:** [`workflows/examples/plan-implement-review.toml`](../../workflows/examples/plan-implement-review.toml)

## 1. Definition file (TOML, `schema_version = 1`)

The parser (`awb.workflow.parser`) is strict. Unknown keys at any level are errors, and so are
wrong types and dangling references.

```toml
schema_version = 1

[workflow]
id = "plan-implement-review"      # ^[a-z0-9]+(-[a-z0-9]+)*$
version = "1.0.0"                 # semver
name = "Plan, implement, review"
description = "…"                 # optional
task_types = ["feature", "bugfix"]  # optional
assurance = "standard"            # basic | standard | high

[roles.planner]                   # role id: same pattern as workflow id
responsibility = "Produce an implementation plan with risks and test strategy."
required_skills = ["principles"]  # must exist in the skill registry
recommended_skills = ["task-router"]  # unknown ids → warning
# Any provider-like key here (provider, providers, preferred_provider, eligible_providers,
# model, cli) is rejected: role ≠ provider.

[[stages]]
id = "plan"
role = "planner"
description = "…"                 # optional
depends_on = []                   # stage ids; must form a DAG
inputs = []                       # "<stage>/<artifact>", from a transitive dependency's outputs
outputs = ["plan.md"]             # bare file names; at least one
optional = false                  # optional stages may become NOT_APPLICABLE
max_attempts = 2                  # 1..10; failed submissions allowed before BLOCKED
on_exhausted = "escalate"         # escalate | block
requires_approval = false         # human approval required before PASSED (not allowed with optional = true)
independent_of = []               # stage ids whose provider must differ for full independence

  [[stages.acceptance]]           # at least one per stage
  id = "plan-written"
  description = "Plan covers scope, risks, test strategy."
  evidence = ["plan.md"]          # non-empty subset of this stage's outputs

[routing]
allow_override = true             # permit --provider / --route

[routing.roles.planner]           # required for every role, and only for declared roles
eligible = ["claude", "codex", "antigravity"]   # known provider ids
preferred = "claude"              # optional; must be eligible; defaults to eligible[0]

[[routing.fallback]]              # optional, evaluated in declaration order
role = "implementer"
from = "antigravity"
to = "codex"                      # must be eligible for the role
on = ["incompatible", "unavailable"]
```

Known provider ids: `claude`, `codex`, `antigravity` (see `awb/providers/registry.py`).

## 2. Planning semantics (`awb.workflow.planner`)

**Order.** Stages are sorted topologically. Ties are broken by declaration order, so the same
input always yields the same order.

**Provider resolution** for each stage, in priority order:

| Source | Trigger | Not eligible for role | Fallback |
|---|---|---|---|
| `override` | `--provider P` (single-provider mode) | stage is `BLOCKED` with reason `not-eligible` | **disabled** |
| `route` | `--route STAGE=P` | plan error, nothing produced | **disabled** |
| `preferred` | default | n/a | allowed |

If `allow_override = false`, `--provider` and `--route` are plan errors.

**Checks** run on each assignment:

- *Incompatible.* A required skill whose `compatible_agents` contains neither `universal` nor any of
  the provider's skill adapters. The plan reports skill, provider and adapters checked. Nothing is
  converted.
- *Unavailable.* Only evaluated when a detected-provider set is supplied (the CLI uses
  `shutil.which`; `--no-detect` turns this off). By itself this is only a **warning**, because a
  manual handoff can be carried out on another machine.

**Fallback.** When a check fails and fallback is allowed, the first `[[routing.fallback]]` rule with
a matching `role`, `from` and reason is applied, **but only if the target provider does not have the
same problem** (for example, falling back from one uninstalled CLI to another uninstalled CLI is
skipped). Fallbacks can chain, but no provider is visited twice. Each step is recorded in `fallback_trail` as `{from, to, reason}`. If an incompatibility
remains after fallback, the stage is `BLOCKED`. **There is no implicit fallback.**

**Independence.** For each stage S that lists T in `independent_of`, the plan records
`distinct-provider` or `same-provider` for the pair. In single-provider mode every pair is
`same-provider`. The plan also notes that fresh-session isolation has not been verified and is a
weaker control. Aurora never reports "independent verification" for a `same-provider` pair.

**Plan output.** A JSON-serializable dict with:

- `plan_schema_version`, `workflow {id, version, sha256}`, `mode` (`multi` | `single`),
  `provider_override`, `routes`, `detection` (or `null`), `order`, `warnings`, `blocked`
- `stages[]`, where each entry has: `id`, `role`, `provider`, `assignment_source`,
  `fallback_trail`, `status` (`assignable` | `blocked`), `blocked_reasons`, `warnings`,
  `incompatibilities`, `independence`, `handoff_path`, `outputs` (run-relative paths),
  `acceptance`

`aurora workflow plan` writes nothing.

## 3. Run directory

```
<run-root>/<run-id>/
  run.json                   # run record (schema_version 1)
  workflow.toml              # byte-for-byte copy of the definition; sha256 pinned in run.json
  handoff/<stage>.md         # role-specific prompt bundle
  artifacts/<stage>/<output> # where the person running the handoff puts outputs
  artifacts/<stage>/NOT_APPLICABLE.md   # justification, optional stages only
  .awb.lock                  # ProcessLock for every run-record update
```

- `--run-root` is mandatory and must already exist. Each write is validated with
  `validate_security_preconditions(<path>, [<run-dir>])` and done with `SafeFS.safe_atomic_write`.
- The default `run-id` is `<workflow-id>-<UTC yyyymmddThhmmssZ>`. Ids are restricted to
  `[A-Za-z0-9._-]`. Aurora refuses to reuse an existing run directory.

## 4. Stage lifecycle

```
PENDING ──deps satisfied──▶ READY ──handoff issued──▶ RUNNING ──submit OK──▶ PASSED
   │                                                    │  ▲         └─ requires_approval → (awaiting approval) ─approve─▶ PASSED
   │                                                    │  └─ retry (submit again)
   │                                                    ▼
   │                                                 FAILED ──attempts ≥ max──▶ BLOCKED (escalated if on_exhausted=escalate)
   └─ plan blocked ─▶ BLOCKED
RUNNING/FAILED (optional stage) ──submit --not-applicable + justification──▶ NOT_APPLICABLE
```

- **Dependencies satisfied** means every dependency is `PASSED` or `NOT_APPLICABLE`. A stage whose
  dependency is `BLOCKED` stays `PENDING`, and `status` names the blocking stage.
- **Submit** (`aurora workflow submit --run R --stage S --met C …`) is accepted only in `RUNNING` or
  `FAILED`. For each declared output, Aurora checks that a non-empty regular file (not a symlink) is
  present under `artifacts/<stage>/` and hashes it. Each criterion needs an explicit `--met` claim
  and its evidence present. Unknown criterion ids are rejected without counting an attempt.
  - all satisfied → `PASSED` (or *awaiting approval*); evidence hashes recorded
  - otherwise → `FAILED` with an explicit `missing` list, or `BLOCKED` once `max_attempts` is reached
- **An exit code never appears in this decision.**
- **Approve** (`aurora workflow approve --run R --stage S --by NAME`) only applies to a stage that is
  awaiting approval. It re-hashes the evidence first and refuses if anything changed. Submitting again
  while awaiting approval re-evaluates the gate (the way out after evidence changes) and never skips
  approval.
- `SKIPPED` is reserved in the v1 enum. The v1 engine never produces it.
- **Run complete** means every non-optional stage is `PASSED`, every optional stage is `PASSED`
  or `NOT_APPLICABLE`, and no accepted evidence has drifted since acceptance.

## 5. Resume and integrity

`aurora workflow resume --run R` and `status` reload `run.json` under the lock and do the following:

- check that `workflow.toml` still matches the pinned sha256, refusing to proceed on mismatch;
- issue handoff bundles for newly ready stages; never re-issue for `PASSED` stages; never overwrite
  an existing bundle whose content differs (a warning is recorded instead);
- re-hash the evidence of `PASSED` stages and report **evidence drift** without changing state.

All writes are atomic. `start` saves the initial record before issuing any bundle, and later
commands write bundles before the record that references them. A crash at any point leaves a valid
record: either the old one, so the next command or `resume` re-issues identical bundles idempotently,
or the new one. `start` validates, plans and pins the same definition bytes.

**Known limitation (v1):** a `PASSED` stage whose evidence later drifts makes the run permanently
incomplete. Drift is reported, but there is no re-open transition yet. Handoff bundles also embed the
absolute run path, so moving a run directory makes unedited bundles look edited.

## 6. Run record (`run.json`, `schema_version = 1`)

```json
{
  "schema_version": 1,
  "kind": "aurora.workflow-run",
  "run_id": "…", "created_at": "…Z", "updated_at": "…Z",
  "workflow": {"id": "…", "version": "…", "sha256": "…", "path": "workflow.toml"},
  "plan": { "…": "see §2" },
  "stages": {
    "<id>": {
      "state": "RUNNING", "provider": "claude", "attempts": 0,
      "awaiting_approval": false, "escalated": false,
      "handoff": {"path": "handoff/<id>.md", "sha256": "…", "preserved_existing": false},
      "evidence": {"artifacts/<id>/plan.md": "<sha256>"},
      "criteria_met": ["plan-written"], "missing": [], "blocked_reasons": [],
      "approval": null, "not_applicable_reason": null
    }
  },
  "events": [{"seq": 1, "at": "…Z", "stage": "plan", "event": "handoff-issued", "detail": {}}]
}
```

The loader rejects unknown `schema_version` or `kind`, invalid states, and a stage set that does not
match the pinned workflow. Future versions must add a migration rather than reinterpret v1
(ADR-0003).

## 7. Additive extensions to schema v1 (Rust implementation)

These fields were added after the Python reference was frozen (ADR-0006, R1 Step 6). They are
**additive**. A definition that does not use them is valid in both implementations and produces
identical plans and runs. The Python implementation rejects a definition that does use them,
because of its strict unknown-key rule. It does still load and continue *runs* that carry the
new data: gate data lives in a top-level `gates` map, which Python tolerates, while stage
records keep exactly the v1 keys.

### 7.1 Structured verdicts and gate outcomes

```toml
[[stages]]
id = "review"
outputs = ["review.md", "review.json"]
verdict = "review.json"        # must be one of the outputs and end in .json
```

`review.json` takes this form:
`{"outcome": "pass" | "concerns" | "fail", "findings": [{"severity": "...", "summary": "..."}]}`.

On `submit`, after the usual output and criterion checks:

| Verdict | Effect |
|---|---|
| `pass` | The gate passes. Recorded in `gates.<stage>`. |
| `concerns` | The gate passes. Findings are recorded and listed under **"Open concerns from upstream stages"** in every downstream handoff (transitive dependents). `status` lists the stage under `concerns`. |
| `fail` | Counts as missing evidence (`verdict outcome is fail (n finding(s))`). The stage becomes FAILED, or BLOCKED once attempts run out. |
| malformed | Counts as missing evidence (`verdict invalid: … (reason)`). |

### 7.2 Waivers

`aurora workflow waive --run R --stage S --reason TEXT --by NAME` accepts a **FAILED or BLOCKED**
stage by recorded human decision (BMAD's WAIVED, Stage-Gate's "go with conditions"). Rules:

- `--reason` and `--by` are required.
- The stage becomes `PASSED` and its escalation is cleared.
- `gates.<stage> = {outcome: "waived", waiver: {by, at, reason}}`, and a `waived` event is
  recorded.
- Every downstream handoff shows `WAIVED by <name>: <reason>`.
- `status` lists the stage under `waived`.

PENDING, RUNNING and PASSED stages cannot be waived.

### 7.3 Run-record and status additions

- `run.json` may contain `gates: {<stage>: {outcome, findings, waiver?}}`. It is omitted when empty.
- `status --json` may contain `concerns: [...]` and `waived: [...]`. Both are omitted when empty,
  so status output for runs without gates matches Python exactly.

### 7.4 Capability requirements and enforcement strength

```toml
[roles.reviewer]
responsibility = "Independently review the change."
requires = ["read_only"]        # known: read_only, tool_restriction
```

A role may require capabilities. That keeps it provider-neutral: it says *what* must hold,
not *which* CLI. Each provider declares how strongly it enforces each capability, with a cited
source (`aurora providers --json`). The table follows AWS CLI Agent Orchestrator's documented
tool-restriction findings.

| Provider | `read_only` | `tool_restriction` |
|---|---|---|
| claude | **hard**: `claude --permission-mode plan` | **hard**: `--disallowedTools` |
| codex | **hard**: `codex --sandbox read-only` | soft: no per-tool deny flag |
| antigravity | soft: `agy --mode plan` (blocking not verified) | soft: settings deny rules, not verifiable by Aurora |

The plan records `enforcement: [{capability, strength, how, source}]` for each stage (omitted when
empty). A soft-only assignment produces a warning that is never silent. The handoff gains a
**Required safeguards** section telling the person how to start the CLI in the restricted mode.
In manual-handoff mode the person applies the restriction. Aurora never passes these flags itself
in R1.
