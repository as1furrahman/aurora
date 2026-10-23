# Aurora handoff: Plan, implement, test, review / stage `plan`

- Run: golden
- Workflow: plan-implement-review v1.0.0 (sha256 cab4526ead1b)
- Role: **planner**: Turn the task into a scoped implementation plan with risks, non-goals and a test strategy.
- Provider: Claude Code (`claude`, assigned by override)
- Attempts allowed: 2, then escalate

## Skills

- Required: principles
- Recommended: task-router

## Task

Turn the task into a scoped implementation plan with risks, non-goals and a test strategy.

## Inputs

- none

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/plan/plan.md`

## Acceptance criteria

- `plan-written`: plan.md states scope, non-goals, risks and the test strategy. (evidence: artifacts/plan/plan.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage plan --met plan-written
```

## How to run this stage with Claude Code

Start `claude` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
