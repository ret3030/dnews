#!/usr/bin/env bash
# Prints distinct feed categories (newsboat tags) from urls, one per line,
# in first-seen order. Shared by dnews.sh (to build the tab key bindings)
# and header.sh (to render the tab bar).
URLS=~/.config/newsboat/urls

python3 - "$URLS" <<'PYEOF'
import sys, shlex

seen = []
try:
    with open(sys.argv[1]) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith('#'):
                continue
            try:
                parts = shlex.split(line)
            except ValueError:
                continue
            for p in parts[1:]:
                if not p.startswith('~') and p not in seen:
                    seen.append(p)
except FileNotFoundError:
    pass

print('\n'.join(seen))
PYEOF
