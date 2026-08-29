#!/usr/bin/env bash
TITLE=$(echo "$1" | python3 -c "import sys,re; t=sys.stdin.read(); t=re.sub(r'\x1b\[[0-9;]*m','',t); t=re.sub(r'^\s*[0-9]+\.\s+[A-Za-z]{2,3}\s+','',t); t=re.sub(r'\s+·.*','',t); print(t.strip())")
TIMESTAMP="$2"
URL="$3"
DOMAIN=$(echo "$URL" | grep -oP '(?<=https?://)[^/]+' | sed 's/^www\.//')
DATE=$(date -d "@$TIMESTAMP" "+%a %d %b %Y  %H:%M" 2>/dev/null || echo "$TIMESTAMP")
W=${FZF_PREVIEW_COLUMNS:-80}

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

printf " %s╭%s╮%s\n" "$ORANGE" "$HR" "$RESET"
while IFS= read -r line; do
    box_line "${BOLD}${line}${RESET}"
done < <(echo "$TITLE" | fold -s -w $(( BOXW - 2 )))
printf " %s├%s┤%s\n" "$ORANGE" "$HR" "$RESET"
box_line "${DIMC}  ${DATE}      ${DOMAIN}${RESET}"
printf " %s╰%s╯%s\n" "$ORANGE" "$HR" "$RESET"
printf "\n"

TW=$(( W - 6 ))

CONTENT=$(timeout 2 rdrview -T title,body -H "$URL" 2>/dev/null \
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
  | awk 'NF{p=1;print} !NF&&p{printf "\n";p=0}' \
  | head -150)

if [[ -z "$CONTENT" ]]; then
    printf "   %sArticle unavailable — press Enter to open in browser%s\n" "$DIMC" "$RESET"
else
    echo "$CONTENT"
fi
