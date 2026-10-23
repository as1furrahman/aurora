#!/usr/bin/env bash
# Real-project dry run of Aurora's workflow engine.
#
# A maintainer triages python-tabulate issue #241 ("pipe characters break Markdown tables")
# with the built-in `bugfix` preset, spreading the roles across four coding CLIs:
#   reproduce -> codex  |  root-cause -> hermes  |  fix -> antigravity  |  review -> claude  |  accept -> codex
#
# Real: the installed CLIs (detected on PATH), the project and its history, the upstream fix
# (commit e6a24aa), every test run, and every Aurora decision. Simulated: only the agents'
# reasoning (stand_in.py), so no CLI login is needed. Each step asserts what Aurora must do.
#
# Usage: examples/real-bugfix-demo/run-demo.sh   (env: DEMO_DIR, default /opt/aurora-demo)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DEMO="${DEMO_DIR:-/opt/aurora-demo}"
AURORA="${AURORA:-$REPO/target/debug/aurora}"
RUN_ROOT="$DEMO/runs"
RUN="$RUN_ROOT/issue-241"
WF="$DEMO/bugfix.toml"
REG="$HERE/demo-registry.toml"
PY="$DEMO/venv/bin/python"
export PYTHONDONTWRITEBYTECODE=1   # stale .pyc files once produced a false test result here

ROUTES=(--route reproduce=codex --route root-cause=hermes --route fix=antigravity --route review=claude --route accept=codex)
pass=0
step() { printf '\n== %s\n' "$*"; }
check() { if eval "$2"; then echo "  ✓ $1"; pass=$((pass + 1)); else echo "  ✗ $1"; exit 1; fi; }
code() { set +e; "$@" >"$DEMO/last.out" 2>&1; local c=$?; set -e; cat "$DEMO/last.out" | sed 's/^/    /'; return $c; }
status_json() { "$AURORA" workflow status --run "$RUN" --json; }
jq_py() { python3 -I -c "import json,sys; d=json.load(sys.stdin); print($1)"; }
stand_in() { python3 -I "$HERE/stand_in.py" "$1" --run "$RUN" --project "$DEMO/project" --python "$PY" --upstream "$DEMO/python-tabulate"; }
submit() { local stage=$1; shift; code "$AURORA" workflow submit --run "$RUN" --stage "$stage" "$@"; }

step "Prepare the real project (python-tabulate, one commit before the upstream fix for #241)"
mkdir -p "$DEMO" "$RUN_ROOT"
[[ -d "$DEMO/python-tabulate/.git" ]] || git clone -q https://github.com/astanin/python-tabulate.git "$DEMO/python-tabulate"
rm -rf "$DEMO/project" "$RUN" "$WF"
git clone -q "$DEMO/python-tabulate" "$DEMO/project"
git -C "$DEMO/project" checkout -q -b demo/issue-241 e6a24aa^
[[ -x "$PY" ]] || { python3 -m venv "$DEMO/venv" && "$PY" -m pip -q install pytest wcwidth; }
(cd "$REPO" && cargo build -q -p aurora-cli)
echo "  project at $(git -C "$DEMO/project" log --oneline -1)"

step "1. Which coding CLIs are installed? (PATH detection only; nothing is launched)"
code "$AURORA" providers
check "all four CLIs detected" '[[ $("$AURORA" providers --json | jq_py "sum(p[\"installed\"] for p in d)") == 4 ]]'

step "2. Start from the built-in bugfix preset"
code "$AURORA" workflow init bugfix --output "$WF"
check "preset written" '[[ -f "$WF" ]]'

step "3. Route root-cause to Hermes before allowing it in the workflow: refused"
code "$AURORA" workflow plan "$WF" --registry "$REG" --no-detect "${ROUTES[@]}" && c=0 || c=$?
check "ineligible provider is a clear error (exit 1)" '[[ $c -eq 1 ]] && grep -q "not eligible" "$DEMO/last.out"'
# Presets are meant to be edited: allow hermes to play the implementer role.
sed -i '/^\[routing.roles.implementer\]/,/^$/ s/eligible = \["claude", "codex", "antigravity"\]/eligible = ["claude", "codex", "antigravity", "hermes"]/' "$WF"
check "implementer role now lists hermes" 'grep -A1 "routing.roles.implementer" "$WF" | grep -q hermes'

step "4. Plan against the shipped registry: Hermes cannot load the required skills"
code "$AURORA" workflow plan "$WF" --no-detect "${ROUTES[@]}" && c=0 || c=$?
check "plan blocked (exit 3), no lossy conversion" '[[ $c -eq 3 ]] && grep -q "incompatible" "$DEMO/last.out"'

step "5. Plan with the team registry that declares Hermes compatibility"
code "$AURORA" workflow plan "$WF" --registry "$REG" --no-detect "${ROUTES[@]}" && c=0 || c=$?
check "plan assignable (exit 0)" '[[ $c -eq 0 ]]'
check "review is independent of fix (different CLI)" 'grep -q "review vs fix: distinct-provider" "$DEMO/last.out"'
check "claude enforces read_only hard; reviewer handoff will say how" \
  '"$AURORA" workflow plan "$WF" --registry "$REG" --no-detect --json "${ROUTES[@]}" | jq_py "[s for s in d[\"stages\"] if s[\"id\"]==\"review\"][0][\"enforcement\"][0][\"strength\"]" | grep -q hard'

step "6. Start the run"
code "$AURORA" workflow start "$WF" --registry "$REG" --run-root "$RUN_ROOT" --run-id issue-241 --no-detect "${ROUTES[@]}"
check "run created; only reproduce has a handoff" '[[ -f "$RUN/handoff/reproduce.md" && ! -e "$RUN/handoff/fix.md" ]]'

step "7. reproduce (codex): the operator forgets to claim one criterion"
stand_in reproduce
submit reproduce --met behaviour-recorded && c=0 || c=$?
check "gate refuses: criterion not claimed (exit 1, FAILED)" '[[ $c -eq 1 ]] && grep -q "criterion '"'"'reproduced'"'"' not claimed" "$DEMO/last.out"'
submit reproduce --met behaviour-recorded --met reproduced
check "second attempt passes" 'grep -q "PASSED" "$DEMO/last.out"'
check "the real regression test failed before the fix" 'grep -q "Exit code: 1" "$RUN/artifacts/reproduce/repro-test.md"'

step "8. root-cause (hermes)"
stand_in root-cause
submit root-cause --met cause-evidenced
check "root-cause passed" 'grep -q "PASSED" "$DEMO/last.out"'

step "9. fix (antigravity): apply the real upstream fix, run the real suite"
stand_in fix
submit fix --met repro-passes
check "fix passed" 'grep -q "PASSED" "$DEMO/last.out"'
check "real suite green after the fix" 'grep -qF -- "-q test\` -> exit 0" "$RUN/artifacts/fix/change-summary.md"'

step "10. review (claude, read-only): structured verdict"
check "reviewer handoff carries the read-only safeguard" 'grep -q "Required safeguards" "$RUN/handoff/review.md" && grep -q "permission-mode plan" "$RUN/handoff/review.md"'
stand_in review
submit review --met fix-reviewed
check "verdict 'concerns' passes the gate" 'grep -q "PASSED" "$DEMO/last.out" && [[ $(status_json | jq_py "d.get(\"concerns\")") == "['"'"'review'"'"']" ]]'

step "11. accept (codex): sees the concern, verifies independently; a human approves"
check "accept handoff lists the open concern" 'grep -q "Open concerns from upstream stages" "$RUN/handoff/accept.md"'
stand_in accept
submit accept --met verified
check "gate satisfied, awaiting human approval" 'grep -q "awaiting approval" "$DEMO/last.out"'
check "acceptance verdict is verified" 'grep -q "Verdict: \*\*verified\*\*" "$RUN/artifacts/accept/final-report.md"'
code "$AURORA" workflow approve --run "$RUN" --stage accept --by maintainer
code "$AURORA" workflow status --run "$RUN"
check "run COMPLETE" '[[ $(status_json | jq_py "d[\"complete\"]") == True ]]'
check "no reduced independence (four distinct CLIs)" '[[ $(status_json | jq_py "d[\"reduced_independence\"]") == "[]" ]]'

step "12. Integrity: someone edits accepted evidence after approval"
cp "$RUN/artifacts/review/review.md" "$DEMO/review.md.bak"
echo "edited later" >>"$RUN/artifacts/review/review.md"
code "$AURORA" workflow status --run "$RUN"
check "drift detected; run no longer complete" '[[ $(status_json | jq_py "d[\"complete\"], len(d[\"drift\"])") == "False 1" ]]'
cp "$DEMO/review.md.bak" "$RUN/artifacts/review/review.md"
check "restored evidence: complete again" '[[ $(status_json | jq_py "d[\"complete\"]") == True ]]'

step "Done: $pass checks passed"
echo "  run directory: $RUN"
echo "  events recorded: $(python3 -I -c "import json; print(len(json.load(open('$RUN/run.json'))['events']))")"
