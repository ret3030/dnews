#!/usr/bin/env bash
DB=~/.local/share/newsboat/cache.db
STATE="$1"

COUNT=$(sqlite3 "$DB" "SELECT count(*) FROM rss_item WHERE unread = 1;")
DATE=$(date "+%a %d %b %Y")
O=$'\033[38;5;214m'
ACTIVE_C=$'\033[38;5;226m'
DIM=$'\033[38;5;242m'
R=$'\033[0m'

ACTIVE=""
[[ -n "$STATE" ]] && ACTIVE=$(cat "$STATE" 2>/dev/null)

mapfile -t CATS < <(~/.config/newsboat/categories.sh)

TABS=""
if [[ -z "$ACTIVE" ]]; then
    TABS="${ACTIVE_C} F1${R}${ACTIVE_C}·All${R}"
else
    TABS="${O} F1${R}${DIM}·All${R}"
fi

i=2
for c in "${CATS[@]}"; do
    (( i > 9 )) && break
    if [[ "$c" == "$ACTIVE" ]]; then
        TABS="$TABS  ${ACTIVE_C} F$i${R}${ACTIVE_C}·$c${R}"
    else
        TABS="$TABS  ${O} F$i${R}${DIM}·$c${R}"
    fi
    i=$((i + 1))
done

printf "${O}   dnews  ·  %s unread  ·  %s${R}\n %s\n " "$COUNT" "$DATE" "$TABS"
