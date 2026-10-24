# Rust bugfix workflow demo

This demo runs Aurora's workflow engine end to end against a small Rust project with a Markdown
table escaping bug. The fixture starts with failing regression tests; the fix stage applies a local
patch, and the review and acceptance stages verify the result independently.

| Stage | Role | CLI | What happens |
|---|---|---|---|
| reproduce | tester | Codex | Captures the failing regression tests and expected behavior |
| root-cause | implementer | Hermes | Locates the unescaped cell join in the Rust source |
| fix | implementer | Antigravity | Applies the patch and runs the full Cargo test suite |
| review | reviewer (read-only) | Claude Code | Checks the escaping change and regression tests |
| accept | tester | Codex | Reruns the full suite independently; a human approves |

The project, tests, patch, and stand-in are Rust. Only agent reasoning is simulated: `stand_in.rs`
acts on the outputs, criteria, and safeguards in each Aurora handoff. No coding CLI login or network
access is needed.

## Run it

You need the Aurora workspace and Rust toolchain. The four provider CLIs are optional; the
demo checks that Aurora lists all four supported providers but does not require their binaries.

```bash
examples/real-bugfix-demo/run-demo.sh
```

The script prints Aurora's commands and 24 checks covering routing, compatibility, provider
independence, reviewer safeguards, stage gates, human approval, and evidence drift detection.

The fixture lives in `rust-table-fixture/`. Its `upstream-fix.patch` is applied only to a temporary
copy under `DEMO_DIR` (default: a unique directory under the system temp directory).
