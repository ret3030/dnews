#!/usr/bin/env bash
MODE=$(cat /tmp/dnews_mode 2>/dev/null || echo "unread")
if [[ "$MODE" == "unread" ]]; then
    echo "all" > /tmp/dnews_mode
    sqlite3 -separator $'\x01' ~/.local/share/newsboat/cache.db \
        "SELECT title, pubDate, url FROM rss_item ORDER BY pubDate DESC;"
else
    echo "unread" > /tmp/dnews_mode
    sqlite3 -separator $'\x01' ~/.local/share/newsboat/cache.db \
        "SELECT title, pubDate, url FROM rss_item WHERE unread = 1 ORDER BY pubDate DESC;"
fi
