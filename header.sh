#!/usr/bin/env bash
DB=~/.local/share/newsboat/cache.db
STATE="$1"

COUNT=$(sqlite3 "$DB" "SELECT count(*) FROM rss_item WHERE unread = 1;")

O=$'\033[38;5;214m'
ACTIVE=$'\033[1;38;5;226m'
TAB=$'\033[38;5;245m'
SEP=$'\033[38;5;239m'
R=$'\033[0m'

FILTER=""
[[ -n "$STATE" ]] && FILTER=$(cat "$STATE" 2>/dev/null)

mapfile -t CATS < <(~/.config/newsboat/categories.sh)

if [[ -z "$FILTER" ]]; then
    TABS="${ACTIVE}All${R}"
else
    TABS="${TAB}All${R}"
fi

for c in "${CATS[@]:0:8}"; do
    if [[ "$c" == "$FILTER" ]]; then
        TABS="$TABS${SEP} · ${R}${ACTIVE}$c${R}"
    else
        TABS="$TABS${SEP} · ${R}${TAB}$c${R}"
    fi
done

printf "${O}dnews${R}  %s  ${SEP}·${R}  ${TAB}%s unread${R}\n" "$TABS" "$COUNT"
