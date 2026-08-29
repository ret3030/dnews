#!/usr/bin/env bash
# SessionEnd hook: keep CLAUDE.md as the single source of truth by folding this
# repo's native auto-memory into it once a day, then committing the result.
#
# Lifecycle this closes:
#   1. first `dev` in a repo  -> /init writes the baseline CLAUDE.md
#   2. every session          -> native auto-memory accumulates facts
#   3. once a day (here)       -> those facts are merged into CLAUDE.md + committed
#
# Runs the merge in a detached background job so the session never blocks on it.
# Disable with DEV_NO_EVOLVE=1.
#
# Recursion guard: the `claude -p` below is itself a session and fires SessionEnd.
# Two things stop the loop: the throttle stamp is written BEFORE spawning (so the
# child's hook sees a fresh timestamp and bails), and the child is run with
# DEV_NO_EVOLVE=1.
set -uo pipefail

[ "${DEV_NO_EVOLVE:-0}" = "1" ] && exit 0

# ---------------------------------------------------------------------------
# background worker (re-invocation of this same script)
# ---------------------------------------------------------------------------
if [ "${1:-}" = "--worker" ]; then
  cd "${EVOLVE_ROOT:-/nonexistent}" 2>/dev/null || exit 0
  DEV_NO_EVOLVE=1 timeout 240 claude -p "$EVOLVE_PROMPT" \
    --dangerously-skip-permissions >/dev/null 2>&1 || true
  if ! git diff --quiet -- CLAUDE.md 2>/dev/null; then
    git add CLAUDE.md 2>/dev/null || true
    git commit -m "chore(claude-md): fold in session learnings [auto]" \
      >/dev/null 2>&1 || true
  fi
  rm -f "${EVOLVE_LOCK:-/nonexistent}"
  exit 0
fi

# ---------------------------------------------------------------------------
# hook entry: decide whether to kick off a run
# ---------------------------------------------------------------------------
INPUT="$(cat 2>/dev/null || true)"
CWD="$(printf '%s' "$INPUT" | jq -r '.cwd // empty' 2>/dev/null || true)"
[ -n "$CWD" ] && [ -d "$CWD" ] && cd "$CWD" 2>/dev/null || exit 0

command -v claude >/dev/null 2>&1 || exit 0
git rev-parse --show-toplevel >/dev/null 2>&1 || exit 0   # need git: we commit
ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT" || exit 0
[ -f "CLAUDE.md" ] || exit 0                              # /init should have made one

# don't touch a CLAUDE.md the user is mid-edit on - wait for a clean cycle
git diff --quiet -- CLAUDE.md 2>/dev/null || exit 0

mkdir -p .claude
STAMP=".claude/.claude-md-evolved"
LOCK=".claude/.claude-md-evolved.lock"

# throttle: at most once per 24h per repo
if [ -f "$STAMP" ]; then
  LAST="$(date -r "$STAMP" +%s 2>/dev/null || echo 0)"
  NOW="$(date +%s)"
  [ $((NOW - LAST)) -lt 86400 ] && exit 0
fi

# lock: one evolve at a time
if ! ( set -o noclobber; : > "$LOCK" ) 2>/dev/null; then
  exit 0
fi

# stamp NOW, before spawning, so the nested `claude -p` SessionEnd bails on throttle
touch "$STAMP"

EVOLVE_PROMPT="Read this project's CLAUDE.md and your project auto-memory. Fold any \
durable, factual project rules from memory into CLAUDE.md: merge related notes, never \
duplicate, delete stale or superseded lines, keep the file in its existing structure \
and under ~200 lines. Only established facts - corrections you were given, approaches \
confirmed to work - nothing speculative. If there is nothing solid to add, make no \
edits. Edit CLAUDE.md only; touch no other file."

export EVOLVE_ROOT="$ROOT"
export EVOLVE_PROMPT
export EVOLVE_LOCK="$ROOT/$LOCK"

setsid nohup "$0" --worker >/dev/null 2>&1 &

exit 0
