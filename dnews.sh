#!/usr/bin/env bash

DB=~/.local/share/newsboat/cache.db
URLS=~/.config/newsboat/urls
LOG=/tmp/dnews_reload.log
FILTER_STATE="/tmp/dnews_filter_$$"

: > "$FILTER_STATE"
trap 'rm -f "$FILTER_STATE"' EXIT

O=$'\033[38;5;214m'
TRACK=$'\033[38;5;238m'
R=$'\033[0m'

TOTAL=$(grep -vcE '^[[:space:]]*(#|$)' "$URLS" 2>/dev/null)
(( TOTAL < 1 )) && TOTAL=1

draw_bar() {
    local done=$1 width=24
    (( done > TOTAL )) && done=$TOTAL
    local filled=$(( done * width / TOTAL ))
    local empty=$(( width - filled ))
    local fill="" gap="" i
    for (( i = 0; i < filled; i++ )); do fill+="█"; done
    for (( i = 0; i < empty; i++ )); do gap+="░"; done
    printf "\r${O}${R}  Fetching feeds  ${O}%s${TRACK}%s${R}  %d/%d " "$fill" "$gap" "$done" "$TOTAL"
}

: > "$LOG"
newsboat -d "$LOG" -l 5 -x reload 2>/dev/null &
RELOAD_PID=$!

draw_bar 0
while kill -0 "$RELOAD_PID" 2>/dev/null; do
    DONE=$(grep -c 'starting reload of' "$LOG" 2>/dev/null)
    draw_bar "${DONE:-0}"
    sleep 0.1
done
wait "$RELOAD_PID" 2>/dev/null
draw_bar "$TOTAL"
rm -f "$LOG"

COUNT=$(sqlite3 "$DB" "SELECT count(*) FROM rss_item WHERE unread = 1;")
printf "\r\033[K${O}${R}  %s unread articles\n" "$COUNT"

VERSION=$(git -C ~/dnews describe --tags --always 2>/dev/null || echo v1.0)
FOOT=$(printf " \\033[38;5;242mdnews %s  ↵ open   / search   ^F fullscreen   ^R reload   Tab/S-Tab next/prev\\033[0m" "$VERSION")
PROMPT=$(printf '   ')

# Build F2..F9 key bindings, one per feed category found in urls (F1 is the
# built-in "All" tab, wired up below alongside esc/ctrl-r).
mapfile -t CATS < <(~/.config/newsboat/categories.sh)
CAT_BINDS=()
i=2
for c in "${CATS[@]}"; do
    (( i > 9 )) && break
    esc_c=${c//\'/\'\\\'\'}
    CAT_BINDS+=(--bind "f$i:execute-silent(printf '%s' '$esc_c' > $FILTER_STATE)+first+reload(~/.config/newsboat/list_query.sh '$esc_c')+transform-header(~/.config/newsboat/header.sh $FILTER_STATE)")
    i=$((i + 1))
done

~/.config/newsboat/list_query.sh \
| fzf \
    --delimiter $'\x01' \
    --with-nth 1 \
    --ansi \
    --exact \
    --gap \
    --no-input \
    --preview "$HOME/.config/newsboat/preview.sh {1} {2} {3}" \
    --preview-window="right:75%" \
    --header "$(~/.config/newsboat/header.sh "$FILTER_STATE")" \
    --header-first \
    --footer "$FOOT" \
    --layout=reverse \
    --prompt "$PROMPT" \
    --pointer "› " \
    --marker "✓ " \
    --info=right \
    --color="bg+:#3c3836,bg:#282828,fg:#ebdbb2,fg+:#fbf1c7" \
    --color="hl:#fabd2f,hl+:#fe8019,header:#fe8019,info:#fe8019" \
    --color="prompt:#fabd2f,pointer:#fe8019,marker:#b8bb26,border:#504945" \
    --color="separator:#504945,scrollbar:#504945,footer:#504945" \
    --border=rounded \
    --gap \
    --bind "focus:execute-silent(~/.config/newsboat/mark_read.sh {3})+transform-header(~/.config/newsboat/header.sh $FILTER_STATE)" \
    --bind "enter:execute-silent(nohup xdg-open {3} >/dev/null 2>&1)" \
    --bind "tab:down" \
    --bind "shift-tab:up" \
    --bind "ctrl-f:toggle-preview" \
    --bind "/:show-input+enable-search" \
    --bind "f1:execute-silent(: > $FILTER_STATE)+first+reload(~/.config/newsboat/list_query.sh)+transform-header(~/.config/newsboat/header.sh $FILTER_STATE)" \
    "${CAT_BINDS[@]}" \
    --bind "esc:clear-query+disable-search+hide-input+first+reload(~/.config/newsboat/list_query.sh \"\$(cat $FILTER_STATE 2>/dev/null)\")" \
    --bind "ctrl-r:execute-silent(newsboat -x reload 2>/dev/null &)+reload(~/.config/newsboat/list_query.sh \"\$(cat $FILTER_STATE 2>/dev/null)\")+transform-header(~/.config/newsboat/header.sh $FILTER_STATE)"
