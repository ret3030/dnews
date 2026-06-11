#!/usr/bin/env bash
TITLE=$(echo "$1" | sed "s|^[A-Z]\{2,4\}  ||" | sed "s|  [0-9]\{2\}:[0-9]\{2\}$||")
TIMESTAMP="$2"
URL="$3"
DOMAIN=$(echo "$URL" | grep -oP '(?<=https?://)[^/]+' | sed 's/^www\.//')
DATE=$(date -d "@$TIMESTAMP" "+%a %d %b %Y  %H:%M" 2>/dev/null || echo "$TIMESTAMP")
W=${FZF_PREVIEW_COLUMNS:-80}
SEP=$(printf '%.0s─' $(seq 1 $(( W - 2 ))))
TW=$(( W - 8 ))

printf "\n"
# Titulek zalamovaný podle šířky preview
echo "$TITLE" | fold -s -w $TW | sed 's/^/   \x1b[1m/' | sed 's/$/\x1b[0m/'
printf " %s\n" "$SEP"
printf "   \033[38;5;214m\uf073  %s    \uf0c1  %s\033[0m\n" "$DATE" "$DOMAIN"
printf " %s\n" "$SEP"
printf "\n"

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
  | sed 's/^/   /' \
  | awk 'NF{p=1;print} !NF&&p{printf "\n";p=0}' \
  | head -150)

if [[ -z "$CONTENT" ]]; then
    printf "   \033[38;5;242mArticle unavailable — press Enter to open in browser\033[0m\n"
else
    echo "$CONTENT"
fi
