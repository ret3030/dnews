# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

**dnews** is a terminal RSS reader — a single Rust binary using [ratatui](https://ratatui.rs) for the
TUI. It replaced an earlier bash/fzf/newsboat/rdrview version (see git history before the Rust
rewrite) — that generation glued five external processes together via subprocess calls and inline
SQL; this one does feed fetching, storage, and rendering natively in one process, with no runtime
dependency on newsboat, fzf, rdrview, pandoc, or `less`.

## Build / install / deploy

```bash
cargo build --release   # or just ./install.sh, which also installs it
./install.sh            # builds, copies the binary to ~/.local/bin/dnews,
                         # seeds ~/.config/dnews/feeds.opml if none exists
```

`install.sh` detects the OS (`uname -s`) and picks per-platform install/config paths: Linux uses
`~/.local/bin` + `${XDG_CONFIG_HOME:-~/.config}/dnews`, macOS uses `~/.local/bin` +
`~/Library/Application Support/dnews`, and Git-Bash/MSYS-on-Windows uses `%LOCALAPPDATA%\Programs\dnews`
+ `%APPDATA%\dnews`. `install.ps1` is the native-PowerShell equivalent for Windows without bash (same
paths as the Git-Bash branch). All three config locations match what `directories`' `BaseDirs`/
`ProjectDirs` resolve to at runtime, so a seeded `feeds.opml` lands where the binary looks for it. The
code itself is platform-agnostic (no `cfg(target_os)`, no POSIX syscalls, `reqwest` on `rustls` so no
system TLS, `rusqlite` bundled so no system SQLite — but the bundled SQLite `cc` build needs an MSVC
or MinGW C toolchain on Windows).

There is no separate "deploy" step beyond copying the built binary — unlike the old bash version,
there's nothing to keep in sync between a repo copy and a live copy; the binary is self-contained.

`cargo test` runs fast/offline tests. Several tests hit the real network and are `#[ignore]`d by
default — run them explicitly with `cargo test -- --ignored` when touching `feed/fetch.rs` or
`reader.rs`: `feed::fetch::tests::fetches_a_real_feed`, `reader::tests::extracts_a_real_article`, and
`reader::tests::extracts_noscript_lazy_image_from_a_real_article`. `cargo clippy --all-targets` should
stay clean.

## Runtime architecture

Entry point `src/main.rs`: resolves `feeds.opml` (repo-local `./feeds.opml` takes priority, else
`~/.config/dnews/feeds.opml`) and the SQLite DB path (`~/.local/share/dnews/dnews.db`, via the
`directories` crate), opens/creates the DB, prunes old articles once (see `prune_old_articles` below),
then runs a `tokio::select!` event loop over four sources: terminal input
(`crossterm::event::EventStream`), feed-reload progress (`feed::fetch::FetchEvent` on an mpsc
channel), reader-content loading (`app::ReaderEvent` on a separate mpsc channel), and a debounced
preview timer (`app.preview_due_at`, see Category tabs / split layout below) — plus a 90ms tick while
`app.loading || app.reader_loading` that both animates the progress bar and advances `app.spin_frame`
(the braille spinner used in both loading states, see `ui::spinner`). Every iteration redraws via
`ui::draw`. `wide` (terminal width ≥ `ui::SPLIT_MIN_WIDTH`) is recomputed once per loop iteration and
threaded into `keys::handle`, since it changes which key bindings apply (see Split layout below).

- **`store.rs`** — the only SQLite access point (`rusqlite`, bundled — no system SQLite dependency).
  One `articles` table (`title`, `link` UNIQUE, `published`, `unread`, `saved`, `content_text`);
  `category` is a plain column, queried through `ListFilter` (`All` / `Category(String)` / `Saved`) —
  `Store::list` takes a `&ListFilter` rather than an `Option<&str>` so the "Saved" pseudo-category
  doesn't need a fake category string. `content_text` caches the Readability-extracted body so
  reopening a read article doesn't refetch it. `init_schema` both creates the table fresh (with
  `saved` included) and separately runs an `ALTER TABLE ... ADD COLUMN saved` wrapped to ignore the
  "duplicate column" error, so older dev DBs from before the save feature don't need to be deleted.
  `prune_old` deletes unsaved articles older than a cutoff (`saved = 0` is part of the `WHERE`, so
  bookmarks are never touched regardless of age). `Store::backup()` copies the DB file to a sibling
  `.bak` path (rolling, single generation) — called from `main.rs` right before `init_schema()` runs on
  every startup, best-effort (a failed backup must not block startup). This isn't protecting against
  binary updates (the DB already lives outside the repo/binary, in `~/.local/share/dnews/`, so
  updating the binary alone can't touch it) — it's a safety net against a future schema-migration bug
  in `init_schema()` silently losing saved articles; restore with `cp dnews.db.bak dnews.db`.
  `Store::open` is cheap (just stores a path) — a fresh `rusqlite::Connection` is opened per call
  rather than held, since writes happen both from the main task and from a dedicated writer thread
  (see below); don't introduce a shared `Connection` across threads without re-checking this.
- **`feed/opml.rs`** — parses `feeds.opml` via `roxmltree` (DOM-style, not a streaming parser — the
  file is small) into a flat `Vec<Feed>`, recursively walking `<outline>` elements: a node with an
  `xmlUrl` attribute is a feed (tagged with its *immediate parent's* title/text as `category`, single
  value, not a path — matches how `feeds.opml` is actually structured, one level of folder nesting); a
  node without one is a category folder, recursed into.
- **`feed/fetch.rs`** — `spawn_reload` is fired on startup and on `Ctrl+R`, and does **not** block the
  UI (a key difference from the old `newsboat -x reload` + `wait`, which froze the whole terminal
  during reload). It fans out one `tokio::spawn` per feed (bounded to `MAX_CONCURRENT = 16` via a
  `Semaphore`), each doing `reqwest` GET → `feed_rs::parser::parse`. Since `rusqlite::Connection` isn't
  `Sync`, writes don't happen inline in the async fetch tasks — each task sends its parsed
  `Vec<NewArticle>` over a plain `std::sync::mpsc` channel to a dedicated `std::thread` that owns one
  `Connection` and upserts batches (`ON CONFLICT(link) DO UPDATE`) sequentially. `FetchEvent::Started`
  /`FeedDone`/`Complete`/`Error` go out on a separate `tokio::sync::mpsc` channel that the main loop
  in `main.rs` consumes to drive the progress bar, prune old articles, and re-query the list
  (`app.reload_view()`) once `Complete` fires.
- **`reader.rs`** — `fetch_article` does the `reqwest` GET, then hands the HTML to
  `readability_rust::Readability` inside `spawn_blocking` (Readability's DOM parsing is
  CPU-bound/sync, so it shouldn't run on the async executor thread). Deliberately uses
  `article.content` (HTML), **not** `article.text_content` — `text_content` collapses everything into
  one run-on block with no paragraph breaks. `html_to_text()` walks the HTML itself (via `scraper` +
  `ego-tree`, the same crates `readability-rust` uses internally, added as direct deps so we can
  reference `scraper::{Html, Node}`/`ego_tree::NodeRef` directly) inserting a blank line around each
  block-level element, a `[image: alt]` placeholder for every `<img>` (there's no inline image
  rendering in a plain terminal buffer, but at least the reader knows one was there), and folds
  `<tr>`/`<td>` into one `·`-joined line per row (some sites, e.g. sports-score widgets, emit bare
  table markup with no `<table>` ancestor — `html_to_text` wraps it in a synthetic one first so the
  parser doesn't drop it as a stray tag). `<noscript>`-wrapped lazy-loaded images are unwrapped: HTML5
  treats `<noscript>` content as RAWTEXT, so a fallback `<img>` inside one arrives HTML-escaped and
  undecoded — `decode_entities` + re-parse recovers it instead of leaking `&lt;img...&gt;` as visible
  text. `normalize_blank_lines()` then collapses whitespace-within-lines and caps consecutive blank
  lines at exactly one.
- **`app.rs`** — `App` holds all UI state. `filter: ListFilter` drives `cycle_filter(dir)` (bound to
  `Tab`/`Shift+Tab`), which walks an ephemeral `[All, categories..., Saved]` list and wraps at both
  ends. `open_selected` (narrow terminals) and `activate_selected` (wide split layout) both funnel
  through the private `load_selected`, which marks the article read optimistically in both the
  in-memory row and the DB *before* the body is available, then either serves `content_text` straight
  from the DB row if already cached, or spawns a background task that calls `reader::fetch_article`
  and reports back via `ReaderEvent` — so opening an article never blocks the event loop, and also
  caches the fetched text back into `content_text` for next time. `step_reader(delta, ...)` moves
  `selected` (via `move_selection`, clamped, no wraparound) and re-runs `load_selected` in one call —
  bound to Shift+J/K inside the narrow full-screen reader (`keys::handle_reader`) so you can page
  through articles without dropping back to the list; no debounce needed here (unlike the wide split
  layout's preview) since every keypress already commits to viewing that article, and the existing
  stale-`ReaderEvent` guard already protects against rapid key-repeat racing a slow fetch. `reader_article`
  is a **snapshot** of the opened article, not a live lookup via `selected`/`articles` — see its doc
  comment for why (a background reload reordering `articles` must not repoint what the reader is
  showing).
  `toggle_saved_selected` branches on `screen`: in the narrow reader it flips `reader_article`'s
  `saved` (syncing the copy in `articles`); on the list it flips the selected row directly, splicing
  it out of view immediately if unsaved while on the Saved tab. `prune_old_articles` is called once at
  startup and again after every completed reload (not just once — a slow-rotating feed like FRED/ČNB
  can keep re-serving the same old item in its current XML, and `upsert_batch` doesn't filter by age,
  so pruning only before the first reload would let that reload immediately reintroduce whatever was
  just pruned).
- **`keys.rs`** — all keybindings, split by `Screen`, and further split by `wide` for `Screen::List`
  (see Split layout below). `search_active` intercepts *all* character input on the list screen so
  typing in search doesn't trigger navigation/save/tabs — check this branch first when adding a new
  list-screen binding. `scroll_list` (list navigation: move/top/bottom/page/save) and `scroll_article`
  (reader scrolling: same shape, on `reader_scroll`) are the two reusable cores shared across all four
  places vim keys can apply — narrow list, narrow full-screen reader, and the wide layout's list/panel
  depending on focus.
- **`ui/`** — `list.rs` (pill-style tab bar + two-line-per-row article list + footer/search-bar/
  progress-bar, all built from real `ratatui` `Line`/`Span`s, not ANSI-escaped strings — the tab pills
  use the powerline round-cap glyphs `\u{e0b6}`/`\u{e0b4}` around the active tab, which need a Nerd
  Font to render correctly) and `article.rs` (article view: a centered `READ_WIDTH`-capped column with
  a title/date/domain header card and a scrollable `Paragraph` using manual `(scroll_y, 0)` offset from
  `app.reader_scroll`; lines starting with `[image:` get a distinct italic/mauve style). Both take a
  bool (`split`/`panel`) distinguishing narrow single-pane rendering from the wide split layout, which
  changes footer hint text; `article.rs`'s panel footer further branches on `app.panel_focused`. Both
  are also responsive at narrow widths rather than just clipping: below `list.rs`'s
  `COMPACT_TABS_MAX_WIDTH` (70 cols), `render_tabs` swaps the Nerd-Font-dependent pill tabs for a
  plain-ASCII `compact_tabs_line` (`·`-separated, bold-accent for the active tab, no glyphs that can
  misrender without a Nerd Font); and both screens size their footer row dynamically
  (`footer_hint_lines`) instead of a fixed `Constraint::Length(1)`, wrapping the hint text onto a
  second line via `super::wrap_hints` (greedy phrase-packing that never splits a hint mid-phrase)
  rather than letting it clip. The live search box uses `super::truncate_start` (ellipsis at the
  *start*, not the end) so the cursor at the end of what you just typed stays visible instead of
  scrolling off-screen on a narrow terminal; the loading progress bar's fill width also shrinks to fit
  whatever room is left after its prefix/suffix text. `theme.rs` holds a Catppuccin-Mocha-inspired dark
  palette (never pure black — that was explicit user feedback on an earlier Gruvbox-black pass) as
  `Color::Rgb` constants, plus a small FNV-1a-hash-based `source_color()` used to tint each row's
  domain name (deterministic on purpose — std's default hasher is randomly seeded per process and would
  reshuffle colors every run). `ui::mod` also has `SPLIT_MIN_WIDTH` (the wide-layout threshold),
  `centered()` (caps content width on wide terminals), `truncate()`/`truncate_start()` (ellipsize long
  text, tail- or head-preserving), `wrap_hints()` (phrase-packing word wrap), and `spinner()` (a
  10-frame braille spinner driven by `app.spin_frame`).

### Split layout and panel focus

At or above `ui::SPLIT_MIN_WIDTH`, `ui::draw` renders the list and the article view side by side
instead of one full-screen `Screen` at a time (`screen` stays `Screen::List` the whole time in this
mode — there's no `Screen::Reader` transition here). Which pane plain vim keys drive is tracked by
`App::panel_focused`, toggled only by explicit keys, never by mere navigation:

- **Not focused** (the default, and after `q`/`Esc` un-focuses): plain `j`/`k`/`g`/`G`/`d`/`u`/`s`
  behave exactly like the narrow list screen (`scroll_list`) — moving the selection alone never loads
  or marks anything read.
- **`Enter`**: calls `App::activate_selected` (loads the selection into the panel, marks it read) and
  sets `panel_focused = true`. From then on plain vim keys scroll the panel (`scroll_article`) instead
  of moving the list selection.
- **`q`/`Esc` while focused**: `App::unfocus_panel` hands control back to the list *and clears the
  panel* (`reader`/`reader_article`/etc. reset to `None`) — returning to plain browsing shows an empty
  panel, not the last-read article lingering.
- **Shift+J / Shift+K**: independent of focus state, always move the list selection and call
  `App::schedule_preview` (~150ms debounce before `activate_selected` fires from the main loop's
  `preview_due_at` timer) — this is what gives you a live preview while browsing without changing
  which pane plain keys control. The debounce exists so holding the key down doesn't fire a fetch per
  article scrolled past.

## Style notes specific to this repo

- Keep the theme palette in `ui/theme.rs` as the single source of truth for colors — don't inline
  hex/RGB values elsewhere in `ui/`. It's intentionally *not* pure black (`theme::BG` has a slight
  blue-slate hue) — don't revert that without checking with the user first, it was a direct complaint.
- No blocking calls inside the main `tokio::select!` loop in `main.rs` or inside any `async fn` that
  runs on it — network/DB work belongs in a `tokio::spawn`ed task (see `feed::fetch::spawn_reload` and
  `App::load_selected`) reporting back over a channel, following the existing `FetchEvent`/
  `ReaderEvent` pattern rather than adding new blocking paths.
- `rusqlite::Connection` is opened fresh per `Store` method call rather than cached — cheap for
  SQLite, and sidesteps `Send`/`Sync` issues across the writer-thread/async-task split described
  above. Don't "optimize" this into a shared long-lived connection without accounting for that.
- No mouse capture (`main.rs` deliberately doesn't enable it) — that would take over the terminal's
  native click-drag text selection, which matters more here than custom scroll-wheel handling; most
  terminals still translate the wheel into arrow-key presses on their own when capture is off.
- The outer app border is top/bottom only, not full box — a full box's left/right `│` would sit on
  every content row and get dragged into a copy-paste selection when reading an article.
