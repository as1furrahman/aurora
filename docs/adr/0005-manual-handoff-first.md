# ADR-0005 — Manual handoff first; no subprocess execution in Milestone 1

**Status:** Accepted (2026-10-23)

## Context
All three target CLIs document non-interactive modes, and each also documents flags that bypass
approvals entirely (`--dangerously-skip-permissions`, `--dangerously-bypass-approvals-and-sandbox` /
`--yolo`). Headless permission behavior varies: Antigravity soft-denies with exit code 0, Claude in
manual mode denies, and Codex defaults to a read-only sandbox. Codex exit codes are not documented.
Launching agents safely requires a tested design for permissions, timeouts and cancellation,
environment isolation, output capture and user approval.

## Decision
- In Milestone 1 every provider's only invocation mode is `manual-handoff`. Aurora renders a
  role-specific prompt bundle and the person runs it in the CLI of their choice. Results are
  ingested through `aurora workflow submit`.
- `ExecutionProvider.execute()` raises `ProviderNotEnabledError`. New code imports no
  `subprocess`, and a test enforces this.
- Each `ProviderSpec` lists `forbidden_flags`. Handoff bundles never suggest them.
- Detection uses `shutil.which` only. The binary is never run.

## Consequences
- Zero risk of Aurora launching an agent with unintended permissions in this milestone.
- The handoff contract (prompt, expected outputs, submit command) is the stable interface that a
  later headless executor must also satisfy.
- Next milestone: one provider behind explicit per-run approval, specified per
  `provider-adapter-contract.md` §2.
