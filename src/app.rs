use std::cell::Cell;
use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::mpsc;

use crate::feed::Feed;
use crate::feed::fetch::{self, FetchEvent};
use crate::reader::{self, ReaderContent};
use crate::store::{ArticleRow, ListFilter, Store};

const COMPACT_KEY: &str = "compact";
const READER_WIDE_KEY: &str = "reader_wide";

/// Unsaved articles older than this get deleted by `prune_old_articles`.
const PRUNE_AFTER_SECS: i64 = 60 * 24 * 60 * 60;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Screen {
    List,
    Reader,
}

pub enum ReaderEvent {
    /// Carries the article `link` the fetch was for, so a late response for
    /// an article the user has since navigated away from can be told apart
    /// from the one currently on screen (see `App::reader_article`).
    Loaded(String, ReaderContent),
    Failed(String, String),
}

/// A modal panel drawn over the current screen; any key closes it.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Overlay {
    None,
    Help,
    FeedErrors,
}

pub struct App {
    pub db: Store,
    pub feeds: Vec<Feed>,
    pub categories: Vec<String>,
    pub filter: ListFilter,

    pub articles: Vec<ArticleRow>,
    pub filtered: Vec<usize>,
    pub selected: usize,

    pub search: String,
    pub search_active: bool,

    pub screen: Screen,
    pub reader: Option<ReaderContent>,
    /// A snapshot of the article currently shown (or loading) in the reader,
    /// taken at the moment it was opened. Deliberately **not** looked up via
    /// `selected`/`articles` on every render: a background feed reload can
    /// rebuild `articles` (new items shift everyone's sort position) while
    /// the user is reading, which would silently repoint `selected` at a
    /// different article and show its title/date over the real one's body.
    /// Also doubles as the article-identity check for a late `ReaderEvent`
    /// (see `apply_reader_event`).
    pub reader_article: Option<ArticleRow>,
    pub reader_scroll: u16,
    /// Largest useful `reader_scroll` for the article as last rendered:
    /// wrapped line count minus the visible body height. Written by the
    /// renderer (which only gets `&App`, hence the `Cell`) and read by the
    /// scroll keys so `G`, `j` and `d` stop at the end of the text instead of
    /// scrolling into blank space. `u16::MAX` until the first render.
    pub reader_max_scroll: Cell<u16>,
    pub reader_loading: bool,
    pub reader_error: Option<String>,
    /// Wide text column in the reader. Persisted in the DB.
    pub reader_wide: bool,
    /// Digits typed in the reader to pick a link (`12` then Enter).
    pub link_input: String,

    pub unread_count: i64,
    /// Unread articles per category, for the tab labels.
    pub tab_unread: HashMap<String, i64>,
    /// One line per article instead of title + meta. Persisted in the DB.
    pub compact: bool,
    pub loading: bool,
    pub total_feeds: usize,
    pub done_feeds: usize,
    pub status: String,
    /// Feeds that failed during the last reload: (feed name, error).
    pub feed_errors: Vec<(String, String)>,
    /// A one-off confirmation ("Link copied") shown in the footer until the
    /// next key press.
    pub notice: Option<String>,
    pub overlay: Overlay,
    pub spin_frame: usize,

    /// In the wide split layout only: whether plain vim keys currently
    /// target the preview panel (`true`, set by `Enter`) or the list
    /// (`false`, the default) — see `keys::handle_list_wide`. Independent
    /// of `screen`, which always stays `List` in that layout since both
    /// panes are drawn at once.
    pub panel_focused: bool,
    /// When set, the main loop's debounce timer calls `activate_selected`
    /// at this instant — used by Shift+J/Shift+K (see `keys.rs`) to preview
    /// the newly-focused list item a beat after browsing settles, rather
    /// than firing a fetch for every article scrolled past while the key
    /// is held down.
    pub preview_due_at: Option<std::time::Instant>,

    pub should_quit: bool,
}

impl App {
    pub fn new(db: Store, feeds: Vec<Feed>) -> Self {
        let mut categories = Vec::new();
        for f in &feeds {
            if !f.category.is_empty() && !categories.contains(&f.category) {
                categories.push(f.category.clone());
            }
        }
        let flag = |key| db.setting(key).ok().flatten().as_deref() == Some("1");
        let compact = flag(COMPACT_KEY);
        let reader_wide = flag(READER_WIDE_KEY);

        Self {
            db,
            feeds,
            categories,
            filter: ListFilter::All,
            articles: Vec::new(),
            filtered: Vec::new(),
            selected: 0,
            search: String::new(),
            search_active: false,
            screen: Screen::List,
            reader: None,
            reader_article: None,
            reader_scroll: 0,
            reader_max_scroll: Cell::new(u16::MAX),
            reader_loading: false,
            reader_error: None,
            reader_wide,
            link_input: String::new(),
            unread_count: 0,
            tab_unread: HashMap::new(),
            compact,
            loading: false,
            total_feeds: 0,
            done_feeds: 0,
            status: String::new(),
            feed_errors: Vec::new(),
            notice: None,
            overlay: Overlay::None,
            spin_frame: 0,
            panel_focused: false,
            preview_due_at: None,
            should_quit: false,
        }
    }

    pub fn start_reload(&mut self, tx: &mpsc::UnboundedSender<FetchEvent>) {
        self.loading = true;
        self.status.clear();
        self.feed_errors.clear();
        fetch::spawn_reload(self.feeds.clone(), self.db.path().to_path_buf(), tx.clone());
    }

    /// Deletes unsaved articles older than [`PRUNE_AFTER_SECS`]. Must be
    /// called after *every* reload completes, not just once at startup —
    /// a feed can keep re-serving the same old item in its current XML
    /// (slow-rotating feeds like FRED/ČNB do this), and `upsert_batch`
    /// doesn't filter by age, so pruning only before the first reload would
    /// let that reload immediately reintroduce whatever was just pruned.
    pub fn prune_old_articles(&self) {
        let cutoff = chrono::Utc::now().timestamp() - PRUNE_AFTER_SECS;
        let _ = self.db.prune_old(cutoff);
    }

    pub fn reload_view(&mut self) -> Result<()> {
        self.articles = self.db.list(&self.filter)?;
        self.refresh_counts();
        self.apply_search();
        Ok(())
    }

    /// Re-reads the unread totals from the DB after anything marks articles.
    pub fn refresh_counts(&mut self) {
        self.unread_count = self.db.unread_count().unwrap_or(self.unread_count);
        if let Ok(by_category) = self.db.unread_by_category() {
            self.tab_unread = by_category;
        }
    }

    pub fn toggle_reader_wide(&mut self) {
        self.reader_wide = !self.reader_wide;
        let _ = self
            .db
            .set_setting(READER_WIDE_KEY, if self.reader_wide { "1" } else { "0" });
    }

    /// Remembers how far the open article was read, so reopening it
    /// continues there.
    fn save_scroll(&mut self) {
        let Some(article) = self.reader_article.as_ref() else {
            return;
        };
        let pos = self.reader_scroll.min(self.reader_max_scroll.get());
        if pos == article.scroll_pos {
            return;
        }
        let link = article.link.clone();
        let _ = self.db.set_scroll(&link, pos);
        self.update_article(&link, |a| a.scroll_pos = pos);
    }

    /// An article can be on screen twice: its row in `articles` and the
    /// `reader_article` snapshot. Every change goes through here so the two
    /// never disagree.
    fn update_article(&mut self, link: &str, change: impl Fn(&mut ArticleRow)) {
        if let Some(row) = self.articles.iter_mut().find(|a| a.link == link) {
            change(row);
        }
        if let Some(open) = self.reader_article.as_mut().filter(|a| a.link == link) {
            change(open);
        }
    }

    /// Opens link `n` (1-based) of the article in the browser.
    pub fn open_link(&mut self, n: usize) {
        let url = self
            .reader
            .as_ref()
            .and_then(|c| c.links.get(n.wrapping_sub(1)))
            .cloned();
        self.notice = Some(match url {
            None => format!("No link {n}"),
            Some(url) => match crate::platform::open_url(&url) {
                Ok(()) => format!("Opened link {n}"),
                Err(e) => format!("Could not open browser: {e}"),
            },
        });
    }

    pub fn toggle_compact(&mut self) {
        self.compact = !self.compact;
        let _ = self
            .db
            .set_setting(COMPACT_KEY, if self.compact { "1" } else { "0" });
    }

    pub fn apply_search(&mut self) {
        let needle = self.search.to_lowercase();
        self.filtered = self
            .articles
            .iter()
            .enumerate()
            .filter(|(_, a)| needle.is_empty() || a.title.to_lowercase().contains(&needle))
            .map(|(i, _)| i)
            .collect();
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
    }

    pub fn set_filter(&mut self, filter: ListFilter) -> Result<()> {
        self.filter = filter;
        self.selected = 0;
        self.reload_view()
    }

    /// Tab/Shift-Tab cycle through All -> each category -> Saved -> All.
    pub fn cycle_filter(&mut self, dir: i32) -> Result<()> {
        let mut stops: Vec<ListFilter> = vec![ListFilter::All];
        stops.extend(self.categories.iter().cloned().map(ListFilter::Category));
        stops.push(ListFilter::Saved);

        let current = stops.iter().position(|f| f == &self.filter).unwrap_or(0);
        let len = stops.len() as i32;
        let next = (current as i32 + dir).rem_euclid(len) as usize;
        self.set_filter(stops[next].clone())
    }

    pub fn move_selection(&mut self, delta: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let len = self.filtered.len() as i32;
        let mut next = self.selected as i32 + delta;
        next = next.clamp(0, len - 1);
        self.selected = next as usize;
    }

    pub fn move_to_top(&mut self) {
        self.selected = 0;
    }

    pub fn move_to_bottom(&mut self) {
        self.selected = self.filtered.len().saturating_sub(1);
    }

    /// Toggles saved-for-later on the current article.
    pub fn toggle_saved(&mut self) {
        let Some(article) = self.current_article() else {
            return;
        };
        let (link, saved) = (article.link.clone(), !article.saved);
        let _ = self.db.set_saved(&link, saved);
        self.update_article(&link, |a| a.saved = saved);

        // Unsaving in the Saved tab drops the row, unless it is being read.
        let reading = self.screen == Screen::Reader || self.panel_focused;
        if self.filter == ListFilter::Saved && !saved && !reading {
            self.articles.retain(|a| a.link != link);
            self.apply_search();
        }
    }

    /// Marks the selected article read locally (optimistic) and in the DB,
    /// switches to the full-screen reader, and kicks off content loading
    /// (cached or fetched in the background).
    pub fn open_selected(&mut self, reader_tx: &mpsc::UnboundedSender<ReaderEvent>) {
        self.load_selected(reader_tx);
        self.screen = Screen::Reader;
    }

    /// Moves the list selection to the next/previous article and loads it
    /// straight into the still-open full-screen reader — lets Shift+J/K page
    /// through articles (see `keys::handle_reader`) without dropping back to
    /// the list first, mirroring the wide split layout's Shift+J/K preview.
    /// A no-op at either end of the list, same as `move_selection`.
    pub fn step_reader(&mut self, delta: i32, reader_tx: &mpsc::UnboundedSender<ReaderEvent>) {
        self.move_selection(delta);
        self.load_selected(reader_tx);
    }

    /// Debounces an `activate_selected` call ~150ms out — called from
    /// Shift+J/Shift+K (see `keys::handle_list_wide`) so holding the key
    /// down while browsing doesn't fire a fetch for every article scrolled
    /// past, only the one it settles on.
    pub fn schedule_preview(&mut self) {
        self.preview_due_at =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(150));
    }

    /// Loads the selected article into the wide-layout preview panel (see
    /// `ui::SPLIT_MIN_WIDTH`) — bound to `Enter` there (which also focuses
    /// the panel for plain-vim-key scrolling) and to the Shift+J/K preview
    /// debounce (which does not). Does nothing if that article is already
    /// loaded there.
    pub fn activate_selected(&mut self, reader_tx: &mpsc::UnboundedSender<ReaderEvent>) {
        let Some(row) = self
            .filtered
            .get(self.selected)
            .and_then(|&i| self.articles.get(i))
        else {
            return;
        };
        if self.reader_article.as_ref().map(|a| a.link.as_str()) == Some(row.link.as_str()) {
            return;
        }
        self.load_selected(reader_tx);
    }

    /// Marks the selected article read and populates `reader`/`reader_article`
    /// with its content (cached, or fetched in the background) — shared by
    /// `open_selected` (full-screen, on Enter in narrow terminals) and
    /// `activate_selected` (the split-layout panel, on Enter there too),
    /// neither of which touches `self.screen` here.
    fn load_selected(&mut self, reader_tx: &mpsc::UnboundedSender<ReaderEvent>) {
        let Some(row_idx) = self.filtered.get(self.selected).copied() else {
            return;
        };
        let Some(row) = self.articles.get_mut(row_idx) else {
            return;
        };

        let was_unread = std::mem::replace(&mut row.unread, false);
        let row = row.clone();
        let _ = self.db.mark_read(&row.link);
        if was_unread {
            self.refresh_counts();
        }

        self.save_scroll();
        self.reader_scroll = row.scroll_pos;
        self.reader_max_scroll.set(u16::MAX);
        self.reader_error = None;
        self.link_input.clear();
        self.reader_article = Some(row.clone());

        if let Some(cached) = row
            .content_text
            .as_deref()
            .and_then(ReaderContent::from_cache)
        {
            self.reader = Some(cached);
            self.reader_loading = false;
            return;
        }

        self.reader = None;
        self.reader_loading = true;

        let link = row.link.clone();
        let db_path = self.db.path().to_path_buf();
        let tx = reader_tx.clone();

        tokio::spawn(async move {
            match reader::fetch_article(&link).await {
                Ok(content) => {
                    if let Ok(store) = Store::open(&db_path) {
                        let _ = store.cache_content(&link, &content.to_cache());
                    }
                    let _ = tx.send(ReaderEvent::Loaded(link, content));
                }
                Err(e) => {
                    let _ = tx.send(ReaderEvent::Failed(link, e.to_string()));
                }
            }
        });
    }

    pub fn close_reader(&mut self) {
        self.save_scroll();
        self.screen = Screen::List;
        self.reader = None;
        self.reader_article = None;
        self.reader_loading = false;
        self.reader_error = None;
        self.reader_scroll = 0;
    }

    /// `q`/`Esc` in the wide split layout while the panel is focused: hands
    /// plain-vim-key control back to the list and clears the panel, so
    /// browsing the list plainly again shows an empty panel rather than the
    /// last-read article lingering — the panel only lights back up through
    /// an explicit `Enter` or the Shift+J/K live preview.
    pub fn unfocus_panel(&mut self) {
        self.save_scroll();
        self.panel_focused = false;
        self.reader = None;
        self.reader_article = None;
        self.reader_loading = false;
        self.reader_error = None;
        self.reader_scroll = 0;
    }

    /// The article the next action applies to: the one open in the reader
    /// (full screen or focused panel), otherwise the list selection.
    pub fn current_article(&self) -> Option<&ArticleRow> {
        if self.screen == Screen::Reader || self.panel_focused {
            return self.reader_article.as_ref();
        }
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.articles.get(i))
    }

    pub fn current_link(&self) -> Option<String> {
        self.current_article().map(|a| a.link.clone())
    }

    /// Flips read/unread on the current article, in memory and in the DB.
    pub fn toggle_read_current(&mut self) {
        let Some((link, unread)) = self.current_article().map(|a| (a.link.clone(), !a.unread))
        else {
            return;
        };
        let _ = self.db.set_unread(&link, unread);
        self.update_article(&link, |a| a.unread = unread);
        self.refresh_counts();
        self.notice = Some(
            if unread {
                "Marked unread"
            } else {
                "Marked read"
            }
            .into(),
        );
    }

    /// Marks every article currently listed (current tab and search) as read.
    pub fn mark_all_read(&mut self) {
        let links: Vec<String> = self
            .filtered
            .iter()
            .filter_map(|&i| self.articles.get(i))
            .filter(|a| a.unread)
            .map(|a| a.link.clone())
            .collect();
        if links.is_empty() {
            return;
        }
        let _ = self.db.mark_read_many(&links);
        for &i in &self.filtered {
            if let Some(a) = self.articles.get_mut(i) {
                a.unread = false;
            }
        }
        self.refresh_counts();
        self.notice = Some(format!("Marked {} read", links.len()));
    }

    pub fn open_in_browser(&mut self) {
        let Some(link) = self.current_link() else {
            return;
        };
        self.notice = Some(match crate::platform::open_url(&link) {
            Ok(()) => "Opened in browser".into(),
            Err(e) => format!("Could not open browser: {e}"),
        });
    }

    pub fn copy_link(&mut self) {
        let Some(link) = self.current_link() else {
            return;
        };
        self.notice = Some(match crate::platform::copy_to_clipboard(&link) {
            Ok(()) => "Link copied".into(),
            Err(e) => format!("Could not copy: {e}"),
        });
    }

    /// Called when a reload finishes: turns collected errors into a summary.
    pub fn finish_reload(&mut self) -> Result<()> {
        self.loading = false;
        self.feed_errors.sort();
        self.status = match self.feed_errors.len() {
            0 => String::new(),
            1 => "1 feed failed, press ! for details".into(),
            n => format!("{n} feeds failed, press ! for details"),
        };
        self.prune_old_articles();
        self.reload_view()
    }

    /// Applies a background reader fetch's result, but only if it's still
    /// for the article currently open — a fetch started for an article the
    /// user has since navigated away from can complete late, and without
    /// this check it would silently overwrite whatever's now on screen with
    /// the wrong article's text.
    pub fn apply_reader_event(&mut self, event: ReaderEvent) {
        let current = self.reader_article.as_ref().map(|a| a.link.as_str());
        match event {
            ReaderEvent::Loaded(link, content) if current == Some(link.as_str()) => {
                self.reader = Some(content);
                self.reader_loading = false;
            }
            ReaderEvent::Failed(link, msg) if current == Some(link.as_str()) => {
                self.reader_loading = false;
                self.reader_error = Some(msg);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::Feed;

    fn test_app() -> App {
        let path = crate::store::temp_db_path("dnews_app_test");
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        App::new(db, Vec::<Feed>::new())
    }

    fn article_stub(link: &str) -> ArticleRow {
        ArticleRow {
            title: "title".into(),
            link: link.into(),
            published: 0,
            unread: false,
            saved: false,
            content_text: None,
            ..Default::default()
        }
    }

    #[test]
    fn stale_reader_event_is_ignored_after_navigating_to_another_article() {
        let mut app = test_app();

        // Simulate opening article A: a fetch for it is now "in flight".
        app.reader_article = Some(article_stub("https://example.com/a"));
        app.reader_loading = true;

        // The user navigates away and opens article B before A's fetch
        // returns.
        app.reader_article = Some(article_stub("https://example.com/b"));
        app.reader = None;
        app.reader_loading = true;

        // B's fetch finishes first.
        app.apply_reader_event(ReaderEvent::Loaded(
            "https://example.com/b".into(),
            ReaderContent::from_plain("B's content"),
        ));
        assert_eq!(app.reader.as_ref().unwrap().plain_text(), "B's content");
        assert!(!app.reader_loading);

        // A's fetch finally completes — it must NOT overwrite B's content,
        // since the user is no longer looking at article A.
        app.apply_reader_event(ReaderEvent::Loaded(
            "https://example.com/a".into(),
            ReaderContent::from_plain("A's content"),
        ));
        assert_eq!(
            app.reader.as_ref().unwrap().plain_text(),
            "B's content",
            "a late response for a since-abandoned article overwrote the current one"
        );
    }

    #[test]
    fn matching_reader_event_is_applied() {
        let mut app = test_app();
        app.reader_article = Some(article_stub("https://example.com/a"));
        app.reader_loading = true;

        app.apply_reader_event(ReaderEvent::Loaded(
            "https://example.com/a".into(),
            ReaderContent::from_plain("hello"),
        ));

        assert_eq!(app.reader.as_ref().unwrap().plain_text(), "hello");
        assert!(!app.reader_loading);
    }

    #[test]
    fn reader_header_metadata_survives_a_list_reload_while_reading() {
        // Regression test for the actual bug report: the header must keep
        // showing the *opened* article's title/date, not whatever article
        // now sits at `selected`'s index after a background reload reorders
        // `articles` (e.g. a newer article lands at index 0).
        let mut app = test_app();
        app.articles = vec![article_stub("https://example.com/old")];
        app.filtered = vec![0];
        app.selected = 0;
        app.reader_article = Some(article_stub("https://example.com/old"));

        // A background reload completes while the article above is open:
        // a newer article is inserted, shifting the old one out of index 0.
        app.articles = vec![
            article_stub("https://example.com/new"),
            article_stub("https://example.com/old"),
        ];
        app.apply_search();

        assert_eq!(
            app.reader_article.as_ref().unwrap().link,
            "https://example.com/old",
            "reload reindexing must not change which article the reader thinks is open"
        );
    }
    #[test]
    fn finished_reload_summarises_failed_feeds() {
        let mut app = test_app();
        app.finish_reload().unwrap();
        assert!(app.status.is_empty());

        app.feed_errors.push(("B feed".into(), "timeout".into()));
        app.feed_errors.push(("A feed".into(), "HTTP 504".into()));
        app.finish_reload().unwrap();
        assert_eq!(app.status, "2 feeds failed, press ! for details");
        assert_eq!(app.feed_errors[0].0, "A feed", "errors are listed sorted");
    }
}
