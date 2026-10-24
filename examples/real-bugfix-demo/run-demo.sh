#!/usr/bin/env bash
# Real-project dry run of Aurora's workflow engine.
#
# A maintainer triages a Rust Markdown-table bug ("pipe characters break Markdown tables")
# with the built-in `bugfix` preset, spreading the roles across four coding CLIs:
#   reproduce -> codex  |  root-cause -> hermes  |  fix -> antigravity  |  review -> claude  |  accept -> codex
#
# Real: provider detection on PATH, the Rust project, its regression tests, the fix patch, and
# every Aurora decision. Simulated: the agents' reasoning (stand_in.rs), so no CLI login is needed.
#
# Usage: examples/real-bugfix-demo/run-demo.sh   (env: DEMO_DIR, default: system temp dir/aurora-demo)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DEMO="${DEMO_DIR:-${TMPDIR:-/tmp}/aurora-demo}"
AURORA="${AURORA:-$REPO/target/debug/aurora}"
RUN_ROOT="$DEMO/runs"
RUN="$RUN_ROOT/issue-241"
WF="$DEMO/bugfix.toml"
REG="$HERE/demo-registry.toml"
ROUTES=(--route reproduce=codex --route root-cause=hermes --route fix=antigravity --route review=claude --route accept=codex)
pass=0
step() { printf '\n== %s\n' "$*"; }
check() { if eval "$2"; then echo "  ✓ $1"; pass=$((pass + 1)); else echo "  ✗ $1"; exit 1; fi; }
code() { set +e; "$@" >"$DEMO/last.out" 2>&1; local c=$?; set -e; cat "$DEMO/last.out" | sed 's/^/    /'; return $c; }
status_json() { "$AURORA" workflow status --run "$RUN" --json; }
stand_in() { "$DEMO/stand-in" "$1" --run "$RUN" --project "$DEMO/project" --patch "$HERE/rust-table-fixture/upstream-fix.patch"; }
submit() { local stage=$1; shift; code "$AURORA" workflow submit --run "$RUN" --stage "$stage" "$@"; }

step "Prepare the Rust project with its intentional regression"
mkdir -p "$DEMO" "$RUN_ROOT"
rm -rf "$DEMO/project" "$RUN" "$WF"
cp -a "$HERE/rust-table-fixture" "$DEMO/project"
rustc --edition=2024 --deny warnings "$HERE/stand_in.rs" -o "$DEMO/stand-in"
(cd "$REPO" && cargo build -q -p aurora-cli)
echo "  project at $DEMO/project"

step "1. Which coding CLIs are installed? (PATH detection only; nothing is launched)"
code "$AURORA" providers
check "all four supported CLIs are listed" '[[ $("$AURORA" providers --json | grep -c "\"id\":") == 4 ]]'

step "2. Start from the built-in bugfix preset"
code "$AURORA" workflow init bugfix --output "$WF"
check "preset written" '[[ -f "$WF" ]]'

step "3. Any CLI may play any role; the catalog's pair profile routes the whole workflow by itself"
check "the preset lets hermes play the implementer role" 'grep -A1 "routing.roles.implementer" "$WF" | grep -q hermes'
code "$AURORA" workflow plan "$WF" --profile pair --no-detect && c=0 || c=$?
check "pair profile: claude builds, codex tests and reviews" \
  '[[ $("$AURORA" workflow plan "$WF" --profile pair --no-detect --json | grep "\"provider\":" | cut -d "\"" -f4 | paste -sd" " -) == "codex claude claude codex codex" ]]'

step "4. Plan against the shipped registry: Hermes cannot load the required skills"
code "$AURORA" workflow plan "$WF" --no-detect "${ROUTES[@]}" && c=0 || c=$?
check "plan blocked (exit 3), no lossy conversion" '[[ $c -eq 3 ]] && grep -q "incompatible" "$DEMO/last.out"'
check "the block says how to fix it" 'grep -q "fix: route the stage" "$DEMO/last.out" && grep -q -- "--registry FILE" "$DEMO/last.out"'

step "5. Plan with the team registry that declares Hermes compatibility"
code "$AURORA" workflow plan "$WF" --registry "$REG" --no-detect "${ROUTES[@]}" && c=0 || c=$?
check "plan assignable (exit 0)" '[[ $c -eq 0 ]]'
check "review is independent of fix (different CLI)" 'grep -q "review vs fix: distinct-provider" "$DEMO/last.out"'
check "claude enforces read_only hard; reviewer handoff will say how" \
  '"$AURORA" workflow plan "$WF" --registry "$REG" --no-detect --json "${ROUTES[@]}" | grep -q "\"strength\": \"hard\""'

step "6. Start the run"
code "$AURORA" workflow start "$WF" --registry "$REG" --run-root "$RUN_ROOT" --run-id issue-241 --no-detect "${ROUTES[@]}"
check "run created; only reproduce has a handoff" '[[ -f "$RUN/handoff/reproduce.md" && ! -e "$RUN/handoff/fix.md" ]]'

step "7. reproduce (codex): the operator forgets to claim one criterion"
stand_in reproduce
submit reproduce --met behaviour-recorded && c=0 || c=$?
check "gate refuses: criterion not claimed (exit 1, FAILED)" '[[ $c -eq 1 ]] && grep -q "criterion '"'"'reproduced'"'"' not claimed" "$DEMO/last.out"'
submit reproduce --met behaviour-recorded --met reproduced
check "second attempt passes" 'grep -q "PASSED" "$DEMO/last.out"'
check "the Rust regression tests failed before the fix" 'grep -q "Exit code: 101" "$RUN/artifacts/reproduce/repro-test.md"'

step "8. root-cause (hermes)"
stand_in root-cause
submit root-cause --met cause-evidenced
check "root-cause passed" 'grep -q "PASSED" "$DEMO/last.out"'

step "9. fix (antigravity): apply the patch and run the Rust suite"
stand_in fix
submit fix --met repro-passes
check "fix passed" 'grep -q "PASSED" "$DEMO/last.out"'
check "Rust fixture suite green after the fix" 'grep -q "cargo test.*passed" "$RUN/artifacts/fix/change-summary.md"'

step "10. review (claude, read-only): structured verdict"
check "reviewer handoff carries the read-only safeguard" 'grep -q "Required safeguards" "$RUN/handoff/review.md" && grep -q "permission-mode plan" "$RUN/handoff/review.md"'
stand_in review
submit review --met fix-reviewed
check "review verdict passes the gate" 'grep -q "PASSED" "$DEMO/last.out"'

step "11. accept (codex): verifies independently; a human approves"
stand_in accept
submit accept --met verified
check "gate satisfied, awaiting human approval" 'grep -q "awaiting approval" "$DEMO/last.out"'
check "acceptance verdict is verified" 'grep -q "Verdict: \*\*verified\*\*" "$RUN/artifacts/accept/final-report.md"'
code "$AURORA" workflow approve --run "$RUN" --stage accept --by maintainer
code "$AURORA" workflow status --run "$RUN"
check "run COMPLETE" 'status_json | grep -q "\"complete\": true"'
check "no reduced independence (four distinct CLIs)" '! grep -q "reduced independence" "$DEMO/last.out"'

step "12. Integrity: someone edits accepted evidence after approval"
cp "$RUN/artifacts/review/review.md" "$DEMO/review.md.bak"
echo "edited later" >>"$RUN/artifacts/review/review.md"
code "$AURORA" workflow status --run "$RUN"
check "drift detected; run no longer complete" 'status_json | grep -q "\"complete\": false" && grep -q "\[DRIFT\]" "$DEMO/last.out"'
cp "$DEMO/review.md.bak" "$RUN/artifacts/review/review.md"
check "restored evidence: complete again" 'status_json | grep -q "\"complete\": true"'

step "Done: $pass checks passed"
echo "  run directory: $RUN"
echo "  events recorded: $(grep -c "\"seq\":" "$RUN/run.json")"
