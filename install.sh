#!/usr/bin/env bash
set -e
CONF="$HOME/.config/newsboat"
mkdir -p "$CONF"

echo "Installing dnews..."
cp dnews.sh preview.sh header.sh mark_read.sh toggle.sh strip_links.lua colorize.py "$CONF/"
chmod +x "$CONF"/*.sh
cp newsboat.config "$CONF/config"

echo "Checking dependencies..."
for dep in fzf newsboat sqlite3 rdrview pandoc python3; do
    command -v "$dep" &>/dev/null && echo "  ✓ $dep" || echo "  ✗ $dep — NOT FOUND"
done

python3 -c "import ftfy" 2>/dev/null && echo "  ✓ python-ftfy" || echo "  ✗ ftfy — run: pip install ftfy"

echo ""
echo "Add to ~/.bashrc:"
echo "  alias dnews='~/.config/newsboat/dnews.sh'"
echo ""
echo "Add your feeds to: $CONF/urls"
echo "See urls.example for reference."
