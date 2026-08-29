#!/usr/bin/env bash
# UserPromptSubmit hook: when auto-parallel is enabled for the project, inject a
# directive that makes the main agent evaluate whether the request splits into
# independent modules and, if so, PROPOSE a parallel run - it launches only
# after the user agrees.
#
# Does nothing unless .claude/parallel-auto.json has "enabled": true.
# This replaces the old manual /parallel-tasks command; run-parallel.sh is
# still the thing that actually opens the tmux windows.
set -uo pipefail

INPUT="$(cat 2>/dev/null || true)"

# Locate the project's .claude dir.
PROJ="${CLAUDE_PROJECT_DIR:-}"
if [ -z "$PROJ" ] && command -v jq >/dev/null 2>&1; then
  PROJ="$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null || true)"
fi
[ -z "$PROJ" ] && PROJ="$PWD"

CFG="$PROJ/.claude/parallel-auto.json"
[ -f "$CFG" ] || exit 0

enabled="false"; max="4"; perm="acceptEdits"
if command -v jq >/dev/null 2>&1; then
  enabled="$(jq -r '.enabled // false'          "$CFG" 2>/dev/null || echo false)"
  max="$(    jq -r '.maxWindows // 4'           "$CFG" 2>/dev/null || echo 4)"
  perm="$(   jq -r '.permissionMode // "acceptEdits"' "$CFG" 2>/dev/null || echo acceptEdits)"
else
  grep -Eq '"enabled"[[:space:]]*:[[:space:]]*true' "$CFG" && enabled="true"
fi
[ "$enabled" = "true" ] || exit 0

# Skip trivial prompts (confirmations, short replies) to keep the noise down.
if command -v jq >/dev/null 2>&1; then
  prompt="$(printf '%s' "$INPUT" | jq -r '.prompt // empty' 2>/dev/null || true)"
  words="$(printf '%s' "$prompt" | wc -w | tr -d ' ')"
  [ "${words:-0}" -lt 6 ] && exit 0
fi

RUNNER="$HOME/.claude/dev-bootstrap-assets/scripts/run-parallel.sh"

cat <<EOF
[auto-parallel is ON for this project]
Before starting, decide whether this request decomposes into 2+ subtasks that
touch DIFFERENT files/modules and are each non-trivial. Default is NO: a single
feature, a question, a bug fix, or tasks that share files stay ONE task - just
proceed normally and do not mention this.

If it genuinely splits:
1. Do not edit anything and do not launch anything yet. First reply with the
   proposed split - for each subtask: a short kebab-case slug, the module/path
   it owns, a one-line description. Cap: $max windows (more -> propose waves).
2. Wait for the user to explicitly agree.
3. On agreement, only inside tmux (\$TMUX set; otherwise ask the user to start
   'dev' first), write .claude/parallel-tasks/tasks-<timestamp>.txt with one
   "slug|description" line per subtask, then run:
     $RUNNER .claude/parallel-tasks/tasks-<timestamp>.txt $perm
4. Never merge worktree branches automatically - report the state and leave the
   merge to the user.
EOF
exit 0
