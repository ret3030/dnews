#!/usr/bin/env bash
COUNT=$(sqlite3 ~/.local/share/newsboat/cache.db \
    "SELECT count(*) FROM rss_item WHERE unread = 1;")
DATE=$(date "+%a %d %b %Y")
O=$'\033[38;5;214m'
R=$'\033[0m'
printf "${O} \uf1ea  dnews  ·  %s unread  ·  %s${R}\n " "$COUNT" "$DATE"
