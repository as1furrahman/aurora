#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
violations=0

mapfile -d '' forbidden_files < <(
  find "$ROOT" -type f \
    -not -path "$ROOT/.git/*" \
    -not -path "$ROOT/target/*" \
    \( -name '*.py' -o -name '*.pyi' -o -name '*.pyc' -o -name '*.pyo' -o \
       -name 'pyproject.toml' -o -name 'requirements*.txt' -o -name 'setup.py' -o \
       -name 'setup.cfg' -o -name 'Pipfile' -o -name 'Pipfile.lock' -o -name 'tox.ini' \) \
    -print0
)
if ((${#forbidden_files[@]})); then
  printf 'Python source or packaging files are not allowed:\n' >&2
  printf '  %s\n' "${forbidden_files[@]}" >&2
  violations=1
fi

if grep -RInE \
  --exclude='check-rust-only.sh' \
  --include='*.sh' --include='*.yml' --include='*.yaml' \
  '(^|[[:space:]/])(python3?|pip3?|pytest|unittest|virtualenv)([[:space:]/]|$)' \
  "$ROOT/scripts" "$ROOT/examples" "$ROOT/.github/workflows"; then
  echo 'Python runtime or test commands are not allowed in scripts, examples, or CI.' >&2
  violations=1
fi

if grep -RInE --include='*.yml' --include='*.yaml' 'actions/setup-python(@|/)' "$ROOT/.github/workflows"; then
  echo 'GitHub Actions must not install a Python toolchain.' >&2
  violations=1
fi

if grep -RInE --include='*.rs' \
  'Command::new\("(python3?|pip3?|pytest|unittest)("|[[:space:]])' \
  "$ROOT/crates" "$ROOT/scripts" "$ROOT/examples"; then
  echo 'Rust code must not launch Python runtimes or test tools.' >&2
  violations=1
fi

if grep -E 'name = "(pyo3|cpython|python-sys)"' "$ROOT/Cargo.lock"; then
  echo 'Python binding crates are not allowed in Cargo.lock.' >&2
  violations=1
fi

if ((violations)); then
  exit 1
fi

echo 'Rust-only repository check passed.'
