#!/usr/bin/env bash
# Cross-implementation run parity (ADR-0006): the Python reference engine and the Rust engine
# operate on the SAME run directories. Proves run.json schema compatibility, byte-identical
# handoff bundles, identical status reports, and lock interoperability. Dev-only; run from
# the repo root. Skips (exit 0) when python3 is unavailable.
set -euo pipefail

command -v python3 >/dev/null || { echo "parity-runs: python3 not found; skipping"; exit 0; }
cargo build -q -p aurora-cli
RUST="$PWD/target/debug/aurora"
py() { python3 -m awb "$@"; }
EX=workflows/examples/plan-implement-review.toml
R=$(mktemp -d)
trap 'rm -rf "$R"' EXIT
fail=0
ok() { echo "ok: $*"; }
bad() { echo "FAIL: $*"; fail=1; }

same_status() {  # $1 run dir, $2 label
  py workflow status --run "$1" --json >"$R/py.json"
  "$RUST" workflow status --run "$1" --json >"$R/rs.json"
  if python3 -I -c 'import json,sys; sys.exit(json.load(open(sys.argv[1])) != json.load(open(sys.argv[2])))' "$R/py.json" "$R/rs.json"; then
    ok "identical status JSON ($2)"
  else
    bad "status JSON differs ($2)"; diff <(python3 -I -m json.tool --sort-keys "$R/py.json") <(python3 -I -m json.tool --sort-keys "$R/rs.json") | head -20 || true
  fi
}

expect_code() {  # $1 expected, rest command
  local want=$1; shift
  set +e; "$@" >/dev/null 2>&1; local got=$?; set -e
  [[ $got -eq $want ]] && ok "exit $got: ${*: -6}" || bad "exit $got (wanted $want): $*"
}

# --- A: Python starts, Rust submits, Python verifies the Rust-written bundle -------------
py workflow start "$EX" --run-root "$R" --run-id a --no-detect --json >/dev/null
A="$R/a"
expect_code 1 "$RUST" workflow submit --run "$A" --stage plan --met plan-written
echo "the plan" >"$A/artifacts/plan/plan.md"
expect_code 0 "$RUST" workflow submit --run "$A" --stage plan --met plan-written
same_status "$A" "python-started run after rust submits"
warnings=$(py workflow resume --run "$A" --json | python3 -I -c 'import json,sys; print(len([w for w in json.load(sys.stdin)["warnings"] if "not overwritten" in w]))')
[[ $warnings -eq 0 ]] && ok "python accepts the rust-written implement bundle byte-for-byte" || bad "python sees the rust bundle as edited"

# --- B: Rust starts (single provider), implementations alternate to completion -----------
"$RUST" workflow start "$EX" --run-root "$R" --run-id b --no-detect --provider claude >/dev/null
B="$R/b"
produce() { echo "evidence for $2" >"$B/artifacts/$1/$2"; }
produce plan plan.md;                expect_code 0 py      workflow submit --run "$B" --stage plan --met plan-written
produce implement change-summary.md; expect_code 0 "$RUST" workflow submit --run "$B" --stage implement --met change-described
produce test test-report.md;         expect_code 0 py      workflow submit --run "$B" --stage test --met suite-green
produce review review.md;            expect_code 0 "$RUST" workflow submit --run "$B" --stage review --met review-complete
echo "No findings." >"$B/artifacts/remediate/NOT_APPLICABLE.md"
expect_code 0 py workflow submit --run "$B" --stage remediate --not-applicable
produce final-verify final-report.md; expect_code 0 "$RUST" workflow submit --run "$B" --stage final-verify --met final-suite-green
expect_code 0 py workflow approve --run "$B" --stage final-verify --by parity
same_status "$B" "rust-started run completed by both"
complete=$("$RUST" workflow status --run "$B" --json | python3 -I -c 'import json,sys; r=json.load(sys.stdin); print(r["complete"], r["reduced_independence"])')
[[ $complete == "True ['final-verify', 'review']" ]] && ok "complete with reduced independence reported" || bad "unexpected completion: $complete"

# --- C: start bundles are identical when each implementation starts its own run ---------
py workflow start "$EX" --run-root "$R" --run-id c1 --no-detect >/dev/null
"$RUST" workflow start "$EX" --run-root "$R" --run-id c2 --no-detect >/dev/null
if diff <(sed "s#$R/c1#RUN#g; s#Run: c1#Run: ID#" "$R/c1/handoff/plan.md") \
        <(sed "s#$R/c2#RUN#g; s#Run: c2#Run: ID#" "$R/c2/handoff/plan.md") >/dev/null; then
  ok "start bundles identical across implementations"
else
  bad "start bundles differ"
fi

# --- D: a Python-held lock makes the Rust writer wait --------------------------------------
python3 -c "
import sys, time; sys.path.insert(0, '.')
from awb.safety.locking import ProcessLock
with ProcessLock('$A/.awb.lock', allowed_roots=['$A']):
    open('$R/locked', 'w').close(); time.sleep(2)
" &
holder=$!
while [[ ! -e "$R/locked" ]]; do sleep 0.05; done
start=$(date +%s.%N)
"$RUST" workflow resume --run "$A" >/dev/null
waited=$(python3 -I -c "print(round($(date +%s.%N) - $start, 2))")
wait $holder
python3 -I -c "import sys; sys.exit(0 if $waited >= 1.5 else 1)" && ok "rust waited ${waited}s for the python lock" || bad "rust did not wait for the python lock (${waited}s)"

exit $fail
