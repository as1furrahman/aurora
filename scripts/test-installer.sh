#!/usr/bin/env bash
# End-to-end test of the release installer, without publishing anything: serves the artifacts
# from `dist build` on a local stand-in for GitHub releases, then
#   1. installs with `curl ... | sh` into an empty home (no Rust, minimal PATH),
#   2. checks that a tampered download is refused,
#   3. runs the installed `aurora` in a fresh project (tmux) and sees the guided setup.
# Usage: dist build --artifacts=all [-t <host target>] && scripts/test-installer.sh
set -u
REPO="$(cd "$(dirname "$0")/.." && pwd)"
DIST="$REPO/target/distrib"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$REPO/Cargo.toml" | head -1)"
SLUG="as1furrahman/aurora"
PORT="${PORT:-8765}"
T="${TMPDIR:-/tmp}/aurora-installer-test"
command -v tmux >/dev/null || { echo "tmux is needed"; exit 2; }
[ -f "$DIST/aurora-cli-installer.sh" ] || { echo "run 'dist build --artifacts=all' first"; exit 2; }
ARCHIVE="$(cd "$DIST" && ls aurora-cli-*-unknown-linux-gnu.tar.xz aurora-cli-*-apple-darwin.tar.xz 2>/dev/null | head -1)"

rm -rf "$T"; REL="$T/srv/$SLUG/releases/download/v$VERSION"; mkdir -p "$REL"
cp "$DIST"/*.tar.xz "$DIST"/*.sha256 "$DIST"/sha256.sum "$DIST"/aurora-cli-installer.sh "$REL/"
rustc --edition=2024 --deny warnings "$REPO/scripts/local_http_server.rs" -o "$T/local-http-server" || exit 2
"$T/local-http-server" "$T/srv" "127.0.0.1:$PORT" >"$T/http.log" 2>&1 &
SRV=$!
trap 'kill $SRV 2>/dev/null; tmux kill-session -t aurora-installer-test 2>/dev/null' EXIT
sleep 1
# Our server must be the one answering: a stale server on the port would serve old files.
kill -0 "$SRV" 2>/dev/null || { echo "cannot serve on port $PORT (in use?): $(tail -1 "$T/http.log")"; exit 2; }
BASE="http://127.0.0.1:$PORT"
URL="$BASE/$SLUG/releases/download/v$VERSION/aurora-cli-installer.sh"
pass=0; fail=0
check() { if eval "$2"; then echo "  ✓ $1"; pass=$((pass+1)); else echo "  ✗ $1"; fail=$((fail+1)); fi; }
install_into() { # home, log
  curl -LsSf "$URL" | env -i HOME="$1" PATH=/usr/bin:/bin SHELL=/bin/bash \
    AURORA_CLI_INSTALLER_GITHUB_BASE_URL="$BASE" sh >"$2" 2>&1
}

echo "== 1. curl | sh into an empty home"
H="$T/home"; mkdir -p "$H"
install_into "$H" "$T/install.log"
check "the installer exits 0" '[ $? -eq 0 ]'
check "aurora lands in ~/.local/bin" '[ -x "$H/.local/bin/aurora" ]'
check "it reports version $VERSION" '"$H/.local/bin/aurora" --version | grep -q "aurora $VERSION"'
check "a new login shell finds aurora on PATH" 'env -i HOME="$H" PATH=/usr/bin:/bin bash -lc "command -v aurora" | grep -q "$H/.local/bin/aurora"'

echo "== 2. a tampered download is refused"
H2="$T/home-tampered"; mkdir -p "$H2"
cp "$REL/$ARCHIVE" "$T/good.tar.xz"; printf 'x' >>"$REL/$ARCHIVE"
install_into "$H2" "$T/install-tampered.log"
check "the installer fails" '[ $? -ne 0 ]'
check "nothing is installed" '[ ! -e "$H2/.local/bin/aurora" ]'
check "it reports a checksum mismatch" 'grep -qi "checksum mismatch" "$T/install-tampered.log"'
cp "$T/good.tar.xz" "$REL/$ARCHIVE"

echo "== 3. the installed aurora opens the guided setup"
P="$T/my-app"; mkdir -p "$P" && git -C "$P" init -q
tmux new-session -d -s aurora-installer-test -x 100 -y 30 \
  "cd '$P' && env -i HOME='$H' TERM=xterm-256color PATH='$H/.local/bin:/usr/bin:/bin' aurora; sleep 30"
sleep 2
check "the setup asks which AI tools you use" 'tmux capture-pane -p -t aurora-installer-test | grep -q "Which AI tools do you use?"'
tmux send-keys -t aurora-installer-test Escape; sleep 0.5
check "Esc shows the pages for this project" 'tmux capture-pane -p -t aurora-installer-test | grep -q "Aurora · my-app"'
tmux send-keys -t aurora-installer-test q; sleep 0.5
check "looking wrote nothing into the project" '[ -z "$(git -C "$P" status --porcelain)" ]'

echo "== Done: $pass passed, $fail failed"
[ "$fail" -eq 0 ]
