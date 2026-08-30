#!/usr/bin/env bash
set -e

# Cross-platform installer for Linux, macOS, and Windows-under-Git-Bash/MSYS.
# Native Windows (PowerShell, no bash) uses install.ps1 instead.

echo "Building dnews (release)..."
cargo build --release

case "$(uname -s)" in
    Linux*)              OS=linux ;;
    Darwin*)             OS=macos ;;
    MINGW*|MSYS*|CYGWIN*) OS=windows ;;
    *)                   OS=linux ;;  # unknown: assume XDG-style layout
esac

if [[ "$OS" == "windows" ]]; then
    BIN_NAME="dnews.exe"
    BIN_DIR="${LOCALAPPDATA:-$HOME/AppData/Local}/Programs/dnews"
    CONF_DIR="${APPDATA:-$HOME/AppData/Roaming}/dnews"
elif [[ "$OS" == "macos" ]]; then
    BIN_NAME="dnews"
    BIN_DIR="$HOME/.local/bin"
    CONF_DIR="$HOME/Library/Application Support/dnews"
else
    BIN_NAME="dnews"
    BIN_DIR="$HOME/.local/bin"
    CONF_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/dnews"
fi

mkdir -p "$BIN_DIR"
cp "target/release/$BIN_NAME" "$BIN_DIR/$BIN_NAME"
echo "Installed: $BIN_DIR/$BIN_NAME"

# Seed the default feed list only if neither a repo-local nor an installed one exists.
if [[ ! -f "./feeds.opml" ]] && [[ ! -f "$CONF_DIR/feeds.opml" ]]; then
    mkdir -p "$CONF_DIR"
    cp feeds.opml "$CONF_DIR/feeds.opml"
    echo "Copied default feeds.opml to $CONF_DIR/feeds.opml"
fi

case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        if [[ "$OS" == "windows" ]]; then
            echo "Add $BIN_DIR to your PATH (PowerShell, one-time):"
            echo "  [Environment]::SetEnvironmentVariable('Path', [Environment]::GetEnvironmentVariable('Path','User') + ';$BIN_DIR', 'User')"
        else
            echo "Add $BIN_DIR to your PATH (e.g. in ~/.bashrc): export PATH=\"$BIN_DIR:\$PATH\""
        fi
        ;;
esac

echo ""
echo "Run with: $BIN_NAME"
echo "Feeds are read from ./feeds.opml if present in the current directory,"
echo "otherwise from $CONF_DIR/feeds.opml. Edit it and restart to change feeds."
