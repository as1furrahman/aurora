# Aurora handoff: Plan, implement, test, review / stage `test`

- Run: golden
- Workflow: plan-implement-review v1.0.0 (sha256 cab4526ead1b)
- Role: **tester**: Run and extend the test suite; report exact commands, counts and failures.
- Provider: Claude Code (`claude`, assigned by override)
- Attempts allowed: 3, then escalate

## Skills

- Required: verification-guard
- Recommended: none

## Task

Run and extend the test suite; report exact commands, counts and failures.

## Inputs

- `/aurora-golden/run/artifacts/implement/change-summary.md`

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/test/test-report.md`

## Acceptance criteria

- `suite-green`: test-report.md records the exact test command and a run with zero failures and errors. (evidence: artifacts/test/test-report.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage test --met suite-green
```

## How to run this stage with Claude Code

Start `claude` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
