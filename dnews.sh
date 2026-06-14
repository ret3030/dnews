#!/usr/bin/env bash

echo "unread" > /tmp/dnews_mode

spinner() {
    local frames=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
    local i=0
    while true; do
        printf "\r\033[38;5;214m%s\033[0m  Fetching feeds..." "${frames[$((i % 10))]}"
        sleep 0.08
        (( i++ ))
    done
}

spinner &
SPIN_PID=$!
newsboat -x reload 2>/dev/null &
COUNT=$(sqlite3 ~/.local/share/newsboat/cache.db \
    "SELECT count(*) FROM rss_item WHERE unread = 1;")
kill $SPIN_PID 2>/dev/null
printf "\r\033[K\033[38;5;214m\uf1ea\033[0m  %s unread articles\n" "$COUNT"

VERSION=$(git -C ~/dnews describe --tags --always 2>/dev/null || echo v1.0)
FOOT=$(printf " \\033[38;5;242mdnews %s  ↵ open   / search   ^A all/unread   ^F fullscreen   ^R reload   Tab/S-Tab next/prev\\033[0m" "$VERSION")
PROMPT=$(printf ' \uf002  ')

sqlite3 -separator $'\x01' ~/.local/share/newsboat/cache.db \
    "SELECT title, pubDate, url FROM rss_item WHERE unread = 1 ORDER BY pubDate DESC;" \
| python3 ~/.config/newsboat/colorize.py \
| fzf \
    --delimiter $'\x01' \
    --with-nth 1 \
    --ansi \
    --exact \
    --gap \
    --no-input \
    --preview "$HOME/.config/newsboat/preview.sh {1} {2} {3}" \
    --preview-window="right:75%" \
    --header "$(~/.config/newsboat/header.sh)" \
    --header-first \
    --footer "$FOOT" \
    --layout=reverse \
    --prompt "$PROMPT" \
    --pointer "› " \
    --marker "✓ " \
    --info=right \
    --color="bg+:#3c3836,bg:#282828,fg:#ebdbb2,fg+:#fbf1c7" \
    --color="hl:#fabd2f,hl+:#fe8019,header:#fe8019,info:#fe8019" \
    --color="prompt:#fabd2f,pointer:#fe8019,marker:#b8bb26,border:#504945" \
    --color="separator:#504945,scrollbar:#504945,footer:#504945" \
    --border=rounded \
    --gap \
    --bind "focus:execute-silent(~/.config/newsboat/mark_read.sh {3})+transform-header(~/.config/newsboat/header.sh)" \
    --bind "enter:execute-silent(nohup xdg-open {3} >/dev/null 2>&1)" \
    --bind "tab:down" \
    --bind "shift-tab:up" \
    --bind "ctrl-a:reload(~/.config/newsboat/toggle.sh)" \
    --bind "ctrl-f:toggle-preview" \
    --bind "/:show-input+enable-search" \
    --bind "esc:clear-query+disable-search+hide-input+first+reload(
        MODE=\$(cat /tmp/dnews_mode 2>/dev/null || echo unread)
        if [[ \$MODE == unread ]]; then
            sqlite3 -separator \$'\\x01' ~/.local/share/newsboat/cache.db \
                'SELECT title, pubDate, url FROM rss_item WHERE unread = 1 ORDER BY pubDate DESC;'
        else
            sqlite3 -separator \$'\\x01' ~/.local/share/newsboat/cache.db \
                'SELECT title, pubDate, url FROM rss_item ORDER BY pubDate DESC;'
        fi | python3 ~/.config/newsboat/colorize.py
    )" \
    --bind "start,every(180):reload(
        MODE=\$(cat /tmp/dnews_mode 2>/dev/null || echo unread)
        if [[ \$MODE == unread ]]; then
            sqlite3 -separator \$'\\x01' ~/.local/share/newsboat/cache.db \
                'SELECT title, pubDate, url FROM rss_item WHERE unread = 1 ORDER BY pubDate DESC;'
        else
            sqlite3 -separator \$'\\x01' ~/.local/share/newsboat/cache.db \
                'SELECT title, pubDate, url FROM rss_item ORDER BY pubDate DESC;'
        fi | python3 ~/.config/newsboat/colorize.py
    )+transform-header(~/.config/newsboat/header.sh)" \
    --bind "ctrl-r:execute-silent(newsboat -x reload 2>/dev/null &)+reload(
        MODE=\$(cat /tmp/dnews_mode 2>/dev/null || echo unread)
        if [[ \$MODE == unread ]]; then
            sqlite3 -separator \$'\\x01' ~/.local/share/newsboat/cache.db \
                'SELECT title, pubDate, url FROM rss_item WHERE unread = 1 ORDER BY pubDate DESC;'
        else
            sqlite3 -separator \$'\\x01' ~/.local/share/newsboat/cache.db \
                'SELECT title, pubDate, url FROM rss_item ORDER BY pubDate DESC;'
        fi | python3 ~/.config/newsboat/colorize.py
    )+transform-header(~/.config/newsboat/header.sh)"
