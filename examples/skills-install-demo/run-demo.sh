#!/usr/bin/env bash
# Real-CLI dry run of Aurora's skill installs (R2 Step 6). No CLI login, no model call.
#
# A maintainer prepares a local Rust sample project for the bugfix workflow with the common two-CLI setup
# (Claude Code builds, Codex tests and reviews), then tries a Hermes-only setup:
#   recommend -> plan (writes nothing) -> review gate -> apply -> the real CLIs load the skills
#   -> workflow handoffs name them -> undo restores the project exactly.
#
# Real: the catalog skills fetched from GitHub at their pinned commits and the installed files.
# When present, Codex's prompt input and Hermes' skill index are checked too. Claude Code has no
# offline skill listing, so its copies are checked with `aurora skills inspect`.
# The target is a Rust sample repository; this script does not run its tests.
#
# Usage: examples/skills-install-demo/run-demo.sh   (env: DEMO_DIR, default: system temp dir/aurora-demo)
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DEMO="${DEMO_DIR:-${TMPDIR:-/tmp}/aurora-demo}"
AURORA="${AURORA:-$REPO/target/debug/aurora}"
export AURORA_CACHE_DIR="${AURORA_CACHE_DIR:-$DEMO/skill-cache}"
FIXTURE_SOURCE="$REPO/examples/real-bugfix-demo/rust-table-fixture"
FIXTURE_REPO="$DEMO/rust-table-fixture"
PAIR="$DEMO/skills-pair"
SOLO="$DEMO/skills-hermes"
RUNS="$DEMO/skills-runs"
REVIEWED=(--reviewed ponytail --reviewed ponytail-review)

pass=0
step() { printf '\n== %s\n' "$*"; }
check() { if eval "$2"; then echo "  ✓ $1"; pass=$((pass + 1)); else echo "  ✗ $1"; exit 1; fi; }
code() { set +e; "$@" >"$DEMO/last.out" 2>&1; local c=$?; set -e; sed 's/^/    /' "$DEMO/last.out" | head -40; return $c; }
fresh() { rm -rf "$1"; git clone -q "$FIXTURE_REPO" "$1"; }
tree_state() { (cd "$1" && find . -path ./.git -prune -o -print | LC_ALL=C sort | while read -r f; do [[ -f $f ]] && sha256sum "$f" || echo "dir $f"; done); }
# The project as a fresh clone would have it: ignore Aurora's journal and the (now empty) skill dirs.
same_as_clone() { diff -r -q --exclude=.git --exclude=.aurora --exclude=.claude --exclude=.agents --exclude=.hermes "$DEMO/clean" "$1"; }

step "Prepare (local Rust fixture, fresh projects, aurora build)"
mkdir -p "$DEMO" "$AURORA_CACHE_DIR"
if [[ ! -d "$FIXTURE_REPO/.git" ]]; then
  cp -a "$FIXTURE_SOURCE" "$FIXTURE_REPO"
  git -C "$FIXTURE_REPO" init -q
  git -C "$FIXTURE_REPO" add -A
  git -C "$FIXTURE_REPO" -c user.name=Aurora -c user.email=aurora@example.invalid commit -qm "Initial Rust demo fixture"
fi
fresh "$DEMO/clean"; fresh "$PAIR"; fresh "$SOLO"; rm -rf "$RUNS"; mkdir -p "$RUNS"
(cd "$REPO" && cargo build -q -p aurora-cli)

step "1. What should each CLI use for a bug fix? (pair: claude + codex)"
code "$AURORA" catalog recommend --preset bugfix
check "review runs on codex with a code-review skill" 'grep -qE "^  review +reviewer +codex .*code-review" "$DEMO/last.out"'

step "2. Plan the install: a dry run that writes nothing"
before=$(tree_state "$PAIR")
code "$AURORA" skills plan --preset bugfix --project "$PAIR"
check "14 skill copies and 2 rule blocks planned" 'grep -q "would be written (14 installed copies, 2 rule blocks)" "$DEMO/last.out"'
check "the project is untouched" '[[ "$(tree_state "$PAIR")" == "$before" ]]'

step "3. Apply without reviewing the two community skills: refused"
code "$AURORA" skills apply --preset bugfix --project "$PAIR" --yes && c=0 || c=$?
check "review gate names what to read (exit 1)" '[[ $c -eq 1 ]] && grep -q -- "--reviewed ponytail" "$DEMO/last.out"'
check "still untouched" '[[ "$(tree_state "$PAIR")" == "$before" ]]'

step "4. Apply after review"
code "$AURORA" skills apply --preset bugfix --project "$PAIR" --yes "${REVIEWED[@]}"
check "applied, with an undo hint" 'grep -q "Applied 16 changes" "$DEMO/last.out" && grep -q "aurora skills undo" "$DEMO/last.out"'
check "claude's skills in .claude/skills, codex's in .agents/skills" \
  '[[ -f $PAIR/.claude/skills/test-driven-development/SKILL.md && -f $PAIR/.agents/skills/code-review/SKILL.md && ! -e $PAIR/.agents/skills/test-driven-development ]]'
check "no installed skill pre-approves tools (allowed-tools stripped)" '! grep -l "^allowed-tools" $PAIR/.claude/skills/*/SKILL.md $PAIR/.agents/skills/*/SKILL.md'
check "karpathy rules projected into CLAUDE.md and AGENTS.md" 'grep -q "aurora:begin karpathy-guidelines" "$PAIR/CLAUDE.md" && grep -q "aurora:begin karpathy-guidelines" "$PAIR/AGENTS.md"'
code "$AURORA" skills plan --preset bugfix --project "$PAIR"
check "re-plan: everything unchanged" '! grep -qE "^  (create|update|refuse) " "$DEMO/last.out" && grep -q "aurora.lock: unchanged" "$DEMO/last.out"'
for d in "$PAIR"/.claude/skills/*/; do "$AURORA" skills inspect "$d" >/dev/null || { echo "  inspect failed: $d"; exit 1; }; done
check "every Claude copy passes the Agent Skills spec check" 'true'

if command -v codex >/dev/null 2>&1; then
  step "5. Codex (offline) sees exactly its role's skills and the rules"
  (cd "$PAIR" && codex debug prompt-input >"$DEMO/codex-prompt.json" 2>"$DEMO/codex-prompt.err")
  check "codex lists code-review, find-bugs, reproduce-bug" 'for s in code-review find-bugs reproduce-bug; do grep -q "$s" "$DEMO/codex-prompt.json" || exit 1; done'
  check "codex does not see claude-only skills" '! grep -q "test-driven-development" "$DEMO/codex-prompt.json" && ! grep -q "diagnosing-bugs" "$DEMO/codex-prompt.json"'
  check "codex has the karpathy rules from AGENTS.md" 'grep -q "karpathy" "$DEMO/codex-prompt.json"'
else
  echo "  - Codex CLI absent; skipped its live prompt-input checks"
fi

step "6. Workflow handoffs name the installed skills in each CLI's syntax"
"$AURORA" workflow init bugfix --output "$DEMO/bugfix.toml" >/dev/null 2>&1 || true
code "$AURORA" workflow start "$DEMO/bugfix.toml" --profile pair --no-detect --run-root "$RUNS" --run-id pair
check "reproduce handoff (codex) says \$reproduce-bug" 'grep -q "Use for this stage: .*\`\$reproduce-bug\`" "$RUNS/pair/handoff/reproduce.md"'
check "every skill it names is installed where codex reads" \
  'for s in $(grep "Use for this stage" "$RUNS/pair/handoff/reproduce.md" | grep -oE "\\\$[a-z0-9-]+" | tr -d "$"); do [[ -d $PAIR/.agents/skills/$s ]] || exit 1; done'

step "7. Install skills for a Hermes-only setup"
code "$AURORA" skills apply --preset bugfix --clis hermes --project "$SOLO" --yes "${REVIEWED[@]}"
check "installed into .hermes/skills" '[[ -f $SOLO/.hermes/skills/code-review/SKILL.md ]] && grep -q "hermes skills trust" "$DEMO/last.out"'
if command -v hermes >/dev/null 2>&1; then
  (cd "$SOLO" && hermes skills trust >/dev/null 2>&1 && hermes skills list >"$DEMO/hermes-list.txt" 2>&1)
  check "hermes lists all 11 as enabled project skills" '[[ $(grep -cE "│ project │ .* │ enabled" "$DEMO/hermes-list.txt") -eq 11 ]]'
  (cd "$SOLO" && hermes skills untrust >/dev/null 2>&1 || true)
else
  echo "  - Hermes CLI absent; skipped its live skill-index check"
fi

step "8. Undo both installs: the projects are back to a fresh clone"
code "$AURORA" skills undo --project "$PAIR"
check "pair project restored" 'same_as_clone "$PAIR"'
code "$AURORA" skills undo --project "$SOLO"
check "hermes project restored" 'same_as_clone "$SOLO"'
check "nothing deleted: removed copies were moved aside" '[[ $(find "$PAIR/.aurora/ops" -name SKILL.md | wc -l) -eq 14 ]]'

step "Done: $pass checks passed"
