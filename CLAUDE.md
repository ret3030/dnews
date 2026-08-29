# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

**dnews** is a terminal RSS reader built by gluing together three external tools with shell scripts:
`newsboat` (feed fetching/storage into a SQLite cache), `fzf` (the interactive list UI), and `rdrview`
(Reader View article extraction, same tech as Firefox). There is no compiler, package manager, or test
suite — this is a bash/python script collection deployed by copying files into `~/.config/newsboat/`.
It's a deliberately light two-screen tool: a list, and (on `Enter`) a full-screen article view — no
preview pane, no background refresh daemon, no periodic reload.

## Install / deploy

```bash
./install.sh
```

Copies `dnews.sh reader.sh header.sh mark_read.sh strip_links.lua colorize.py categories.sh
list_query.sh` and `newsboat.config` (renamed to `config`) into `~/.config/newsboat/`, `chmod +x`s the
shell scripts, and checks for dependencies (`fzf`, `newsboat`, `sqlite3`, `rdrview`, `pandoc`, `python3`,
`less`, `python-ftfy`). If `feeds.opml` is present, install.sh also **regenerates**
`~/.config/newsboat/urls` from it (backing up any existing `urls` to `urls.bak` first) via an inline
Python OPML→urls converter — this replaces the feed list rather than appending to it, unlike newsboat's
own `-i/--import-from-opml` flag (which only appends and requires the target `urls` file to already
exist).
**There is no uninstall/sync-back script** — after editing any file in this repo, re-run `./install.sh`
(or manually `cp`) to deploy the change, since the live copy in `~/.config/newsboat/` is what actually
runs.

There is no linter or test suite to run; `bash -n <script>.sh` / `shellcheck` are reasonable manual
sanity checks before committing but nothing in the repo enforces them.

## Runtime architecture

Everything reads/writes newsboat's SQLite cache directly at `~/.local/share/newsboat/cache.db`
(table `rss_item`, columns used: `title`, `pubDate`, `url`, `unread`, `feedurl`) — there is no newsboat
API layer, just raw `sqlite3` queries embedded in the shell scripts. The list query itself lives in one
place, `list_query.sh` (`SELECT title, pubDate, url, unread FROM rss_item [WHERE feedurl IN (...)]
ORDER BY pubDate DESC`, piped through `colorize.py`) — `dnews.sh`'s initial pipeline and its `f1`-`f9`/
`esc`/`ctrl-r` fzf bindings all call it rather than embedding SQL, since it takes an optional category
argument (see Category tabs below). There is no unread/all mode split any more: the query always
returns every article (optionally scoped to a category), and `colorize.py` dims already-read rows
instead of hiding them.

### Category tabs

`categories.sh` extracts the distinct newsboat tags from `~/.config/newsboat/urls` (in first-seen
order) — these are exactly the top-level OPML `<outline>` folder names from `feeds.opml` (e.g.
"Zprávy & Trhy", "Tech & Dev"), since `install.sh`'s OPML→urls converter turns each folder into a tag
on every feed inside it. `dnews.sh` reads this list at startup and builds one fzf key binding per
category — `f2` through `f9` (max 8 tabs; `f1` is the hardcoded "All" tab) — each of which writes the
category name to a per-process state file (`/tmp/dnews_filter_$$`, cleaned up via `trap ... EXIT`) and
reloads the list through `list_query.sh '<category>'`. The `esc` and `ctrl-r` bindings read that same
state file at execute time (`$(cat $FILTER_STATE)`) so they preserve whichever tab is active instead of
resetting to "All" — `esc` only clears the search text now, it does not change tabs. `header.sh` also
takes the state file path as `$1` and renders the tab list inline (single line, active tab bold/bright,
others dim) alongside the unread count; since it's called via `transform-header(...)` on every relevant
binding, keep any new binding that changes the tab or article count passing `$FILTER_STATE` through to
it.

Data flow, entry point `dnews.sh`:

1. **Reload feeds** — `newsboat -d <tmp log> -l 5 -x reload` runs in the background (`-l 5` = INFO-level
   logging) while a progress bar polls the log for `Reloader::reload: starting reload of <url>` lines to
   count feeds fetched so far, against a total read from `~/.config/newsboat/urls`. Critically, the
   script explicitly `wait`s on the reload's PID before querying the DB for the initial list — a prior
   version backgrounded the reload and queried the DB immediately without waiting, which is what caused
   articles to sometimes not appear until a manual refresh. This is the only reload that happens
   automatically — there is no periodic/background auto-refresh anywhere in the codebase; a further
   reload only ever happens on explicit `ctrl-r`.
2. **List rows** — `sqlite3` queries `rss_item` (all articles, not just unread), using `\x01` as a field
   separator (titles/URLs may contain commas/pipes), piped through `colorize.py`.
3. **colorize.py** transforms each row into a **two-line, NUL-terminated record** (fzf's `--read0`
   multi-line item format): line 1 is `NN. Title (domain)`, line 2 is an indented `TAG · Xm/h/d ago`
   meta line — TAG is a 3-letter source tag derived from the domain (color-hashed via md5 so each source
   gets a stable palette color). Read articles (`unread = 0`) render both lines in uniform dim gray
   instead of title-bold/tag-colored. The record is `line1\nline2\x01pubdate\x01url\x01raw_title`,
   terminated with `\0` — field 4 carries the untouched title text specifically so `reader.sh` doesn't
   have to regex it back out of the styled/multi-line display text.
4. **fzf** runs with `--read0` (NUL-delimited, multi-line-aware records) and
   `--delimiter $'\x01' --with-nth 1` (only field 1 — the two display lines — is shown/searched; fields
   2/3/4 = pubDate/url/raw-title stay available to key bindings via `{2}`/`{3}`/`{4}`).
5. **Key bindings** (all inline in `dnews.sh`, no separate config): `enter` runs `reader.sh` (see below)
   via `execute(...)`, which takes over the terminal and returns to the list on exit; `/` shows the
   search input; `f1`-`f9` switch category tabs (see Category tabs above); `esc` clears the search text
   and reloads the current tab (does not change tabs); `ctrl-r` force-reloads feeds from network in the
   background and immediately re-queries the current tab (there's no live progress bar for this one — it
   just picks up whatever's already in the cache, so a second `ctrl-r` shortly after may be needed to see
   newly-fetched content). There is no `focus`-triggered mark-as-read any more — articles are only marked
   read when actually opened, in `reader.sh`.
6. **Reader view** — `reader.sh` is invoked on `enter` in place of the old fzf preview pane. It marks the
   article read via `mark_read.sh` immediately, then fetches the body via `rdrview -T title,body -H
   <url>` (4s timeout), strips JSON/boilerplate noise, fixes encoding with `ftfy`, converts HTML→plain
   text with `pandoc` using `strip_links.lua` (drops the first `<h1>` since the title is already shown,
   and inlines link text/discards images), renders it inside a box-drawing header card (title/date/
   domain) with a colored left-rule (`▏`) accent down each body line sized to the full terminal width
   (capped at 100 cols), and pipes the whole thing through `less -R` so long articles scroll — `q` exits
   back to the fzf list. If rdrview/pandoc yield no content (fetch failure, timeout, paywall), it falls
   back to opening the URL directly with `xdg-open`.

## Style notes specific to this repo

- Gruvbox color palette (fixed 256-color codes, e.g. `#282828`/`#fabd2f`/`#fe8019`) and Nerd Font glyphs
  (``, ``, etc.) are used throughout for the fzf theme, header, and reader view — match these
  when adding UI elements rather than introducing new colors/icons ad hoc. The look favors a flat,
  minimal list (no borders/boxes in the fzf UI itself — `--border=none`) over heavy chrome.
- Scripts assume they're deployed at `~/.config/newsboat/` and reference each other via that absolute
  path (e.g. `dnews.sh` calls `~/.config/newsboat/reader.sh`), not relative paths — this must be
  preserved for `install.sh`'s flat copy to keep working.
- `open.sh` is a paywall-workaround opener (checks for 404/410/timeout and falls back to
  `archive.ph/<url>`) but is not currently wired into `reader.sh`'s fallback, which uses a plain
  `xdg-open`.
- There is intentionally no standalone background-reload script (a prior `sync.sh` loop was removed) and
  no cron/systemd integration — keeping the tool's own footprint to "reload once at launch, or on
  `ctrl-r`" was an explicit design choice, not an oversight.
