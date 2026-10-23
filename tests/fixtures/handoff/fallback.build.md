# Aurora handoff: Minimal / stage `build`

- Run: golden
- Workflow: mini v1.0.0 (sha256 3ed68e238ec1)
- Role: **builder**: Build it.
- Provider: Google Antigravity CLI (`antigravity`, assigned by preferred)
- Attempts allowed: 2, then escalate

## Skills

- Required: alpha
- Recommended: none

## Task

Build it.

## Inputs

- none

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/build/out.md`

## Acceptance criteria

- `built`: out.md exists. (evidence: artifacts/build/out.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage build --met built
```

## How to run this stage with Google Antigravity CLI

Start `agy` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
