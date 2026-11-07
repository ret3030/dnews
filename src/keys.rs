use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::app::{App, Overlay, ReaderEvent, Screen};
use crate::feed::fetch::FetchEvent;
use crate::store::ListFilter;

pub fn handle(
    app: &mut App,
    key: KeyEvent,
    progress_tx: &mpsc::UnboundedSender<FetchEvent>,
    reader_tx: &mpsc::UnboundedSender<ReaderEvent>,
    wide: bool,
) -> Result<()> {
    // Windows also reports key releases; acting on them would fire every
    // binding twice. Repeats are kept so holding a key scrolls.
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return Ok(());
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    app.notice = None;

    // An open overlay swallows the key that closes it.
    if app.overlay != Overlay::None {
        app.overlay = Overlay::None;
        return Ok(());
    }

    if !app.search_active && handle_global(app, key) {
        return Ok(());
    }

    let reading = app.screen == Screen::Reader || (wide && app.panel_focused);
    if reading && handle_reading(app, key) {
        return Ok(());
    }

    match app.screen {
        Screen::List => handle_list(app, key, progress_tx, reader_tx, wide)?,
        Screen::Reader => handle_reader(app, key, reader_tx),
    }

    Ok(())
}

/// Keys that act the same on every screen (outside the search box).
/// Returns true when the key was consumed.
fn handle_global(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('?') => app.overlay = Overlay::Help,
        KeyCode::Char('!') if !app.feed_errors.is_empty() => app.overlay = Overlay::FeedErrors,
        KeyCode::Char('o') => app.open_in_browser(),
        KeyCode::Char('y') => app.copy_link(),
        KeyCode::Char('m') => app.toggle_read_current(),
        _ => return false,
    }
    true
}

/// Article-only keys shared by the full-screen reader and the focused panel:
/// picking a link by number and the text width toggle.
fn handle_reading(app: &mut App, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char(c @ '0'..='9') if app.link_input.len() < 4 => {
            if !(c == '0' && app.link_input.is_empty()) {
                app.link_input.push(c);
            }
        }
        KeyCode::Enter if !app.link_input.is_empty() => {
            let n = app.link_input.parse().unwrap_or(0);
            app.link_input.clear();
            app.open_link(n);
        }
        KeyCode::Backspace if !app.link_input.is_empty() => {
            app.link_input.pop();
        }
        KeyCode::Esc if !app.link_input.is_empty() => app.link_input.clear(),
        KeyCode::Char('w') => app.toggle_reader_wide(),
        KeyCode::Char('b') => {
            if let Err(e) = app.show_story() {
                app.notice = Some(format!("Could not open the Briefing: {e}"));
            }
        }
        _ => return false,
    }
    true
}

/// The list screen. In the wide split layout, Enter loads the article into
/// the side panel and focuses it; while focused, plain keys scroll the
/// panel and q/Esc hands focus back. Shift+J/K always move the selection
/// and preview it after a short pause.
fn handle_list(
    app: &mut App,
    key: KeyEvent,
    progress_tx: &mpsc::UnboundedSender<FetchEvent>,
    reader_tx: &mpsc::UnboundedSender<ReaderEvent>,
    wide: bool,
) -> Result<()> {
    if app.search_active {
        handle_search(app, key);
        return Ok(());
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
        app.start_reload(progress_tx);
        return Ok(());
    }

    // In the Briefing, Enter on a story lists its articles and q/Esc goes
    // back; those articles read like any list.
    if app.filter == ListFilter::Briefing && !app.panel_focused {
        match (app.story, key.code) {
            (None, KeyCode::Enter) => {
                if let Some(&story) = app.briefing_rows.get(app.selected) {
                    app.open_story(story);
                }
                return Ok(());
            }
            (Some(_), KeyCode::Char('q') | KeyCode::Esc) => {
                app.close_story();
                return Ok(());
            }
            _ => {}
        }
    }

    if wide {
        match key.code {
            KeyCode::Char('J' | 'K') => {
                app.move_selection(if key.code == KeyCode::Char('J') {
                    1
                } else {
                    -1
                });
                // In the Briefing itself the story card follows the cursor;
                // previewing would fetch the headline and mark it read.
                if !(app.filter == ListFilter::Briefing && app.story.is_none()) {
                    app.schedule_preview();
                }
                return Ok(());
            }
            KeyCode::Enter => {
                app.activate_selected(reader_tx);
                app.panel_focused = true;
                return Ok(());
            }
            _ => {}
        }
        if app.panel_focused {
            match key.code {
                // The panel stays empty until Enter or Shift+J/K loads one again.
                KeyCode::Char('q') | KeyCode::Esc => {
                    app.close_article();
                    app.panel_focused = false;
                }
                KeyCode::Char('s') => app.toggle_saved(),
                _ => scroll_article(app, key),
            }
            return Ok(());
        }
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('/') if app.filter != ListFilter::Briefing => app.search_active = true,
        KeyCode::Esc if !app.search.is_empty() => {
            app.search.clear();
            app.apply_search();
        }
        KeyCode::Enter => {
            app.load_selected(reader_tx);
            app.screen = Screen::Reader;
        }
        KeyCode::Tab => app.cycle_filter(1)?,
        KeyCode::BackTab => app.cycle_filter(-1)?,
        KeyCode::Char('M') if app.filter == ListFilter::Briefing && app.story.is_none() => {
            app.finish_briefing()?
        }
        KeyCode::Char('s') => app.toggle_saved(),
        KeyCode::Char('M') => app.mark_all_read(),
        KeyCode::Char('c') => app.toggle_compact(),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Char('g') => app.selected = 0,
        KeyCode::Char('G') => app.selected = app.filtered.len().saturating_sub(1),
        KeyCode::PageDown | KeyCode::Char('d') => app.move_selection(15),
        KeyCode::PageUp | KeyCode::Char('u') => app.move_selection(-15),
        _ => {}
    }
    Ok(())
}

/// Typing in the search box filters the list as you go; Enter keeps the
/// filter, Esc drops it.
fn handle_search(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.search.clear();
            app.search_active = false;
        }
        KeyCode::Enter => {
            app.search_active = false;
            return;
        }
        KeyCode::Backspace => {
            app.search.pop();
        }
        KeyCode::Char(c) => app.search.push(c),
        _ => return,
    }
    app.apply_search();
}

fn handle_reader(app: &mut App, key: KeyEvent, reader_tx: &mpsc::UnboundedSender<ReaderEvent>) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => {
            app.close_article();
            app.screen = Screen::List;
        }
        KeyCode::Char('s') => app.toggle_saved(),
        KeyCode::Char('J' | 'K') => {
            app.move_selection(if key.code == KeyCode::Char('J') {
                1
            } else {
                -1
            });
            app.load_selected(reader_tx);
        }
        _ => scroll_article(app, key),
    }
}

/// Scrolls the article, clamped to `App::reader_max_scroll` so `G`, `j` and
/// `d` stop at the end. The current offset is clamped first too: after a
/// resize it can sit past the new end.
fn scroll_article(app: &mut App, key: KeyEvent) {
    let max = app.reader_max_scroll.get();
    let cur = app.reader_scroll.min(max);
    let next = match key.code {
        KeyCode::Down | KeyCode::Char('j') => cur.saturating_add(1),
        KeyCode::Up | KeyCode::Char('k') => cur.saturating_sub(1),
        KeyCode::Char('g') => 0,
        KeyCode::Char('G') => max,
        KeyCode::Char(' ') if key.modifiers.contains(KeyModifiers::SHIFT) => cur.saturating_sub(15),
        KeyCode::PageDown | KeyCode::Char('d' | ' ') => cur.saturating_add(15),
        KeyCode::PageUp | KeyCode::Char('u') => cur.saturating_sub(15),
        _ => return,
    };
    app.reader_scroll = next.min(max);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::app::Screen;
    use crate::feed::Feed;
    use crate::store::{ListFilter, NewArticle, Store};

    fn test_app() -> App {
        let path = crate::store::temp_db_path("dnews_keys_test");
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        // Two articles, newest first (ORDER BY published DESC), so list
        // navigation tests have somewhere to move to.
        db.upsert_batch(&[
            NewArticle {
                category: String::new(),
                title: "Newer Story".into(),
                link: "https://example.com/newer".into(),
                published: 200,
                ..Default::default()
            },
            NewArticle {
                category: String::new(),
                title: "Older Story".into(),
                link: "https://example.com/older".into(),
                published: 100,
                ..Default::default()
            },
        ])
        .unwrap();
        let mut app = App::new(db, Vec::<Feed>::new());
        app.filter = crate::store::ListFilter::All;
        app.reload_view().unwrap();
        app
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[tokio::test]
    async fn key_release_events_are_ignored() {
        // Windows sends a Release for every Press; acting on both would move the
        // selection two rows per keystroke.
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        let mut down = key(KeyCode::Char('j'));
        handle(&mut app, down, &progress_tx, &reader_tx, false).unwrap();
        assert_eq!(app.selected, 1);

        down.kind = KeyEventKind::Release;
        handle(&mut app, down, &progress_tx, &reader_tx, false).unwrap();
        assert_eq!(app.selected, 1, "the release half must not move again");
    }

    #[tokio::test]
    async fn enter_opens_full_screen_reader_when_narrow() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();

        assert_eq!(app.screen, Screen::Reader);
    }

    #[tokio::test]
    async fn shift_j_and_k_page_through_articles_within_the_full_screen_reader() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.reader_article.as_ref().unwrap().title, "Newer Story");

        handle(
            &mut app,
            key(KeyCode::Char('J')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(
            app.screen,
            Screen::Reader,
            "Shift+J should stay in the reader, not bounce back to the list"
        );
        assert_eq!(app.selected, 1);
        assert_eq!(app.reader_article.as_ref().unwrap().title, "Older Story");

        handle(
            &mut app,
            key(KeyCode::Char('K')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.selected, 0);
        assert_eq!(app.reader_article.as_ref().unwrap().title, "Newer Story");
    }

    #[test]
    fn plain_vim_keys_browse_the_list_before_an_article_is_activated_when_wide() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Char('j')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();

        assert_eq!(
            app.selected, 1,
            "plain j should move the list selection until an article is activated"
        );
        assert_eq!(
            app.reader_scroll, 0,
            "nothing should be loaded/scrolled yet"
        );
        assert!(
            app.reader_article.is_none(),
            "plain list browsing must not auto-activate an article"
        );
    }

    #[tokio::test]
    async fn enter_activates_selection_and_focuses_the_panel_when_wide() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();

        assert_eq!(
            app.screen,
            Screen::List,
            "wide mode must not switch to the full-screen reader"
        );
        assert!(app.panel_focused, "Enter should focus the panel");
        assert_eq!(
            app.reader_article.as_ref().map(|a| a.link.as_str()),
            Some("https://example.com/newer"),
            "Enter should load the selected article into the panel"
        );
    }

    #[tokio::test]
    async fn plain_vim_keys_scroll_the_panel_once_focused_when_wide() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        let selected_before = app.selected;

        handle(
            &mut app,
            key(KeyCode::Char('j')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();

        assert_eq!(
            app.selected, selected_before,
            "plain j must not move the list selection once the panel is focused"
        );
        assert_eq!(
            app.reader_scroll, 1,
            "plain j should scroll the panel article"
        );
    }

    #[tokio::test]
    async fn q_unfocuses_and_unloads_the_panel_back_to_plain_list_browsing() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert!(
            app.reader_article.is_some(),
            "sanity check: Enter should have loaded the panel"
        );

        handle(
            &mut app,
            key(KeyCode::Char('q')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();

        assert!(!app.panel_focused, "q should unfocus the panel");
        assert!(
            !app.should_quit,
            "q must unfocus, not quit, while the panel is focused"
        );
        assert!(
            app.reader_article.is_none(),
            "leaving the panel empties it until Enter or Shift+J/K loads one"
        );

        // With focus back on the list, plain j now browses again.
        handle(
            &mut app,
            key(KeyCode::Char('j')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn shift_j_and_k_move_list_selection_and_schedule_a_preview_when_wide() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Char('J')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert_eq!(app.selected, 1, "Shift+J should move selection down");
        assert!(
            app.preview_due_at.is_some(),
            "Shift+J should schedule a debounced preview load"
        );
        assert!(
            app.reader_article.is_none(),
            "the preview load is debounced, not synchronous"
        );

        handle(
            &mut app,
            key(KeyCode::Char('K')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert_eq!(app.selected, 0, "Shift+K should move selection back up");
    }

    #[tokio::test]
    async fn tab_and_search_are_no_ops_while_the_panel_is_focused_when_wide() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert!(
            app.panel_focused,
            "sanity check: Enter should focus the panel"
        );
        let filter_before = app.filter.clone();

        handle(&mut app, key(KeyCode::Tab), &progress_tx, &reader_tx, true).unwrap();
        assert_eq!(
            app.filter, filter_before,
            "Tab must not cycle category tabs while reading in the panel"
        );

        handle(
            &mut app,
            key(KeyCode::Char('/')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert!(
            !app.search_active,
            "/ must not open search while reading in the panel"
        );
    }
    #[tokio::test]
    async fn article_scrolling_stops_at_the_end_of_the_text() {
        let mut app = test_app();
        let (progress_tx, _progress_rx) = mpsc::unbounded_channel();
        let (reader_tx, _reader_rx) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        // What the renderer would store for an article 20 lines past the screen.
        app.reader_max_scroll.set(20);

        handle(
            &mut app,
            key(KeyCode::Char('G')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(
            app.reader_scroll, 20,
            "G should land exactly on the last screen"
        );

        handle(
            &mut app,
            key(KeyCode::Char('j')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        handle(
            &mut app,
            key(KeyCode::Char('d')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.reader_scroll, 20, "j/d must not scroll past the end");

        handle(
            &mut app,
            key(KeyCode::Char('k')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(
            app.reader_scroll, 19,
            "k should move up right away from the end"
        );

        // After a resize the stored offset can be past the new end: the first
        // k must still move up one line from the visible position.
        app.reader_scroll = 50;
        handle(
            &mut app,
            key(KeyCode::Char('k')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.reader_scroll, 19);
    }
    #[test]
    fn m_toggles_read_and_capital_m_marks_everything_listed_read() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();
        let unread_before = app.unread_count;
        assert!(app.articles[0].unread);

        handle(
            &mut app,
            key(KeyCode::Char('m')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert!(
            !app.articles[0].unread,
            "m should mark the selected article read"
        );
        assert_eq!(app.unread_count, unread_before - 1);

        handle(
            &mut app,
            key(KeyCode::Char('m')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert!(
            app.articles[0].unread,
            "a second m should mark it unread again"
        );

        handle(
            &mut app,
            key(KeyCode::Char('M')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert!(app.articles.iter().all(|a| !a.unread));
        assert_eq!(app.unread_count, 0);

        // The DB agrees, not just the in-memory rows.
        app.reload_view().unwrap();
        assert!(app.articles.iter().all(|a| !a.unread));
    }

    #[test]
    fn help_overlay_swallows_the_key_that_closes_it() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Char('?')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.overlay, Overlay::Help);

        handle(
            &mut app,
            key(KeyCode::Char('q')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.overlay, Overlay::None);
        assert!(!app.should_quit, "closing the overlay must not also quit");
    }

    #[test]
    fn failed_feeds_overlay_only_opens_when_there_are_errors() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Char('!')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.overlay, Overlay::None);

        app.feed_errors
            .push(("FRED Blog".into(), "HTTP status server error (504)".into()));
        handle(
            &mut app,
            key(KeyCode::Char('!')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.overlay, Overlay::FeedErrors);
    }

    #[test]
    fn keys_typed_into_search_are_not_shortcuts() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();

        handle(
            &mut app,
            key(KeyCode::Char('/')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        for c in ['m', 'o', '?'] {
            handle(
                &mut app,
                key(KeyCode::Char(c)),
                &progress_tx,
                &reader_tx,
                false,
            )
            .unwrap();
        }
        assert_eq!(app.search, "mo?");
        assert_eq!(app.overlay, Overlay::None);
        assert!(
            app.articles[0].unread,
            "m inside search must not mark anything"
        );
    }
    #[tokio::test]
    async fn digits_then_enter_pick_a_link_and_esc_cancels() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();
        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        app.reader = Some(crate::reader::ReaderContent::from_plain("body"));

        for c in ['1', '2'] {
            handle(
                &mut app,
                key(KeyCode::Char(c)),
                &progress_tx,
                &reader_tx,
                false,
            )
            .unwrap();
        }
        assert_eq!(app.link_input, "12");
        handle(&mut app, key(KeyCode::Esc), &progress_tx, &reader_tx, false).unwrap();
        assert!(app.link_input.is_empty());
        assert_eq!(
            app.screen,
            Screen::Reader,
            "Esc only cancels the link prompt"
        );

        handle(
            &mut app,
            key(KeyCode::Char('3')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert!(app.link_input.is_empty());
        // The fixture has no links, so nothing is opened.
        assert_eq!(app.notice.as_deref(), Some("No link 3"));
    }

    #[tokio::test]
    async fn space_pages_and_w_toggles_wide_text_persistently() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();
        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        app.reader_max_scroll.set(100);

        handle(
            &mut app,
            key(KeyCode::Char(' ')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.reader_scroll, 15);
        let shift_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::SHIFT);
        handle(&mut app, shift_space, &progress_tx, &reader_tx, false).unwrap();
        assert_eq!(app.reader_scroll, 0);

        assert!(!app.reader_wide);
        handle(
            &mut app,
            key(KeyCode::Char('w')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert!(app.reader_wide);
        assert_eq!(app.db.setting("reader_wide").unwrap().as_deref(), Some("1"));
    }

    #[tokio::test]
    async fn reopening_an_article_continues_where_it_was_left() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();
        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        app.reader_max_scroll.set(100);
        app.reader_scroll = 42;
        handle(
            &mut app,
            key(KeyCode::Char('q')),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.screen, Screen::List);

        // Survives a reload from the DB too.
        app.reload_view().unwrap();
        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            false,
        )
        .unwrap();
        assert_eq!(app.reader_scroll, 42);
    }

    #[tokio::test]
    async fn s_in_the_focused_panel_saves_the_open_article_not_the_selection() {
        let mut app = test_app();
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();
        handle(
            &mut app,
            key(KeyCode::Enter),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        // Shift+J moves the selection; the preview has not loaded yet.
        handle(
            &mut app,
            key(KeyCode::Char('J')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert_eq!(app.selected, 1);

        handle(
            &mut app,
            key(KeyCode::Char('s')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert!(app.reader_article.as_ref().unwrap().saved);
        assert!(app.articles[0].saved, "the open article's row is saved too");
        assert!(
            !app.articles[1].saved,
            "the merely selected row is untouched"
        );
    }

    /// The real news snapshot, moved to end now, in a fresh DB with feeds
    /// for its outlets. Starts in the Briefing.
    pub(crate) fn briefing_app() -> App {
        let (articles, then) = crate::cluster::tests::snapshot();
        let shift = chrono::Utc::now().timestamp() - then;
        let db = Store::open(&crate::store::temp_db_path("dnews_briefing_test")).unwrap();
        db.init_schema().unwrap();
        let rows: Vec<NewArticle> = articles
            .iter()
            .map(|a| NewArticle {
                category: a.category.clone(),
                source: a.source.clone(),
                summary: a.summary.clone(),
                title: a.title.clone(),
                link: a.link.clone(),
                published: a.published + shift,
            })
            .collect();
        db.upsert_batch(&rows).unwrap();
        let mut feeds: Vec<Feed> = Vec::new();
        for a in &articles {
            if !feeds.iter().any(|f| f.name == a.source) {
                feeds.push(Feed {
                    url: format!("https://example.com/{}", feeds.len()),
                    category: a.category.clone(),
                    name: a.source.clone(),
                    priority: 1.0,
                });
            }
        }
        let mut app = App::new(db, feeds);
        app.reload_view().unwrap();
        app
    }

    fn press(app: &mut App, code: KeyCode) {
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();
        handle(app, key(code), &progress_tx, &reader_tx, false).unwrap();
    }

    #[test]
    fn the_briefing_leads_with_top_stories_then_each_category() {
        let app = briefing_app();
        assert_eq!(
            app.filter,
            ListFilter::Briefing,
            "the app opens on the Briefing"
        );
        let rows = &app.briefing_rows;
        let top = &rows[..app.briefing_top];
        assert_eq!(top.len(), 5);
        assert!(top.iter().all(|&s| app.stories[s].sources > 1));
        assert!(
            top.windows(2)
                .all(|w| app.stories[w[0]].score >= app.stories[w[1]].score),
            "best first"
        );
        let mut seen = rows.clone();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), rows.len(), "no story shows twice");

        // Below the top, categories follow the feed list, a few stories each.
        let order: Vec<&str> = rows[app.briefing_top..]
            .iter()
            .map(|&s| app.stories[s].category.as_str())
            .collect();
        let mut expected: Vec<&str> = app.categories.iter().map(String::as_str).collect();
        expected.retain(|c| order.contains(c));
        let mut sections = order.clone();
        sections.dedup();
        assert_eq!(sections, expected, "one block per category, in order");
        for category in sections {
            let n = order.iter().filter(|c| **c == category).count();
            assert!((1..=5).contains(&n), "{category}: {n} stories");
        }
        assert_eq!(app.filtered.len(), rows.len(), "a list row per story");
    }

    #[tokio::test]
    async fn enter_opens_a_story_and_q_goes_back_to_it() {
        let mut app = briefing_app();
        let poland = (0..app.stories.len())
            .find(|&s| {
                let head = &app.articles[app.stories[s].head];
                head.title.contains("Poland’s constitutional court")
            })
            .unwrap();
        app.selected = app.briefing_rows.iter().position(|&s| s == poland).unwrap();
        let row = app.selected;

        press(&mut app, KeyCode::Enter);
        assert_eq!(app.story, Some(poland));
        let members = &app.stories[poland].members;
        assert_eq!(
            app.filtered.len(),
            members.len(),
            "every article of the story"
        );
        assert!(
            app.filtered
                .windows(2)
                .all(|w| app.articles[w[0]].published >= app.articles[w[1]].published),
            "newest first"
        );

        // Enter reads an article; q returns to the story, q again to the Briefing.
        app.selected = 1;
        let wanted = app.selected_article().unwrap().link.clone();
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.screen, Screen::Reader);
        assert_eq!(app.reader_article.as_ref().unwrap().link, wanted);
        press(&mut app, KeyCode::Char('q'));
        assert_eq!((app.screen, app.story), (Screen::List, Some(poland)));

        // M inside a story marks just its articles; the Briefing stays open.
        let since = app.briefing_since;
        press(&mut app, KeyCode::Char('M'));
        assert!(app.filtered.iter().all(|&i| !app.articles[i].unread));
        assert!(app.unread_count > 0);
        assert_eq!(app.db.setting("briefing_done").unwrap(), None);
        assert_eq!(app.briefing_since, since);

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.story, None);
        assert_eq!(app.selected, row, "the cursor is back on the story");
    }

    #[test]
    fn shift_j_in_the_wide_briefing_moves_without_fetching_or_marking_read() {
        let mut app = briefing_app();
        let unread = app.unread_count;
        let (progress_tx, _p) = mpsc::unbounded_channel();
        let (reader_tx, _r) = mpsc::unbounded_channel();
        handle(
            &mut app,
            key(KeyCode::Char('J')),
            &progress_tx,
            &reader_tx,
            true,
        )
        .unwrap();
        assert_eq!(app.selected, 1);
        assert!(
            app.preview_due_at.is_none(),
            "the story card needs no fetch"
        );
        assert_eq!(app.unread_count, unread);
    }

    #[test]
    fn capital_m_finishes_the_briefing() {
        let mut app = briefing_app();
        assert!(!app.briefing_rows.is_empty());

        press(&mut app, KeyCode::Char('M'));
        assert_eq!(app.unread_count, 0, "everything in it is read");
        assert_eq!(app.briefing_new, 0);
        assert!(app.briefing_rows.is_empty(), "nothing new to show");

        // A second M with nothing new changes nothing (it used to finish
        // the seconds between the two presses).
        let done = app.db.setting("briefing_done").unwrap();
        press(&mut app, KeyCode::Char('M'));
        assert_eq!(app.db.setting("briefing_done").unwrap(), done);
        assert_eq!(app.notice.as_deref(), Some("Nothing new to finish"));

        // Something new arrives and shows up.
        let now = chrono::Utc::now().timestamp();
        app.db
            .upsert_batch(&[NewArticle {
                category: "Svět".into(),
                source: "BBC World".into(),
                title: "Volcano erupts in Iceland".into(),
                link: "https://example.com/volcano".into(),
                published: now + 60,
                ..Default::default()
            }])
            .unwrap();
        app.reload_view().unwrap();
        assert_eq!(app.briefing_new, 1);
        assert_eq!(app.filtered.len(), 1);
        assert!(app.articles[app.filtered[0]].title.starts_with("Volcano"));
    }

    #[tokio::test]
    async fn b_in_the_reader_opens_the_articles_story() {
        let mut app = briefing_app();
        app.set_filter(ListFilter::All).unwrap();
        app.selected = app
            .filtered
            .iter()
            .position(|&i| {
                app.articles[i]
                    .title
                    .starts_with("What we know about Binance")
            })
            .unwrap();
        assert!(app.coverage[&app.articles[app.filtered[app.selected]].link].len() >= 5);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('b'));

        assert_eq!(app.screen, Screen::List);
        assert_eq!(app.filter, ListFilter::Briefing);
        assert!(app.story.is_some());
        assert!(
            app.selected_article()
                .unwrap()
                .title
                .starts_with("What we know about Binance")
        );
    }
}
