use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use crate::app::{App, ReaderEvent, Screen};
use crate::feed::fetch::FetchEvent;

pub fn handle(
    app: &mut App,
    key: KeyEvent,
    progress_tx: &mpsc::UnboundedSender<FetchEvent>,
    reader_tx: &mpsc::UnboundedSender<ReaderEvent>,
    wide: bool,
) -> Result<()> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        app.should_quit = true;
        return Ok(());
    }

    match app.screen {
        Screen::List => handle_list(app, key, progress_tx, reader_tx, wide)?,
        Screen::Reader => handle_reader(app, key),
    }

    Ok(())
}

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
            KeyCode::Enter => {
                app.search_active = false;
            }
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

    if wide {
        return handle_list_wide(app, key, progress_tx, reader_tx);
    }

    match key.code {
        KeyCode::Char('q') => app.should_quit = true,
        KeyCode::Char('/') => app.search_active = true,
        KeyCode::Esc => {
            if !app.search.is_empty() {
                app.search.clear();
                app.apply_search();
            }
        }
        KeyCode::Enter => app.open_selected(reader_tx),
        KeyCode::Tab => app.cycle_filter(1)?,
        KeyCode::BackTab => app.cycle_filter(-1)?,
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.start_reload(progress_tx);
        }
        _ => scroll_list(app, key),
    }

    Ok(())
}

/// The wide split layout (see `ui::SPLIT_MIN_WIDTH`) has one keyboard
/// focus that toggles between the list and the preview panel, tracked by
/// `App::panel_focused`:
///
/// - Not focused (the default, and after `q`/`Esc` un-focuses): plain vim
///   keys browse the list exactly like the narrow single-pane screen does
///   (`scroll_list`) — nothing gets activated/marked read just by moving.
/// - Focused (`Enter` on a list item, which also activates it into the
///   panel): plain vim keys scroll the panel instead (`scroll_article`,
///   the same logic the narrow full-screen reader uses), and `q`/`Esc`
///   hands focus back to the list rather than clearing the panel.
///
/// Independent of that toggle, Shift+J/Shift+K always move the list
/// selection and schedule a debounced preview load into the panel — so you
/// can keep the list "in control" while still previewing as you browse.
/// Category tabs and search are list-only actions, so they (along with
/// `q`/`Esc`'s own meaning) are gated on focus too — same as the narrow
/// full-screen reader, `/` and `Tab` do nothing while the panel is focused.
fn handle_list_wide(
    app: &mut App,
    key: KeyEvent,
    progress_tx: &mpsc::UnboundedSender<FetchEvent>,
    reader_tx: &mpsc::UnboundedSender<ReaderEvent>,
) -> Result<()> {
    match key.code {
        KeyCode::Char('J') => {
            app.move_selection(1);
            app.schedule_preview();
            return Ok(());
        }
        KeyCode::Char('K') => {
            app.move_selection(-1);
            app.schedule_preview();
            return Ok(());
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.start_reload(progress_tx);
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
            KeyCode::Char('q') | KeyCode::Esc => app.unfocus_panel(),
            KeyCode::Char('s') => app.toggle_saved_selected(),
            _ => scroll_article(app, key),
        }
    } else {
        match key.code {
            KeyCode::Char('q') => app.should_quit = true,
            KeyCode::Char('/') => app.search_active = true,
            KeyCode::Esc => {
                if !app.search.is_empty() {
                    app.search.clear();
                    app.apply_search();
                }
            }
            KeyCode::Tab => app.cycle_filter(1)?,
            KeyCode::BackTab => app.cycle_filter(-1)?,
            _ => scroll_list(app, key),
        }
    }

    Ok(())
}

/// Classic list navigation: narrow single-pane screen's plain vim keys, and
/// the wide split layout's list side while its panel isn't focused (see
/// `handle_list_wide`).
fn scroll_list(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('s') => app.toggle_saved_selected(),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Char('g') => app.move_to_top(),
        KeyCode::Char('G') => app.move_to_bottom(),
        KeyCode::PageDown | KeyCode::Char('d') => app.move_selection(15),
        KeyCode::PageUp | KeyCode::Char('u') => app.move_selection(-15),
        _ => {}
    }
}

fn handle_reader(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => app.close_reader(),
        KeyCode::Char('s') => app.toggle_saved_selected(),
        _ => scroll_article(app, key),
    }
}

/// Article scrolling: the narrow full-screen reader, and the wide split
/// layout's panel while it's focused (see `handle_list_wide`).
fn scroll_article(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Down | KeyCode::Char('j') => {
            app.reader_scroll = app.reader_scroll.saturating_add(1)
        }
        KeyCode::Up | KeyCode::Char('k') => app.reader_scroll = app.reader_scroll.saturating_sub(1),
        KeyCode::Char('g') => app.reader_scroll = 0,
        KeyCode::Char('G') => app.reader_scroll = u16::MAX / 2,
        KeyCode::PageDown | KeyCode::Char('d') => {
            app.reader_scroll = app.reader_scroll.saturating_add(15)
        }
        KeyCode::PageUp | KeyCode::Char('u') => {
            app.reader_scroll = app.reader_scroll.saturating_sub(15)
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Screen;
    use crate::feed::Feed;
    use crate::store::{NewArticle, Store};

    fn test_app() -> App {
        let path = std::env::temp_dir().join(format!(
            "dnews_keys_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
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
            },
            NewArticle {
                category: String::new(),
                title: "Older Story".into(),
                link: "https://example.com/older".into(),
                published: 100,
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
            "returning to plain list browsing should unload the panel — it only \
             lights back up via Enter or the Shift+J/K live preview"
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
}
