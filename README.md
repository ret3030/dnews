# dnews

A terminal news reader built on newsboat + fzf with a clean Reader View.

![dnews screenshot](screenshot.png)

## Features
- Gruvbox theme with Nerd Font icons
- Reader View via rdrview (Firefox-grade content extraction)
- Auto mark-as-read on hover
- Toggle unread/all with Ctrl+A
- Braille spinner on feed reload
- Full UTF-8 / Czech diacritics support

## Dependencies
- `newsboat` — feed fetcher
- `fzf` — fuzzy finder UI
- `sqlite3` — database queries
- `rdrview` — Reader View (AUR: `yay -S rdrview`)
- `pandoc` — HTML to plain text
- `python-ftfy` — encoding fix (`pip install ftfy`)

## Install
```bash
git clone https://github.com/YOUR_USERNAME/dnews
cd dnews
./install.sh
```

Add your feeds to `~/.config/newsboat/urls` — see `urls.example`.

## Usage
```bash
dnews
```
| Key | Action |
|-----|--------|
| `Enter` | Open in browser |
| `Ctrl+A` | Toggle unread/all |
| `j/k` | Navigate |
| `d/u` | Page down/up |
| `/` | Search |
