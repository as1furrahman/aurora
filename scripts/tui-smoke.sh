#!/usr/bin/env bash
# Real-terminal smoke test of the TUI: the actual `aurora` binary in a tmux session, driven with
# real key presses and mouse events, asserted on what the terminal shows and on the disk.
#
# Needs tmux, the python-tabulate clone from examples/real-bugfix-demo ($DEMO_DIR) and network or a
# warm skill cache ($AURORA_CACHE_DIR). Writes only under $SMOKE_DIR (default /var/tmp/aurora-tui-smoke).
#
# Usage: scripts/tui-smoke.sh
set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DEMO="${DEMO_DIR:-/opt/aurora-demo}"
WORK="${SMOKE_DIR:-/var/tmp/aurora-tui-smoke}"
AURORA="${AURORA:-$REPO/target/debug/aurora}"
export AURORA_CACHE_DIR="${AURORA_CACHE_DIR:-$DEMO/skill-cache}"
S="aurora-tui-smoke-$$"
command -v tmux >/dev/null || { echo "tmux is required"; exit 1; }
[[ -d "$DEMO/python-tabulate/.git" ]] || { echo "run examples/real-bugfix-demo/run-demo.sh first"; exit 1; }

pass=0
step() { printf '\n== %s\n' "$*"; }
check() { if eval "$2"; then echo "  ✓ $1"; pass=$((pass + 1)); else echo "  ✗ $1"; tmux capture-pane -p -t "$S" | sed 's/^/    | /'; exit 1; fi; }
key() { tmux send-keys -t "$S" "$@"; sleep 0.4; }
type_text() { local i; for ((i = 0; i < ${#1}; i++)); do key "${1:i:1}"; done; }
screen() { tmux capture-pane -p -t "$S"; }
shows() { screen | grep -qF -- "$1"; }
wait_for() { local i; for i in $(seq 1 60); do shows "$1" && return 0; sleep 0.5; done; return 1; }
gone() { local i; for i in $(seq 1 40); do shows "$1" || return 0; sleep 0.5; done; return 1; }
click() { tmux send-keys -t "$S" -l "$(printf '\e[<0;%d;%dM\e[<0;%d;%dm' "$1" "$2" "$1" "$2")"; sleep 0.6; }
trap 'tmux kill-session -t "$S" 2>/dev/null || true' EXIT

step "Prepare a disposable project, a fresh clone to compare with, and a run root"
rm -rf "$WORK"; mkdir -p "$WORK/runs"
git clone -q "$DEMO/python-tabulate" "$WORK/project"
git clone -q "$DEMO/python-tabulate" "$WORK/clean"
(cd "$REPO" && cargo build -q -p aurora-cli)
"$AURORA" workflow init bugfix --output "$WORK/bugfix.toml" >/dev/null
"$AURORA" workflow start "$WORK/bugfix.toml" --profile pair --no-detect --run-root "$WORK/runs" --run-id fresh >/dev/null
mkdir -p "$WORK/runs/fresh/artifacts/reproduce"
echo "current, expected and unchanged behaviour" >"$WORK/runs/fresh/artifacts/reproduce/bugfix.md"
echo "a failing test" >"$WORK/runs/fresh/artifacts/reproduce/repro-test.md"
tmux new-session -d -s "$S" -x 140 -y 34 \
  "cd '$WORK/project' && '$AURORA' tui --run-root '$WORK/runs'; echo AURORA_EXIT=\$?; sleep 120"

step "1. Launch: header, tabs, the plan loads"
check "header and tabs" 'wait_for "Aurora · $WORK/project" && shows "1 Skills" && shows "4 Runs"'
check "the skills matrix appears" 'wait_for "verification-before-completion" && shows "listing budget"'

step "2. Help and filter"
key "?"
check "? opens the help, generated from the key table" 'shows "Keys (Esc closes)" && shows "Skills tab"'
key Escape
check "Esc closes it" 'gone "Keys (Esc closes)"'
key /; type_text review; key Enter
check "a kept filter narrows the matrix" 'shows "/review" && shows "code-review" && ! shows "test-driven-development"'
key Escape
check "Esc clears it" 'shows "test-driven-development"'

step "3. Roles: give review to another CLI in one key"
key 2; key j; key j; key j
key p
check "review moves to hermes" 'wait_for "pair (claude + hermes)"'
key P; key P; key P
check "profiles cycle back to pair (claude + codex)" 'wait_for "pair (claude + codex)"'

step "4. Install: review, apply, undo"
key 3
check "the plan lists the changes" 'wait_for "+ create" && shows "needs your review first: ponytail, ponytail-review"'
key a
check "apply refuses before review" 'shows "review ponytail, ponytail-review first"'
key /; type_text ponytail; key Enter
key Enter
check "Enter shows the verified file" 'wait_for "verified copy" && shows "name: ponytail"'
key Escape
key r; key j; key r
key Escape
key a
check "the confirm dialog lists exact changes and defaults to No" 'wait_for "Apply 16 changes to" && shows "the default is No"'
key Enter
check "Enter means No" 'gone "Apply 16 changes to" && [[ ! -e "$WORK/project/CLAUDE.md" ]]'
key a; key y
check "y applies: files on disk" 'wait_for "last: Applied 16 changes" && [[ -f "$WORK/project/.claude/skills/ponytail/SKILL.md" && -f "$WORK/project/CLAUDE.md" ]]'
key u; key y
check "u y undoes" 'wait_for "last: Restored"'
check "the project matches a fresh clone" 'diff -r -q --exclude=.git --exclude=.aurora --exclude=.claude --exclude=.agents "$WORK/clean" "$WORK/project"'

step "5. Runs: submit through the checkbox form"
key 4
check "the run is listed" 'wait_for "fresh" && shows "0/5"'
key Enter
check "its pipeline opens" 'shows "run fresh · bugfix" && shows "◔ running"'
key s
check "the form lists the criteria as checkboxes" 'wait_for "[ ] behaviour-recorded" && shows "[ ] reproduced"'
key Space; key j; key Space; key Enter
check "the gate passes the stage" 'wait_for "reproduce: PASSED" && wait_for "1/5"'

step "6. Mouse"
col=$(screen | sed -n 2p | python3 -I -c 'import sys; l=sys.stdin.read(); print(l.find("1 Skills") + 2)')
click "$col" 2
check "a click on a tab switches to it" 'shows "Skills × CLIs"'

step "7. Quit"
key q
check "q quits with exit 0 and the terminal is restored" 'wait_for "AURORA_EXIT=0"'

step "Done: $pass checks passed"
