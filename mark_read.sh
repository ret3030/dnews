#!/usr/bin/env bash
sqlite3 ~/.local/share/newsboat/cache.db "UPDATE rss_item SET unread=0 WHERE url='$1';"
