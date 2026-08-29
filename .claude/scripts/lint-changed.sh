#!/usr/bin/env bash
# PostToolUse hook: formats/lints the file Claude just edited.
# Reads the file path either from an env var or from the JSON on stdin.
set -euo pipefail

FILE="${CLAUDE_TOOL_INPUT_FILE_PATH:-}"
if [ -z "$FILE" ]; then
  INPUT="$(cat)"
  FILE="$(echo "$INPUT" | jq -r '.tool_input.file_path // empty' 2>/dev/null || true)"
fi
[ -z "$FILE" ] && exit 0
[ -f "$FILE" ] || exit 0

case "$FILE" in
  *.py)
    command -v ruff >/dev/null 2>&1 && ruff check --fix "$FILE" || true
    command -v black >/dev/null 2>&1 && black -q "$FILE" || true
    ;;
  *.js|*.jsx|*.ts|*.tsx)
    command -v npx >/dev/null 2>&1 && npx --no-install eslint --fix "$FILE" 2>/dev/null || true
    command -v npx >/dev/null 2>&1 && npx --no-install prettier --write "$FILE" 2>/dev/null || true
    ;;
  *.go)
    command -v gofmt >/dev/null 2>&1 && gofmt -w "$FILE" || true
    ;;
  *.rs)
    command -v rustfmt >/dev/null 2>&1 && rustfmt "$FILE" || true
    ;;
esac

exit 0
