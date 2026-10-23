#!/usr/bin/env python3
"""
Stand-in agents for the real-bugfix demo.

Each stand-in plays the role a coding CLI would play after a person pasted Aurora's handoff
bundle into it. It acts on the bundle (output paths, acceptance criteria, upstream concerns)
and on the REAL project: it runs the real test suite, reads the real source and applies the
real upstream fix (python-tabulate commit e6a24aa, issue #241). Only the model's reasoning is
scripted. No LLM is called and no CLI login is needed.

Usage: stand_in.py <stage> --run RUN_DIR --project PROJECT --python VENV_PYTHON --upstream MIRROR
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

UPSTREAM_FIX = "e6a24aa"  # "fix #241 - escape pipe character in github and pipe formats"
REGRESSION_TEST = "test/test_regression.py::test_github_escape_pipe_character"
SAMPLE = "[['foo|bar', 1]], headers=('spam|eggs', 'n')"
# A table with no pipe characters: its rendering must not change in ANY format (Kiro's
# "unchanged behaviour" requirement for bug fixes).
UNCHANGED_FORMATS = ["plain", "simple", "grid", "github", "pipe", "rst", "html"]


def sh(cmd, cwd, check=False):
    """Run an argv command (never a shell string) and return (exit_code, combined output)."""
    proc = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True)
    if check and proc.returncode != 0:
        sys.exit(f"command failed: {' '.join(cmd)}\n{proc.stdout}{proc.stderr}")
    return proc.returncode, (proc.stdout + proc.stderr).strip()


class Handoff:
    """What an agent can learn from the bundle Aurora wrote for its stage."""

    def __init__(self, run_dir: Path, stage: str):
        self.text = (run_dir / "handoff" / f"{stage}.md").read_text(encoding="utf-8")
        self.outputs = {}
        section = self.text.split("## Required outputs", 1)[1].split("##", 1)[0]
        for path in re.findall(r"^- `([^`]+)`", section, re.M):
            self.outputs[Path(path).name] = Path(path)
        self.criteria = re.findall(r"^- `([a-z0-9-]+)`: ", self.text.split("## Acceptance criteria", 1)[1], re.M)
        match = re.search(r"^aurora workflow submit .*$", self.text, re.M)
        self.submit = match.group(0) if match else ""
        concerns = self.text.split("## Open concerns from upstream stages", 1)
        self.concerns = re.findall(r"^- `[^`]+` .*$", concerns[1].split("\n## ", 1)[0], re.M) if len(concerns) > 1 else []
        self.role = re.search(r"^- Role: \*\*([a-z-]+)\*\*", self.text, re.M).group(1)
        self.provider = re.search(r"^- Provider: (.+?) \(`", self.text, re.M).group(1)

    def write(self, name: str, content: str) -> None:
        if name not in self.outputs:
            sys.exit(f"handoff does not ask for {name}; it asks for {sorted(self.outputs)}")
        self.outputs[name].write_text(content.rstrip() + "\n", encoding="utf-8")


def render(py, project, fmt, table=SAMPLE):
    code = f"from tabulate import tabulate; print(tabulate({table}, tablefmt={fmt!r}))"
    return sh([py, "-c", code], cwd=project, check=True)[1]


def baseline(py, project):
    return {fmt: render(py, project, fmt, "[['alpha', 1], ['beta', 22]], headers=('name', 'n')") for fmt in UNCHANGED_FORMATS}


def say(h: Handoff, msg: str) -> None:
    print(f"  [{h.provider} stand-in, role {h.role}] {msg}")


def reproduce(h, a):
    current = render(a.python, a.project, "github")
    code, out = sh([a.python, "-m", "pytest", "-q", REGRESSION_TEST], cwd=a.project)
    say(h, f"regression test exit code {code} (expected non-zero: the bug is present)")
    h.write("bugfix.md", f"""# Issue #241: pipe characters break github/pipe Markdown tables

## Current behaviour
A `|` inside a cell or header is emitted verbatim, which splits the Markdown cell:

```
{current}
```

## Expected behaviour
`|` inside cells and headers is escaped as `\\|` in the `github` and `pipe` formats.

## Unchanged behaviour (regression requirement)
Tables without pipe characters must render byte-identically in every format. Baseline captured
before any change:

```json
{json.dumps(baseline(a.python, a.project), indent=2)}
```
""")
    h.write("repro-test.md", f"""# Reproduction

Command: `python -m pytest -q {REGRESSION_TEST}`
Exit code: {code} (fails on the unfixed code)

```
{out[-1500:]}
```
""")


def root_cause(h, a):
    src = (Path(a.project) / "tabulate" / "__init__.py").read_text(encoding="utf-8").splitlines()
    rows = []
    for fmt in ("github", "pipe"):
        start = next((i for i, line in enumerate(src) if line.strip().startswith(f'"{fmt}": TableFormat(')), None)
        if start is None:  # at this commit `github` has no block of its own: it is built from `pipe`
            continue
        end = next(i for i in range(start + 1, len(src)) if src[i].strip() == "),")
        rows += [i + 1 for i in range(start, end) if 'DataRow("|", "|", "|")' in src[i]]
    escape = [i + 1 for i, line in enumerate(src) if "escape_map" in line][:3]
    say(h, f"github/pipe rows built without an escape map at lines {rows}")
    h.write("root-cause.md", f"""# Root cause

`tabulate/__init__.py` builds the `github` and `pipe` header and data rows as
`DataRow("|", "|", "|")` with no escape map (lines {rows}). Cell text is joined between `|`
separators unchanged, so a literal `|` becomes an extra column boundary.

Evidence: `DataRow` already supports an `escape_map` (used at lines {escape}), and LaTeX formats
pass `LATEX_ESCAPE_RULES`. The fix is to give the pipe-delimited formats an escape map for `|`.
The reproduction in `repro-test.md` fails exactly on the unescaped `|`.
""")


def fix(h, a):
    patch = sh(["git", "-C", a.upstream, "show", UPSTREAM_FIX], cwd=a.project, check=True)[1] + "\n"
    proc = subprocess.run(["git", "apply", "-"], cwd=a.project, input=patch, text=True, capture_output=True)
    if proc.returncode != 0:
        sys.exit(f"upstream fix did not apply: {proc.stderr}")
    _, stat = sh(["git", "diff", "--stat"], cwd=a.project)
    code_reg, out_reg = sh([a.python, "-m", "pytest", "-q", REGRESSION_TEST], cwd=a.project)
    code_all, out_all = sh([a.python, "-m", "pytest", "-q", "test"], cwd=a.project)
    say(h, f"applied upstream {UPSTREAM_FIX}; regression test exit {code_reg}, full suite exit {code_all}")
    h.write("change-summary.md", f"""# Change summary

Applied the upstream fix for #241 (python-tabulate {UPSTREAM_FIX}): `GITHUB_ESCAPE_RULES`
(`|` -> `\\|`) are passed to the `github` and `pipe` header and data rows.

```
{stat}
```

Before the change the reproduction failed (see `reproduce/repro-test.md`).
After the change:
- `python -m pytest -q {REGRESSION_TEST}` -> exit {code_reg}: {out_reg.splitlines()[-1]}
- `python -m pytest -q test` -> exit {code_all}: {out_all.splitlines()[-1]}
""")


def review(h, a):
    _, diff = sh(["git", "diff", "--", "tabulate/__init__.py"], cwd=a.project)
    escaped = {fmt: "\\|" in render(a.python, a.project, fmt) for fmt in ("github", "pipe")}
    tests = "\n".join(p.read_text(encoding="utf-8") for p in (Path(a.project) / "test").glob("test_*.py"))
    pipe_tested = bool(re.search(r'tablefmt="pipe"[^\n]*\n?[^\n]*\\\\\|', tests))
    findings = []
    if not pipe_tested:
        findings.append({"severity": "minor", "summary": "No regression test covers the `pipe` format, which shares the new "
                         "escape rules (verified manually: its output is escaped)."})
    outcome = "fail" if not all(escaped.values()) else ("concerns" if findings else "pass")
    say(h, f"read-only review of {len(diff.splitlines())} diff lines; escaped={escaped}; verdict {outcome}")
    h.write("review.md", f"""# Review

## Pass 1: spec compliance
- Expected behaviour (escape `|` in github and pipe): github={escaped['github']}, pipe={escaped['pipe']}.
- Root cause addressed at the row definitions, not by post-processing output.

## Pass 2: code health
- Reuses the existing `DataRow.escape_map` mechanism; no new code path.
- Findings: {len(findings)} ({', '.join(f['severity'] for f in findings) or 'none'}).
""")
    h.write("review.json", json.dumps({"outcome": outcome, "findings": findings}, indent=2))


def accept(h, a):
    say(h, f"upstream concerns in my handoff: {len(h.concerns)}")
    for concern in h.concerns:
        say(h, f"  {concern}")
    code_all, out_all = sh([a.python, "-m", "pytest", "-q", "test"], cwd=a.project)
    recorded = json.loads(
        re.search(r"```json\n(.*?)\n```", (Path(a.run) / "artifacts" / "reproduce" / "bugfix.md").read_text(), re.S).group(1)
    )
    now = baseline(a.python, a.project)
    unchanged = [fmt for fmt in UNCHANGED_FORMATS if recorded.get(fmt) == now[fmt]]
    pipe_escaped = "\\|" in render(a.python, a.project, "pipe")
    verdict = "verified" if code_all == 0 and len(unchanged) == len(UNCHANGED_FORMATS) and pipe_escaped else "failed"
    say(h, f"independent full run exit {code_all}; unchanged formats {len(unchanged)}/{len(UNCHANGED_FORMATS)}; verdict {verdict}")
    acknowledged = "\n".join(f"  - {c} -> checked: pipe output escaped = {pipe_escaped}" for c in h.concerns) or "  - none"
    h.write("final-report.md", f"""# Acceptance

- Full suite, re-run independently: `python -m pytest -q test` -> exit {code_all}: {out_all.splitlines()[-1]}
- Unchanged behaviour: {len(unchanged)}/{len(UNCHANGED_FORMATS)} formats render byte-identically to the
  baseline captured before the change ({', '.join(unchanged)}).
- Upstream concerns acknowledged:
{acknowledged}
- Verdict: **{verdict}**
""")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("stage", choices=["reproduce", "root-cause", "fix", "review", "accept"])
    parser.add_argument("--run", required=True)
    parser.add_argument("--project", required=True)
    parser.add_argument("--python", required=True)
    parser.add_argument("--upstream", required=True)
    a = parser.parse_args()
    h = Handoff(Path(a.run), a.stage)
    {"reproduce": reproduce, "root-cause": root_cause, "fix": fix, "review": review, "accept": accept}[a.stage](h, a)
    print(f"  operator runs: {h.submit}")


if __name__ == "__main__":
    main()
