#!/usr/bin/env bash
COUNT=$(sqlite3 ~/.local/share/newsboat/cache.db \
    "SELECT count(*) FROM rss_item WHERE unread = 1;")
DATE=$(date "+%a %d %b %Y")
printf " \uf1ea  NEWS  \xc2\xb7  %s unread  \xc2\xb7  %s" "$COUNT" "$DATE"
