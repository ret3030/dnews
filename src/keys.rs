use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc;

use crate::app::{App, Overlay, ReaderEvent, Screen};
use crate::feed::fetch::FetchEvent;

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
        match key.code {
            KeyCode::Esc => {
                app.search.clear();
                app.search_active = false;
                app.apply_search();
            }
            KeyCode::Enter => app.search_active = false,
            KeyCode::Backspace => {
                app.search.pop();
                app.apply_search();
            }
            KeyCode::Char(c) => {
                app.search.push(c);
                app.apply_search();
            }
            _ => {}
        }
        return Ok(());
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('r') {
        app.start_reload(progress_tx);
        return Ok(());
    }

    if wide {
        match key.code {
            KeyCode::Char('J' | 'K') => {
                app.move_selection(if key.code == KeyCode::Char('J') {
                    1
                } else {
                    -1
                });
                app.schedule_preview();
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
        KeyCode::Char('/') => app.search_active = true,
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
        _ => scroll_list(app, key),
    }
    Ok(())
}

fn scroll_list(app: &mut App, key: KeyEvent) {
    match key.code {
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
mod tests {
    use super::*;
    use crate::app::Screen;
    use crate::feed::Feed;
    use crate::store::{NewArticle, Store};

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
}
