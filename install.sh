#!/usr/bin/env bash
set -e
CONF="$HOME/.config/newsboat"
mkdir -p "$CONF"

echo "Installing dnews..."
cp dnews.sh preview.sh header.sh mark_read.sh strip_links.lua colorize.py categories.sh list_query.sh "$CONF/"
chmod +x "$CONF"/*.sh
cp newsboat.config "$CONF/config"

if [[ -f feeds.opml ]]; then
    echo "Importing feeds from feeds.opml (replaces $CONF/urls)..."
    [[ -f "$CONF/urls" ]] && cp "$CONF/urls" "$CONF/urls.bak"
    python3 - feeds.opml > "$CONF/urls" <<'PYEOF'
import sys
import xml.etree.ElementTree as ET

tree = ET.parse(sys.argv[1])
body = tree.getroot().find('body')

def emit(node, tags):
    url = node.get('xmlUrl')
    if url:
        title = node.get('title') or node.get('text') or ''
        tag_str = ''.join(' "%s"' % t for t in tags)
        print('%s "~%s"%s' % (url, title, tag_str))
    else:
        cat = node.get('text') or node.get('title') or ''
        for child in node.findall('outline'):
            emit(child, tags + [cat] if cat else tags)

for outline in body.findall('outline'):
    emit(outline, [])
PYEOF
fi

echo "Checking dependencies..."
for dep in fzf newsboat sqlite3 rdrview pandoc python3; do
    command -v "$dep" &>/dev/null && echo "  ✓ $dep" || echo "  ✗ $dep — NOT FOUND"
done

python3 -c "import ftfy" 2>/dev/null && echo "  ✓ python-ftfy" || echo "  ✗ ftfy — run: pip install ftfy"

echo ""
echo "Add to ~/.bashrc:"
echo "  alias dnews='~/.config/newsboat/dnews.sh'"
echo ""
if [[ -f feeds.opml ]]; then
    echo "Feeds imported from feeds.opml into $CONF/urls (previous urls saved to urls.bak)."
else
    echo "Add your feeds to: $CONF/urls"
    echo "See urls.example for reference."
fi
