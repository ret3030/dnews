pub mod article;
pub mod list;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders};

use crate::app::{App, Screen};

/// Below this terminal width, the list screen is single-pane: `Enter` opens
/// the full-screen reader (see `Screen::Reader`). At or above it, the list
/// splits into the list and an article preview panel side by side instead
/// — see `App::panel_focused` and `keys::handle_list_wide` for how keyboard
/// focus moves between the two.
pub const SPLIT_MIN_WIDTH: u16 = 150;

/// Draws the app chrome — an orange top/bottom rule with "dnews" in the top
/// one — and hands the inner content area to the active screen.
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();
    f.render_widget(Block::default().style(Style::default().bg(theme::BG)), area);

    let title = Line::styled(
        " dnews ",
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    );
    let card = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(theme::ACCENT))
        .style(Style::default().bg(theme::BG))
        .title(title);
    let inner = card.inner(area);
    f.render_widget(card, area);

    match app.screen {
        Screen::Reader => article::draw(f, app, inner, false),
        Screen::List if inner.width >= SPLIT_MIN_WIDTH => {
            let cols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(55),
                    Constraint::Length(2),
                    Constraint::Percentage(45),
                ])
                .split(inner);
            list::draw(f, app, cols[0], true);
            article::draw(f, app, cols[2], true);
        }
        Screen::List => list::draw(f, app, inner, false),
    }
}

/// Horizontally centers a `max_width`-wide (or narrower, if the area is
/// smaller) column within `area`, keeping the full height.
pub fn centered(area: Rect, max_width: u16) -> Rect {
    let width = area.width.min(max_width);
    let margin = (area.width - width) / 2;
    Rect {
        x: area.x + margin,
        y: area.y,
        width,
        height: area.height,
    }
}

pub fn domain(url: &str) -> String {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}

pub fn time_ago(published: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = (now - published).max(0);
    if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}

/// Truncates `s` to fit within `max_width` display columns, appending an
/// ellipsis if it had to cut — keeps long error/status text from ever
/// overflowing its bar.
pub fn truncate(s: &str, max_width: usize) -> String {
    if s.chars().count() <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let mut out: String = s.chars().take(max_width - 1).collect();
    out.push('…');
    out
}

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// A braille-dot spinner frame, advanced once per tick while something is
/// loading in the background (feed reload, article fetch).
pub fn spinner(frame: usize) -> char {
    SPINNER[frame % SPINNER.len()]
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::App;
    use crate::feed::Feed;
    use crate::store::{NewArticle, Store};

    fn dump(width: u16, height: u16, app: &App) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| super::draw(f, app)).unwrap();
        let buf = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn list_screen_renders_expected_layout() {
        let path = std::env::temp_dir().join(format!("dnews_ui_test_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        db.upsert_batch(&[
            NewArticle {
                category: "Tech & Dev".into(),
                title: "Rust is great".into(),
                link: "https://example.com/a".into(),
                published: chrono::Utc::now().timestamp(),
            },
            NewArticle {
                category: "Zprávy".into(),
                title: "Nějaká zpráva".into(),
                link: "https://irozhlas.cz/b".into(),
                published: chrono::Utc::now().timestamp(),
            },
        ])
        .unwrap();

        let feeds = vec![
            Feed {
                url: "https://example.com/feed".into(),
                category: "Tech & Dev".into(),
            },
            Feed {
                url: "https://irozhlas.cz/feed".into(),
                category: "Zprávy".into(),
            },
        ];
        let mut app = App::new(db, feeds);
        app.reload_view().unwrap();

        let out = dump(120, 30, &app);
        println!("{out}");

        assert!(out.contains("dnews"), "app title missing from top border");
        assert!(out.contains("All"), "All tab missing");
        assert!(out.contains("Tech & Dev"), "category tab missing");
        assert!(out.contains("Rust is great"), "article title missing");
        assert!(out.contains("unread"), "unread counter missing");
        assert!(out.contains('─'), "top/bottom border rule missing");
        // Top/bottom only — no vertical `│` down the sides.
        assert!(!out.contains('│'), "a side border crept back in");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reader_screen_centers_and_wraps() {
        let path =
            std::env::temp_dir().join(format!("dnews_ui_test_reader_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        db.upsert_batch(&[NewArticle {
            category: String::new(),
            title: "A Wide Article Title".into(),
            link: "https://example.com/a".into(),
            published: chrono::Utc::now().timestamp(),
        }])
        .unwrap();

        let mut app = App::new(db, vec![]);
        app.reload_view().unwrap();
        app.screen = crate::app::Screen::Reader;
        app.reader = Some(crate::reader::ReaderContent {
            text: "Hello world, this is the article body.".into(),
        });

        // A very wide terminal: the reader column must not span edge to edge.
        let out = dump(200, 30, &app);
        println!("{out}");
        let first_content_line = out.lines().nth(1).unwrap();
        let left_pad = first_content_line.chars().take_while(|c| *c == ' ').count();
        assert!(
            left_pad > 20,
            "reader view should be centered with wide side margins on a wide terminal, got {left_pad} left padding"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn wide_terminal_splits_list_and_preview_side_by_side() {
        let path =
            std::env::temp_dir().join(format!("dnews_ui_test_split_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        db.upsert_batch(&[NewArticle {
            category: String::new(),
            title: "Split View Story".into(),
            link: "https://example.com/a".into(),
            published: chrono::Utc::now().timestamp(),
        }])
        .unwrap();

        let mut app = App::new(db, vec![]);
        app.reload_view().unwrap();
        // Simulate the debounced preview already having loaded, as it would
        // shortly after the selection settles.
        app.reader_article = app.articles.first().cloned();
        app.reader = Some(crate::reader::ReaderContent {
            text: "Previewed article body text.".into(),
        });

        let wide = dump(super::SPLIT_MIN_WIDTH + 20, 30, &app);
        assert!(
            wide.contains("Split View Story"),
            "list pane missing in split layout"
        );
        assert!(
            wide.contains("Previewed article body text"),
            "preview pane missing in split layout"
        );

        let narrow = dump(super::SPLIT_MIN_WIDTH - 20, 30, &app);
        assert!(
            narrow.contains("Split View Story"),
            "list should still render below the split threshold"
        );
        assert!(
            !narrow.contains("Previewed article body text"),
            "preview pane should not render below the split threshold"
        );

        let _ = std::fs::remove_file(&path);
    }
}
