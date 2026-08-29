#!/usr/bin/env bash
# Stop hook: runs the test suite for the detected stack.
# exit 2 + text on stderr => Claude gets the output back as feedback and must iterate.
#
# IMPORTANT: in an isolated worktree session ${CLAUDE_PROJECT_DIR} still points
# at the main checkout, not the worktree - and the hook process is not run in
# the worktree directory automatically. The real path must be taken from the
# JSON on stdin (the "cwd" field) and cd'd into, otherwise the wrong directory
# gets tested.
set -uo pipefail

INPUT="$(cat)"
TARGET_CWD="$(echo "$INPUT" | jq -r '.cwd // empty' 2>/dev/null || true)"
if [ -n "$TARGET_CWD" ] && [ -d "$TARGET_CWD" ]; then
  cd "$TARGET_CWD"
fi

LOG="$(mktemp)"
RUN_CMD=""

if [ -f "package.json" ] && grep -q '"test"' package.json; then
  RUN_CMD="npm test --silent"
elif [ -f "pyproject.toml" ] || [ -f "pytest.ini" ]; then
  RUN_CMD="pytest -q"
elif [ -f "go.mod" ]; then
  RUN_CMD="go test ./..."
elif [ -f "Cargo.toml" ]; then
  RUN_CMD="cargo test --quiet"
else
  exit 0
fi

if ! eval "$RUN_CMD" > "$LOG" 2>&1; then
  echo "Tests failed (\`$RUN_CMD\` in $(pwd)), fix them and try again:" >&2
  tail -n 60 "$LOG" >&2
  rm -f "$LOG"
  exit 2
fi

rm -f "$LOG"
exit 0
