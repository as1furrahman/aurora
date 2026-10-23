# Real-project dry run: python-tabulate issue #241

This demo runs Aurora's workflow engine end to end on a **real open-source bug**, with no CLI
logins.

The scenario: a maintainer triages [python-tabulate](https://github.com/astanin/python-tabulate)
issue #241, where `|` inside a cell breaks Markdown tables. They use the built-in `bugfix` preset
and spread the roles across four coding CLIs.

| Stage | Role | CLI | What happens |
|---|---|---|---|
| reproduce | tester | Codex | bug report with current, expected and **unchanged** behaviour; the real regression test fails |
| root-cause | implementer | Hermes | the cause is located in the real source (file and line) |
| fix | implementer | Antigravity | the **real upstream fix** (commit `e6a24aa`) is applied and the real suite runs green |
| review | reviewer (read-only) | Claude Code | two-pass review; structured verdict `concerns` (no regression test covers the `pipe` format) |
| accept | tester | Codex | sees the concern in its handoff, re-runs everything independently, checks unchanged behaviour; a human approves |

**Real:**
- the installed CLIs, detected on `PATH`;
- the project and its git history;
- the upstream fix;
- every test run;
- every Aurora plan and gate decision.

**Simulated:** only the agents' reasoning. `stand_in.py` plays each CLI after the handoff bundle
has been "pasted" into it. It acts only on what the bundle says (outputs, criteria, upstream
concerns), so the demo also checks that bundles are complete enough to act on. No model is
called.

## Run it

You need the `aurora` workspace and network access for the first clone. The four CLIs are
optional: without them, check 1 fails, because it expects all four to be detected.

```bash
examples/real-bugfix-demo/run-demo.sh        # DEMO_DIR defaults to /opt/aurora-demo
```

It prints each Aurora command's output and **25 checks**. Among them:

- **Routing:** any CLI may play any preset role, and `--profile pair` routes the whole workflow
  (claude builds, codex tests and reviews).
- **Compatibility:** the shipped registry blocks Hermes as `incompatible`, so nothing is silently
  converted, and the block says how to fix it.
- **Independence:** review runs on a different CLI from the fix.
- **Read-only reviewer:** the reviewer's handoff carries the `claude --permission-mode plan`
  safeguard.
- **Gates:**
  - a forgotten `--met` fails the gate;
  - the `concerns` verdict passes, and the concern reaches the acceptance handoff;
  - acceptance waits for human approval.
- **Integrity:** editing accepted evidence after approval is detected as drift.

## What the dry run taught us (inputs for the TUI and later milestones)

1. **Presets needed editing before Hermes could play a role.** Fixed in R2 Step 5: every preset
   role now allows all four CLIs (preferences unchanged). The TUI's Roles board should still make
   "allow this CLI for this role" a single action for team workflows.
2. **The shipped registry declares no Hermes compatibility,** so plans block. That is correct
   behaviour; since R2 Step 5 the plan prints the fix (route the stage elsewhere, or declare the
   skill compatible in a team registry).
3. **Operators type one `--met` flag per criterion.** Forgetting one fails the gate with a clear
   message. In the TUI, the submit form should show the criteria as checkboxes.
4. **`github` has no format definition of its own at this commit:** tabulate builds it from
   `pipe`. A realistic agent would have to discover that. The root-cause stand-in handles it.
