<div align="center">

# dnews

**A fast, minimal terminal news reader with Reader View — one native binary, no subprocess glue.**

[![Built with Rust](https://img.shields.io/badge/built%20with-Rust-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![TUI: ratatui](https://img.shields.io/badge/TUI-ratatui-4C9A91)](https://ratatui.rs)
[![Platforms](https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-6C7086)](#install)
[![Buy Me a Coffee](https://img.shields.io/badge/Buy%20Me%20a%20Coffee-support-FFDD00?logo=buymeacoffee&logoColor=black)](https://buymeacoffee.com/ret3030)

![dnews list view](docs/screenshot.png)

</div>

---

## Contents

- [Why dnews](#why-dnews)
- [Features](#features)
- [Install](#install)
- [Configuration](#configuration)
- [Usage](#usage)
- [How it works](#how-it-works)
- [Development](#development)
- [Support](#support)
- [About the developer](#about-the-developer)

---

## Why dnews

**dnews** is a single Rust binary: it fetches your RSS/Atom feeds concurrently, stores them in its
own SQLite database, and renders a [ratatui](https://ratatui.rs) terminal UI — a dense article list,
and the full article extracted with a Rust port of Mozilla's Readability (the same engine behind
Firefox's Reader Mode). On a wide terminal the article view sits in a side panel next to the list; on
a narrower one it opens full-screen instead.

There's no newsboat, no fzf, no rdrview/pandoc pipeline, no `less` handoff — everything runs inside
one process, so reloading is genuinely concurrent (every feed fetched in parallel, not one at a time)
and the UI never blocks or shells out.

| | dnews |
|---|---|
| **Runtime dependencies** | None — no newsboat, fzf, rdrview, pandoc, or `less` |
| **Reload** | Concurrent and non-blocking; the UI stays usable while feeds fetch |
| **Article text** | Readability-extracted, cached in SQLite, real paragraph breaks |
| **Storage** | Embedded SQLite (bundled — no system SQLite needed) |
| **TLS** | rustls — no OpenSSL or system TLS stack to install |
| **Config** | One `feeds.opml` file; folders become tabs |

---

## Features

- **Responsive split layout** — wide terminals show the list and article panel side by side, narrower
  ones open a full-screen reader instead. The tab bar, footer hints, search box, and progress bar all
  adapt to narrow widths rather than clipping or misaligning.
- **Real Reader View** — article text preserves paragraph breaks and flags where images were with an
  `[image: alt]` marker (inline images aren't possible in a plain terminal buffer). Extracted text is
  cached in SQLite, so reopening an article never refetches it.
- **Save for later** — press `s` to bookmark; a dedicated **Saved** tab collects bookmarks across all
  categories, and saved articles are never auto-deleted.
- **Automatic pruning + backups** — unsaved articles older than two months are removed so the
  database doesn't grow forever, and a rolling backup is kept as a safety net against a future
  migration bug.
- **Non-blocking reload** — feeds fetch concurrently in the background behind an animated progress
  bar; the interface never freezes.
- **Category tabs** — rounded pill-style tabs cycled with `Tab`/`Shift+Tab`, driven entirely by your
  `feeds.opml` folder structure. No separate config file.
- **Vim-style navigation** — `j`/`k`, `g`/`G`, `d`/`u` everywhere, in both the list and the article.
- **Live search** — `/` filters headlines as you type.
- **Modern dark theme** — Catppuccin Mocha–inspired, with no pure-black background.

---

## Install

**Requirements:** the [Rust toolchain](https://rustup.rs) (`cargo`/`rustc`).

On Windows you also need a C compiler for the bundled SQLite — either the Visual Studio "Desktop
development with C++" workload (or the standalone Build Tools), or the `x86_64-pc-windows-gnu`
toolchain.

For the rounded pill-style tabs to render, use a [Nerd Font](https://www.nerdfonts.com) in your
terminal. Without one, dnews falls back to plain-ASCII tabs on narrow terminals, and the pill glyphs
may show as boxes on wide ones.

### Linux / macOS

*(also works from Git Bash on Windows)*

```bash
git clone https://github.com/ret3030/dnews
cd dnews
./install.sh
```

### Windows (PowerShell)

```powershell
git clone https://github.com/ret3030/dnews
cd dnews
.\install.ps1
```

Both scripts build a release binary, install it, and copy `feeds.opml` into the OS config directory
if you don't already have one there.

| OS | Binary | Config | Database |
|----|--------|--------|----------|
| Linux | `~/.local/bin/dnews` | `~/.config/dnews/feeds.opml` | `~/.local/share/dnews/dnews.db` |
| macOS | `~/.local/bin/dnews` | `~/Library/Application Support/dnews/feeds.opml` | `~/Library/Application Support/dnews/dnews.db` |
| Windows | `%LOCALAPPDATA%\Programs\dnews\dnews.exe` | `%APPDATA%\dnews\feeds.opml` | `%APPDATA%\dnews\data\dnews.db` |

Make sure the binary directory is on your `PATH`.

---

## Configuration

Everything lives in **one file**: `feeds.opml`. Top-level `<outline>` folders become category tabs,
and each `<outline>` with an `xmlUrl` is a feed.

```xml
<opml version="1.0">
  <body>
    <outline text="World">
      <outline type="rss" text="BBC" xmlUrl="https://feeds.bbci.co.uk/news/world/rss.xml"/>
      <outline type="rss" text="Reuters" xmlUrl="https://www.reuters.com/rssfeed/worldNews"/>
    </outline>
    <outline text="Tech">
      <outline type="rss" text="Hacker News" xmlUrl="https://hnrss.org/frontpage"/>
    </outline>
  </body>
</opml>
```

dnews resolves the file in this order:

1. `./feeds.opml` in the current working directory — handy for keeping a project-local feed list
2. the OS config path from the table above

---

## Usage

```bash
dnews
```

### Narrow terminal

| Key | Action |
|-----|--------|
| `Enter` | Open the selected article full-screen (marks it read) |
| `q` / `Esc` | Back to the list (from the article) / quit (from the list) |
| `Shift+J` / `Shift+K` *(in the article)* | Jump to the next / previous article without leaving the reader |

### Wide terminal (split list + article panel)

Keyboard focus toggles between the list and the panel — it never switches just from moving the
selection:

| Key | Action |
|-----|--------|
| `Enter` | Load the selection into the panel, mark it read, and focus it — plain vim keys now scroll the article |
| `q` / `Esc` *(panel focused)* | Unfocus back to the list; the panel clears |
| `q` *(list focused)* | Quit |
| `Shift+J` / `Shift+K` | Move the list selection and live-preview it in the panel, without changing which pane plain keys control |

### Both layouts

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Next / previous tab (All → categories → Saved) |
| `s` | Save / unsave the currently relevant article |
| `Ctrl+R` | Reload feeds (non-blocking, runs in the background) |
| `/` | Search headlines live |
| `j` / `k`, arrows | Move down / up |
| `g` / `G` | Jump to top / bottom |
| `d` / `u`, PageDown / PageUp | Page down / up |

---

## How it works

```
feeds.opml ──▶ OPML parser ──▶ concurrent fetch (16 at a time)
                                      │
                                      ▼
                              feed_rs parser
                                      │
                                      ▼
                        SQLite writer thread (upsert by link)
                                      │
       ┌──────────────────────────────┴───────────────┐
       ▼                                              ▼
  article list ◀── ratatui event loop ──▶ Readability extraction
                                          (cached back into SQLite)
```

The main loop is a `tokio::select!` over terminal input, feed-reload progress, article loading, and a
debounced preview timer — so no network or database work ever blocks a keystroke. Feed fetches fan
out across up to 16 concurrent tasks and funnel their results through a single writer thread that
owns the SQLite connection.

---

## Development

```bash
cargo build --release      # build only
cargo test                 # fast offline tests
cargo test -- --ignored    # network-backed tests (real feeds, real articles)
cargo clippy --all-targets # lints
```

The codebase is platform-agnostic — no `cfg(target_os)`, no POSIX syscalls, `reqwest` on rustls, and
`rusqlite` bundled — so the same source builds on all three platforms.

---

## Support

If dnews saves you time, you can support its development:

<a href="https://buymeacoffee.com/ret3030">
  <img src="https://img.shields.io/badge/Buy%20Me%20a%20Coffee-ret3030-FFDD00?style=for-the-badge&logo=buymeacoffee&logoColor=black" alt="Buy Me a Coffee">
</a>

---

## About the developer

Built by **[Robert Plevac](https://github.com/ret3030)** ([Velrion Solutions](https://github.com/ret3030)) —
a developer who lives in the terminal and builds tools that stay out of the way.

dnews started as a shell script gluing together newsboat, fzf, and rdrview, and was rewritten from
scratch as a single native Rust binary once the seams in that stack got in the way more than they
helped. That's the throughline: take a workflow that works but drags, and rebuild it as something
fast, self-contained, and pleasant to use every day.

Feedback, bug reports, and pull requests are welcome — open an
[issue](https://github.com/ret3030/dnews/issues).
