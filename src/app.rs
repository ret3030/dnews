use std::cell::Cell;
use std::collections::{HashMap, HashSet};

use anyhow::Result;
use tokio::sync::mpsc;

use crate::cluster::{Story, cluster};
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
    /// Tagged with the article link, so a late result for an article the
    /// user already left is ignored.
    Loaded(String, ReaderContent),
    Failed(String, String),
}

/// One selectable line of the Briefing. Rows map 1:1 onto `App::filtered`,
/// so selection, the reader and article actions work as in any list.
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum BriefingRow {
    /// A story, standing in for its headline article.
    Story(usize),
    /// One article of an expanded story.
    Member(usize),
    /// "+ N more stories" for a category; Enter opens its tab.
    More(String, usize),
}

/// The Briefing shows at most this many multi-outlet stories per category,
/// and at least this many stories of any size.
const BRIEFING_MAX_PER_CATEGORY: usize = 5;
const BRIEFING_MIN_PER_CATEGORY: usize = 2;
/// When the Briefing was last finished with `M`, and where that finished
/// Briefing started (shown greyed out until something new comes in).
const BRIEFING_DONE_KEY: &str = "briefing_done";
const BRIEFING_PREVIOUS_KEY: &str = "briefing_previous";

/// What the last `M` changed, so `U` can put it back.
pub struct FinishedBriefing {
    links: Vec<String>,
    done: Option<String>,
    previous: Option<String>,
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
    /// Copy of the article in the reader, taken when it was opened. Not looked
    /// up through `selected`, because a background reload can reorder
    /// `articles` while it is being read.
    pub reader_article: Option<ArticleRow>,
    pub reader_scroll: u16,
    /// Last useful scroll offset, set while drawing (hence the `Cell`) so the
    /// scroll keys stop at the end of the text. `u16::MAX` until drawn.
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

    /// Wide split layout only: plain keys scroll the article panel instead of
    /// moving through the list. `screen` stays `List` there.
    pub panel_focused: bool,
    /// When the main loop should load the selection into the panel.
    pub preview_due_at: Option<std::time::Instant>,

    /// Briefing tab: stories over `articles`, the rows on screen, and which
    /// stories are expanded.
    pub stories: Vec<Story>,
    pub briefing_rows: Vec<BriefingRow>,
    pub expanded: HashSet<usize>,
    /// Stories with nothing newer than this were already read through (`M`).
    pub briefing_since: i64,
    /// Start of the last finished Briefing, if it is still within reach.
    pub briefing_previous: Option<i64>,
    pub undo_finish: Option<FinishedBriefing>,
    /// Articles published since `briefing_since`.
    pub briefing_new: usize,
    /// The outlets covering an article's story, for stories with two or
    /// more. Kept for every tab, for the `[+N]` badges.
    pub coverage: HashMap<String, Vec<String>>,

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
            filter: ListFilter::Briefing,
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
            stories: Vec::new(),
            briefing_rows: Vec::new(),
            expanded: HashSet::new(),
            briefing_since: 0,
            briefing_previous: None,
            undo_finish: None,
            briefing_new: 0,
            coverage: HashMap::new(),
            should_quit: false,
        }
    }

    pub fn start_reload(&mut self, tx: &mpsc::UnboundedSender<FetchEvent>) {
        self.loading = true;
        self.status.clear();
        self.feed_errors.clear();
        fetch::spawn_reload(self.feeds.clone(), self.db.path().to_path_buf(), tx.clone());
    }

    /// Deletes unsaved articles older than [`PRUNE_AFTER_SECS`]. Runs after
    /// every reload, because slow feeds keep serving old items that the
    /// reload would otherwise bring straight back.
    pub fn prune_old_articles(&self) {
        let cutoff = chrono::Utc::now().timestamp() - PRUNE_AFTER_SECS;
        let _ = self.db.prune_old(cutoff);
    }

    pub fn reload_view(&mut self) -> Result<()> {
        self.articles = self.db.list(&self.filter)?;
        if self.filter == ListFilter::Briefing {
            (self.stories, self.coverage) = recent_stories(&self.articles, &self.feeds);
            self.build_briefing();
        } else {
            self.apply_search();
        }
        self.refresh_counts();
        Ok(())
    }

    /// Lays out the Briefing: what is new since it was last finished, then
    /// that finished Briefing (read, so greyed out) below it.
    pub fn build_briefing(&mut self) {
        let setting = |key| {
            self.db
                .setting(key)
                .ok()
                .flatten()
                .and_then(|s| s.parse::<i64>().ok())
        };
        let oldest = chrono::Utc::now().timestamp() - crate::store::BRIEFING_WINDOW_SECS;
        self.briefing_since = setting(BRIEFING_DONE_KEY).unwrap_or(0).max(oldest);
        self.briefing_previous = setting(BRIEFING_PREVIOUS_KEY)
            .map(|p| p.max(oldest))
            .filter(|&p| p < self.briefing_since);
        self.briefing_new = self
            .articles
            .iter()
            .filter(|a| a.published > self.briefing_since)
            .count();

        self.briefing_rows.clear();
        self.filtered.clear();
        self.push_briefing_section(self.briefing_since, i64::MAX, true);
        if let Some(previous) = self.briefing_previous {
            self.push_briefing_section(previous, self.briefing_since, false);
        }
        self.selected = self.selected.min(self.filtered.len().saturating_sub(1));
    }

    /// Rows for the stories whose newest article falls in `(from, to]`: per
    /// category, widely covered ones first, and optionally a "more" row.
    fn push_briefing_section(&mut self, from: i64, to: i64, with_more: bool) {
        let stories = &self.stories;
        for category in &self.categories {
            // `stories` is best first; members are newest first.
            let fresh: Vec<usize> = (0..stories.len())
                .filter(|&s| {
                    let newest = stories[s].newest;
                    stories[s].category == *category && newest > from && newest <= to
                })
                .collect();
            let shown = pick_stories(stories, &fresh);
            for &s in &shown {
                self.briefing_rows.push(BriefingRow::Story(s));
                self.filtered.push(stories[s].head);
                if self.expanded.contains(&s) {
                    for &m in &stories[s].members {
                        self.briefing_rows.push(BriefingRow::Member(s));
                        self.filtered.push(m);
                    }
                }
            }
            let hidden = fresh.iter().find(|s| !shown.contains(s));
            if let Some(&hidden) = hidden.filter(|_| with_more) {
                let rest = fresh.len() - shown.len();
                self.briefing_rows
                    .push(BriefingRow::More(category.clone(), rest));
                self.filtered.push(stories[hidden].head);
            }
        }
    }

    /// Expands or collapses a story, keeping the cursor on it.
    pub fn toggle_story(&mut self, story: usize) {
        if !self.expanded.remove(&story) {
            self.expanded.insert(story);
        }
        self.build_briefing();
        self.selected = self
            .briefing_rows
            .iter()
            .position(|r| *r == BriefingRow::Story(story))
            .unwrap_or(0);
    }

    /// `M` in the Briefing: everything in it is read, and the next Briefing
    /// starts from now. This one stays below it, greyed out.
    pub fn finish_briefing(&mut self) -> Result<()> {
        let links: Vec<String> = self
            .articles
            .iter()
            .filter(|a| a.published > self.briefing_since && a.unread)
            .map(|a| a.link.clone())
            .collect();
        self.db.set_unread_many(&links, false)?;
        self.undo_finish = Some(FinishedBriefing {
            links,
            done: self.db.setting(BRIEFING_DONE_KEY)?,
            previous: self.db.setting(BRIEFING_PREVIOUS_KEY)?,
        });
        let now = chrono::Utc::now().timestamp();
        self.db
            .set_setting(BRIEFING_PREVIOUS_KEY, &self.briefing_since.to_string())?;
        self.db.set_setting(BRIEFING_DONE_KEY, &now.to_string())?;
        self.expanded.clear();
        self.selected = 0;
        self.reload_view()?;
        self.notice = Some("Briefing done. U undoes it.".into());
        Ok(())
    }

    /// `U`: takes back the last `M`, unread flags included.
    pub fn undo_finish_briefing(&mut self) -> Result<()> {
        let Some(finished) = self.undo_finish.take() else {
            self.notice = Some("Nothing to undo".into());
            return Ok(());
        };
        self.db.set_unread_many(&finished.links, true)?;
        for (key, value) in [
            (BRIEFING_DONE_KEY, finished.done),
            (BRIEFING_PREVIOUS_KEY, finished.previous),
        ] {
            match value {
                Some(value) => self.db.set_setting(key, &value)?,
                None => self.db.remove_setting(key)?,
            }
        }
        self.reload_view()?;
        self.notice = Some("Briefing restored".into());
        Ok(())
    }

    /// `b` in the reader: shows the open article's story in the Briefing,
    /// expanded, with the cursor on the article.
    pub fn show_story(&mut self) -> Result<()> {
        let Some(link) = self.current_link() else {
            return Ok(());
        };
        self.close_article();
        self.screen = Screen::List;
        self.panel_focused = false;
        self.set_filter(ListFilter::Briefing)?;
        let story = (0..self.stories.len()).find(|&s| {
            self.stories[s]
                .members
                .iter()
                .any(|&m| self.articles[m].link == link)
        });
        let Some(story) = story else {
            self.notice = Some("This article is not in the Briefing".into());
            return Ok(());
        };
        self.expanded.insert(story);
        self.build_briefing();
        let row = (0..self.filtered.len()).find(|&i| {
            self.briefing_rows[i] == BriefingRow::Member(story)
                && self.articles[self.filtered[i]].link == link
        });
        match row {
            Some(row) => self.selected = row,
            None => self.notice = Some("Its story has nothing new since the last Briefing".into()),
        }
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
        let mut stops: Vec<ListFilter> = vec![ListFilter::Briefing, ListFilter::All];
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

    /// Previews the selection after a short pause, so holding Shift+J
    /// doesn't fetch every article passed on the way.
    pub fn schedule_preview(&mut self) {
        self.preview_due_at =
            Some(std::time::Instant::now() + std::time::Duration::from_millis(150));
    }

    /// Loads the selection into the panel unless it is already there.
    pub fn activate_selected(&mut self, reader_tx: &mpsc::UnboundedSender<ReaderEvent>) {
        let Some(row) = self.selected_article() else {
            return;
        };
        if self.reader_article.as_ref().map(|a| a.link.as_str()) == Some(row.link.as_str()) {
            return;
        }
        self.load_selected(reader_tx);
    }

    /// Marks the selected article read and shows it in the reader, from the
    /// cache or fetched in the background.
    pub fn load_selected(&mut self, reader_tx: &mpsc::UnboundedSender<ReaderEvent>) {
        let Some(row) = self.selected_article().cloned() else {
            return;
        };
        self.save_scroll();
        if row.unread {
            let _ = self.db.set_unread(&row.link, false);
            self.update_article(&row.link, |a| a.unread = false);
            self.refresh_counts();
        }
        let row = ArticleRow {
            unread: false,
            ..row
        };

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

    /// Empties the reader (full screen or panel), remembering the scroll
    /// position first.
    pub fn close_article(&mut self) {
        self.save_scroll();
        self.reader = None;
        self.reader_article = None;
        self.reader_loading = false;
        self.reader_error = None;
        self.reader_scroll = 0;
    }

    pub fn selected_article(&self) -> Option<&ArticleRow> {
        // A "more" row in the Briefing stands for a tab, not an article.
        if self.filter == ListFilter::Briefing
            && matches!(
                self.briefing_rows.get(self.selected),
                Some(BriefingRow::More(..))
            )
        {
            return None;
        }
        self.filtered
            .get(self.selected)
            .and_then(|&i| self.articles.get(i))
    }

    /// The article the next action applies to: the one open in the reader
    /// (full screen or focused panel), otherwise the list selection.
    pub fn current_article(&self) -> Option<&ArticleRow> {
        if self.screen == Screen::Reader || self.panel_focused {
            return self.reader_article.as_ref();
        }
        self.selected_article()
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
        let _ = self.db.set_unread_many(&links, false);
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
        if self.filter != ListFilter::Briefing {
            // The badges in other tabs need fresh stories too.
            let recent = self.db.list(&ListFilter::Briefing)?;
            self.coverage = recent_stories(&recent, &self.feeds).1;
        }
        self.reload_view()
    }

    /// Applies a finished fetch, unless the user has moved on to another
    /// article in the meantime.
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

/// Which of a category's `fresh` stories (best first) the Briefing shows:
/// those covered by two or more outlets, up to a limit, topped up with the
/// best single-outlet ones so a quiet category still shows something.
fn pick_stories(stories: &[Story], fresh: &[usize]) -> Vec<usize> {
    let mut shown: Vec<usize> = fresh
        .iter()
        .copied()
        .filter(|&s| stories[s].sources > 1)
        .take(BRIEFING_MAX_PER_CATEGORY)
        .collect();
    for &s in fresh {
        if shown.len() >= BRIEFING_MIN_PER_CATEGORY {
            break;
        }
        if !shown.contains(&s) {
            shown.push(s);
        }
    }
    shown.sort_by_key(|s| fresh.iter().position(|f| f == s));
    shown
}

/// Groups recent articles into stories and records, for each article in a
/// story covered by two or more outlets, which outlets those are.
fn recent_stories(
    articles: &[ArticleRow],
    feeds: &[Feed],
) -> (Vec<Story>, HashMap<String, Vec<String>>) {
    let priority = feeds.iter().map(|f| (f.name.clone(), f.priority)).collect();
    let stories = cluster(articles, chrono::Utc::now().timestamp(), &priority);
    let mut coverage = HashMap::new();
    for story in stories.iter().filter(|s| s.sources > 1) {
        let mut outlets: Vec<String> = Vec::new();
        for &m in &story.members {
            let source = articles[m].source_name();
            if !outlets.contains(&source) {
                outlets.push(source);
            }
        }
        for &m in &story.members {
            coverage.insert(articles[m].link.clone(), outlets.clone());
        }
    }
    (stories, coverage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::feed::Feed;

    fn test_app() -> App {
        let path = crate::store::temp_db_path("dnews_app_test");
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        let mut app = App::new(db, Vec::<Feed>::new());
        app.filter = ListFilter::All;
        app
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

        // A's late result must not replace B, which is now on screen.
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

    #[test]
    fn a_category_shows_wide_stories_first_and_never_less_than_two() {
        let story = |sources| Story {
            members: vec![0],
            head: 0,
            sources,
            newest: 0,
            category: String::new(),
            score: 0.0,
        };
        // Best first: a single, then three wide ones, then singles.
        let stories: Vec<Story> = [1, 3, 2, 2, 1, 1].into_iter().map(story).collect();
        let fresh: Vec<usize> = (0..stories.len()).collect();
        assert_eq!(
            pick_stories(&stories, &fresh),
            [1, 2, 3],
            "wide stories, best first"
        );

        // A quiet category: only singles, still two of them.
        let quiet: Vec<Story> = [1, 1, 1].into_iter().map(story).collect();
        assert_eq!(pick_stories(&quiet, &[0, 1, 2]), [0, 1]);

        // Seven wide stories: capped at five.
        let busy: Vec<Story> = std::iter::repeat_with(|| story(2)).take(7).collect();
        assert_eq!(pick_stories(&busy, &(0..7).collect::<Vec<_>>()).len(), 5);
    }
}
