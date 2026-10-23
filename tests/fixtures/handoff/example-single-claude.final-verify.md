# Aurora handoff: Plan, implement, test, review / stage `final-verify`

- Run: golden
- Workflow: plan-implement-review v1.0.0 (sha256 cab4526ead1b)
- Role: **tester**: Run and extend the test suite; report exact commands, counts and failures.
- Provider: Claude Code (`claude`, assigned by override)
- Independence: REDUCED. Stage `implement` also ran on `claude`. Start a fresh session and do not rely on earlier context; this is still not an independent review.
- Attempts allowed: 2, then escalate
- Requires human approval after submission.

## Skills

- Required: verification-guard
- Recommended: none

## Task

Run and extend the test suite; report exact commands, counts and failures.

## Inputs

- `/aurora-golden/run/artifacts/test/test-report.md`

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/final-verify/final-report.md`

## Acceptance criteria

- `final-suite-green`: final-report.md records a full test run after remediation with zero failures. (evidence: artifacts/final-verify/final-report.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage final-verify --met final-suite-green
```

## How to run this stage with Claude Code

Start `claude` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
