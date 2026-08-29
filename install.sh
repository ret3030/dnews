#!/usr/bin/env bash
set -e

echo "Building dnews (release)..."
cargo build --release

BIN_DIR="$HOME/.local/bin"
mkdir -p "$BIN_DIR"
cp target/release/dnews "$BIN_DIR/dnews"
echo "Installed: $BIN_DIR/dnews"

CONF_DIR="$HOME/.config/dnews"
if [[ ! -f "./feeds.opml" ]] && [[ ! -f "$CONF_DIR/feeds.opml" ]]; then
    mkdir -p "$CONF_DIR"
    cp feeds.opml "$CONF_DIR/feeds.opml"
    echo "Copied default feeds.opml to $CONF_DIR/feeds.opml"
fi

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) echo "Add $BIN_DIR to your PATH (e.g. in ~/.bashrc): export PATH=\"$BIN_DIR:\$PATH\"" ;;
esac

echo ""
echo "Run with: dnews"
echo "Feeds are read from ./feeds.opml if present in the current directory,"
echo "otherwise from $CONF_DIR/feeds.opml. Edit it and restart to change feeds."
