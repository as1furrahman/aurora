#!/usr/bin/env bash
# Live CLI parity: run the Python reference CLI (python3 -m awb) and the Rust CLI on the same
# inputs and require identical JSON output and exit codes (ADR-0006). Dev-only; run from the
# repo root. Skips (exit 0) when python3 is unavailable.
set -euo pipefail

command -v python3 >/dev/null || { echo "parity: python3 not found; skipping"; exit 0; }
cargo build -q -p aurora-cli
RUST=target/debug/aurora
EX=workflows/examples/plan-implement-review.toml
MIN=tests/fixtures/parity/minimal.toml
FREG=tests/fixtures/parity/fixture-registry.toml

cases=(
  "providers --json"
  "workflow validate $EX --json"
  "workflow validate tests/fixtures/workflows/provider-in-role.toml --json"
  "workflow validate tests/fixtures/workflows/cycle.toml --json"
  "workflow validate $MIN --json"
  "workflow validate $MIN --registry $FREG --json"
  "workflow show $EX --json"
  "workflow show tests/fixtures/workflows/parallel.toml --registry $FREG --json"
  "workflow plan $EX --no-detect --json"
  "workflow plan $EX --json"
  "workflow plan $EX --no-detect --json --provider claude"
  "workflow plan $EX --no-detect --json --provider codex"
  "workflow plan $EX --no-detect --json --provider antigravity"
  "workflow plan $EX --no-detect --json --route implement=codex"
  "workflow plan $EX --no-detect --json --route implement=codex --route plan=codex"
  "workflow plan $MIN --registry $FREG --no-detect --json --provider antigravity"
  "workflow plan tests/fixtures/parity/minimal-ag-fallback.toml --registry $FREG --no-detect --json"
  "workflow plan tests/fixtures/workflows/parallel.toml --registry $FREG --no-detect --json"
)

fail=0
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
for args in "${cases[@]}"; do
  # shellcheck disable=SC2086
  set +e
  python3 -m awb $args >"$tmp/py.json" 2>"$tmp/py.err"; py=$?
  "$RUST" $args >"$tmp/rs.json" 2>"$tmp/rs.err"; rs=$?
  set -e
  if [[ $py -ne $rs ]]; then
    echo "DIFF exit code ($py vs $rs): $args"; fail=1; continue
  fi
  if python3 -I - "$tmp/py.json" "$tmp/rs.json" "$args" <<'PY'
import json, sys
a, b = (json.load(open(p)) for p in sys.argv[1:3])
if sys.argv[3].startswith("providers"):
    # Rust knows more providers (hermes) and may forbid more flags; compare the providers
    # both implementations know, requiring Rust's forbidden flags to be a superset.
    known = {p["id"] for p in a}
    b = [p for p in b if p["id"] in known]
    for pa, pb in zip(a, b):
        if not set(pa.pop("forbidden_flags")) <= set(pb.pop("forbidden_flags")):
            sys.exit(1)
        pb.pop("enforcement", None)
if isinstance(a, dict) and isinstance(a.get("detection"), dict) and isinstance(b.get("detection"), dict):
    b["detection"] = {k: v for k, v in b["detection"].items() if k in a["detection"]}
sys.exit(0 if a == b else 1)
PY
  then
    echo "same (exit $py): $args"
  else
    echo "DIFF json: $args"; diff <(python3 -I -m json.tool --sort-keys "$tmp/py.json") \
      <(python3 -I -m json.tool --sort-keys "$tmp/rs.json") | head -20 || true
    fail=1
  fi
done
exit $fail
