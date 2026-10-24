# Aurora handoff: Plan, implement, test, review / stage `review`

- Run: golden
- Workflow: plan-implement-review v1.0.0 (sha256 cab4526ead1b)
- Role: **reviewer**: Independently review the change against the plan for correctness, safety and scope.
- Provider: OpenAI Codex CLI (`codex`, assigned by preferred)
- Independence: stage `implement` ran on a different provider (`antigravity`). Do not consult its session.
- Attempts allowed: 2, then escalate

## Skills

- Required: verification-guard, principles
- Recommended: none

## Task

Independently review the change against the plan for correctness, safety and scope.

## Inputs

- `/aurora-golden/run/artifacts/plan/plan.md`
- `/aurora-golden/run/artifacts/implement/change-summary.md`
- `/aurora-golden/run/artifacts/test/test-report.md`

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/review/review.md`

## Acceptance criteria

- `review-complete`: review.md lists findings by severity, or states that none were found. (evidence: artifacts/review/review.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage review --met review-complete
```

## How to run this stage with OpenAI Codex CLI

Start `codex` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
