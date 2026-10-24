# Aurora handoff: Plan, implement, test, review / stage `remediate`

- Run: golden
- Workflow: plan-implement-review v1.0.0 (sha256 cab4526ead1b)
- Role: **implementer**: Make the planned change in small, reviewable steps without expanding scope.
- Provider: Claude Code (`claude`, assigned by override)
- Attempts allowed: 2, then escalate

## Skills

- Required: principles
- Recommended: none

## Task

Make the planned change in small, reviewable steps without expanding scope.

## Inputs

- `/aurora-golden/run/artifacts/review/review.md`

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/remediate/remediation.md`

## Acceptance criteria

- `findings-addressed`: remediation.md maps each review finding to a fix or a justified deferral. (evidence: artifacts/remediate/remediation.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage remediate --met findings-addressed
```

This stage is optional. If it does not apply, write the justification to `/aurora-golden/run/artifacts/remediate/NOT_APPLICABLE.md` and run:

```
aurora workflow submit --run /aurora-golden/run --stage remediate --not-applicable
```

## How to run this stage with Claude Code

Start `claude` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
