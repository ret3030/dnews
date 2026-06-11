# dnews

> A fast, minimal terminal news reader with Reader View — built for people who want to stay informed without leaving the terminal.

![dnews screenshot](screenshot.png)

---

## Overview

**dnews** combines [newsboat](https://newsboat.org/) for feed fetching with [fzf](https://github.com/junegunn/fzf) for a clean interactive UI. Articles are rendered via [rdrview](https://github.com/eafer/rdrview) — the same Reader View technology as Firefox — stripped of ads, navigation and clutter.

Feeds reload automatically on launch with a live spinner. Articles are marked as read as you browse. Everything stays in your terminal.

---

## Features

- Gruvbox color theme with Nerd Font icons
- Reader View via rdrview — clean article text, no bloat
- Auto mark-as-read on focus
- Toggle between unread / all articles with `Ctrl+A`
- Exact word search across all headlines
- Braille loading spinner on feed reload
- Groups articles by source with visual spacing
- Full UTF-8 support including diacritics

---

## Dependencies

| Package | Purpose | Install |
|---------|---------|---------|
| `newsboat` | RSS feed fetcher | `pacman -S newsboat` |
| `fzf` | Interactive UI | `pacman -S fzf` |
| `sqlite3` | Database queries | `pacman -S sqlite` |
| `rdrview` | Reader View | `yay -S rdrview` |
| `pandoc` | HTML → plain text | `pacman -S pandoc` |
| `python-ftfy` | Encoding fixes | `pip install ftfy` |

---

## Install

```bash
git clone https://github.com/ret3030/dnews
cd dnews
./install.sh
```

Add your feeds to `~/.config/newsboat/urls` — see `urls.example` for reference.

---

## Usage

```bash
dnews
```

| Key | Action |
|-----|--------|
| `Enter` | Open article in browser |
| `Ctrl+A` | Toggle unread / all |
| `j` / `k` | Navigate up / down |
| `d` / `u` | Page down / up |
| Type | Search headlines |

---

## Built by

[Velrion Solutions](https://github.com/ret3030)
