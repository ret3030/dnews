#!/usr/bin/env bash
# Full-screen reader view, invoked on `enter` in dnews.sh. Marks the article
# read, fetches Reader View content via rdrview, and pages it with `less`.
# Replaces the old fzf preview-pane (ctrl-f) — dnews is now a two-screen
# flow: the list, and this article view.
TITLE="$1"
TIMESTAMP="$2"
URL="$3"

~/.config/newsboat/mark_read.sh "$URL"

DOMAIN=$(echo "$URL" | grep -oP '(?<=https?://)[^/]+' | sed 's/^www\.//')
DATE=$(date -d "@$TIMESTAMP" "+%a %d %b %Y  %H:%M" 2>/dev/null || echo "$TIMESTAMP")

COLS=$(tput cols 2>/dev/null || echo 100)
W=$COLS
(( W > 100 )) && W=100
(( W < 40 )) && W=40

ORANGE=$'\033[38;5;214m'
DIMC=$'\033[38;5;245m'
BOLD=$'\033[1m'
RESET=$'\033[0m'

BOXW=$(( W - 4 ))
(( BOXW < 20 )) && BOXW=20
HR=$(printf '─%.0s' $(seq 1 "$BOXW"))

strip_ansi() { sed -E $'s/\x1b\\[[0-9;]*m//g'; }

box_line() {
    local raw="$1" plain pad
    plain=$(printf '%s' "$raw" | strip_ansi)
    pad=$(( BOXW - ${#plain} - 2 ))
    (( pad < 0 )) && pad=0
    printf " %s│%s %s%*s %s│%s\n" "$ORANGE" "$RESET" "$raw" "$pad" "" "$ORANGE" "$RESET"
}

{
printf " %s╭%s╮%s\n" "$ORANGE" "$HR" "$RESET"
while IFS= read -r line; do
    box_line "${BOLD}${line}${RESET}"
done < <(echo "$TITLE" | fold -s -w $(( BOXW - 2 )))
printf " %s├%s┤%s\n" "$ORANGE" "$HR" "$RESET"
box_line "${DIMC}  ${DATE}      ${DOMAIN}${RESET}"
printf " %s╰%s╯%s\n" "$ORANGE" "$HR" "$RESET"
printf "\n"

TW=$(( W - 6 ))

CONTENT=$(timeout 4 rdrview -T title,body -H "$URL" 2>/dev/null \
  | grep -v 'elementId\|_type\|dotcomrendering\|blockCreated\|pageElements' \
  | grep -v '^\s*[{}\[\]]' \
  | python3 -c "
import sys, ftfy, html, re
skip = {'read more','share','advertisement','key events','graph','chart'}
for line in sys.stdin:
    line = ftfy.fix_text(html.unescape(line.rstrip()))
    s = line.strip()
    if not s or re.match(r'^\[.*\]\s*$', s) or s.lower() in skip: continue
    print(line)
" 2>/dev/null \
  | pandoc -f html -t plain --wrap=auto --columns=$TW \
      --lua-filter ~/.config/newsboat/strip_links.lua 2>/dev/null \
  | sed "s/^/  ${ORANGE}▏${RESET} /" \
  | awk 'NF{p=1;print} !NF&&p{printf "\n";p=0}')

if [[ -z "$CONTENT" ]]; then
    printf "   %sArticle unavailable — opening in browser...%s\n" "$DIMC" "$RESET"
    nohup xdg-open "$URL" >/dev/null 2>&1 &
else
    echo "$CONTENT"
fi
printf "\n %sq to return%s\n" "$DIMC" "$RESET"
} | less -R
