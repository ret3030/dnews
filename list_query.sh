#!/usr/bin/env bash
# Single source of truth for the article list fed to fzf. Optionally filtered
# to one category/tag (as defined in urls, see categories.sh). Used by
# dnews.sh's initial pipe and its esc/ctrl-r/tab reload bindings so the query
# never drifts out of sync between them.
DB=~/.local/share/newsboat/cache.db
URLS=~/.config/newsboat/urls
CATEGORY="$1"

WHERE=""
if [[ -n "$CATEGORY" ]]; then
    WHERE=$(python3 - "$URLS" "$CATEGORY" <<'PYEOF'
import sys, shlex

urls_file, cat = sys.argv[1], sys.argv[2]
urls = []
try:
    with open(urls_file) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            try:
                parts = shlex.split(line)
            except ValueError:
                continue
            if not parts:
                continue
            url, tags = parts[0], parts[1:]
            if cat in tags:
                urls.append(url)
except FileNotFoundError:
    pass

if urls:
    escaped = ["'" + u.replace("'", "''") + "'" for u in urls]
    print("WHERE feedurl IN (%s)" % ",".join(escaped))
else:
    print("WHERE 0")
PYEOF
    )
fi

sqlite3 -separator $'\x01' "$DB" \
    "SELECT title, pubDate, url, unread FROM rss_item $WHERE ORDER BY pubDate DESC;" \
| python3 ~/.config/newsboat/colorize.py
