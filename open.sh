#!/usr/bin/env bash
URL="$1"
STATUS=$(curl -s -o /dev/null -w "%{http_code}" --max-time 2 -A "Mozilla/5.0" "$URL")
if [[ "$STATUS" == "404" || "$STATUS" == "410" || "$STATUS" == "000" ]]; then
    xdg-open "https://archive.ph/$URL" >/dev/null 2>&1
else
    xdg-open "$URL" >/dev/null 2>&1
fi
