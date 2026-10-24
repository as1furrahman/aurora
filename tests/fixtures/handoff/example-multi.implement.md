# Aurora handoff: Plan, implement, test, review / stage `implement`

- Run: golden
- Workflow: plan-implement-review v1.0.0 (sha256 cab4526ead1b)
- Role: **implementer**: Make the planned change in small, reviewable steps without expanding scope.
- Provider: Google Antigravity CLI (`antigravity`, assigned by preferred)
- Attempts allowed: 2, then escalate

## Skills

- Required: principles
- Recommended: none

## Task

Make the planned change in small, reviewable steps without expanding scope.

## Inputs

- `/aurora-golden/run/artifacts/plan/plan.md`

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/implement/change-summary.md`

## Acceptance criteria

- `change-described`: change-summary.md lists every file changed and why, traced to the plan. (evidence: artifacts/implement/change-summary.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage implement --met change-described
```

## How to run this stage with Google Antigravity CLI

Start `agy` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
