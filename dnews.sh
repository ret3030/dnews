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
newsboat -x reload 2>/dev/null
COUNT=$(sqlite3 ~/.local/share/newsboat/cache.db \
    "SELECT count(*) FROM rss_item WHERE unread = 1;")
kill $SPIN_PID 2>/dev/null
printf "\r\033[K\033[38;5;214m\uf1ea\033[0m  %s unread articles\n" "$COUNT"

PROMPT=$(printf ' \uf002  ')

sqlite3 -separator $'\x01' ~/.local/share/newsboat/cache.db \
    "SELECT title, pubDate, url FROM rss_item WHERE unread = 1 ORDER BY pubDate DESC;" \
| fzf \
    --delimiter $'\x01' \
    --with-nth 1 \
    --preview "$HOME/.config/newsboat/preview.sh {1} {2} {3}" \
    --preview-window="right:75%" \
    --header "$(~/.config/newsboat/header.sh)" \
    --header-first \
    --footer " dnews v1.0 · by Velrion Solutions" \
    --layout=reverse \
    --prompt "$PROMPT" \
    --pointer "› " \
    --marker "✓ " \
    --info=right \
    --color="bg+:#3c3836,bg:#282828,fg:#ebdbb2,fg+:#fbf1c7" \
    --color="hl:#fabd2f,hl+:#fe8019,header:#fe8019,info:#fe8019" \
    --color="prompt:#fabd2f,pointer:#fe8019,marker:#b8bb26,border:#504945" \
    --color="separator:#504945,scrollbar:#504945,footer:#665c54" \
    --border=rounded \
    --bind "focus:execute-silent(~/.config/newsboat/mark_read.sh {3})+transform-header(~/.config/newsboat/header.sh)" \
    --bind "enter:execute-silent(nohup xdg-open {3} >/dev/null 2>&1)" \
    --bind "ctrl-a:reload(~/.config/newsboat/toggle.sh)"
