# Aurora handoff: Minimal / stage `check`

- Run: golden
- Workflow: mini v1.0.0 (sha256 3ed68e238ec1)
- Role: **checker**: Check it.
- Provider: OpenAI Codex CLI (`codex`, assigned by fallback)
- Fallback applied: antigravity -> codex (incompatible)
- Independence: stage `build` ran on a different provider (`antigravity`). Do not consult its session.
- Attempts allowed: 2, then escalate

## Skills

- Required: beta
- Recommended: none

## Task

Check it.

## Inputs

- `/aurora-golden/run/artifacts/build/out.md`

## Required outputs

Write each file below. Only these files count as evidence:

- `/aurora-golden/run/artifacts/check/check.md`

## Acceptance criteria

- `checked`: check.md exists. (evidence: artifacts/check/check.md)

## Submit

The stage passes only when every output exists and every criterion is claimed with its evidence present.
A zero exit code, or a statement that the work is done, is not sufficient on its own.

```
aurora workflow submit --run /aurora-golden/run --stage check --met checked
```

## How to run this stage with OpenAI Codex CLI

Start `codex` interactively in the project directory and paste this handoff (everything above).

Use the CLI's normal permission prompts. Do not use flags that bypass approvals or sandboxing.
