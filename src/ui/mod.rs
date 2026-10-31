pub mod article;
pub mod list;
pub mod overlay;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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

    overlay::draw(f, app, area);
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
    if s.width() <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in s.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > max_width - 1 {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Word-wraps `s` to `width` display columns, keeping at most `max_lines`
/// lines; if text is left over, the last line ends in an ellipsis. Words
/// longer than a whole line are hard-split.
pub fn wrap_words(s: &str, width: usize, max_lines: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;
    let mut overflow = false;

    'words: for word in s.split_whitespace() {
        let mut word = word;
        loop {
            let ww = word.width();
            let need = if cur.is_empty() { ww } else { cur_w + 1 + ww };
            if need <= width {
                if !cur.is_empty() {
                    cur.push(' ');
                    cur_w += 1;
                }
                cur.push_str(word);
                cur_w += ww;
                continue 'words;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
                if lines.len() == max_lines {
                    overflow = true;
                    break 'words;
                }
                continue;
            }
            // A single word wider than the line: split it.
            let mut head = String::new();
            let mut hw = 0;
            let mut split_at = word.len();
            for (i, c) in word.char_indices() {
                let w = c.width().unwrap_or(0);
                if hw + w > width {
                    split_at = i;
                    break;
                }
                head.push(c);
                hw += w;
            }
            lines.push(head);
            word = &word[split_at..];
            if lines.len() == max_lines {
                overflow = !word.is_empty();
                break 'words;
            }
            if word.is_empty() {
                continue 'words;
            }
        }
    }
    if !cur.is_empty() {
        if lines.len() < max_lines {
            lines.push(cur);
        } else {
            overflow = true;
        }
    }
    if overflow && let Some(last) = lines.last_mut() {
        // `truncate` keeps the ellipsis when the line is already full.
        *last = truncate(&format!("{last}…"), width);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Like `truncate`, but operates on a styled `Line` (e.g. the tab bar) instead
/// of a plain string — cuts partway through a `Span` if needed and appends a
/// dim ellipsis span, so a row of tabs too wide for the terminal shrinks
/// cleanly instead of being clipped mid-word at the `Rect` boundary.
pub fn truncate_line(line: Line<'static>, max_width: usize) -> Line<'static> {
    let total_width: usize = line.spans.iter().map(|s| s.content.chars().count()).sum();
    if total_width <= max_width {
        return line;
    }
    if max_width == 0 {
        return Line::from(Span::raw(""));
    }
    let budget = max_width - 1; // reserve one column for the ellipsis
    let mut spans = Vec::new();
    let mut used = 0usize;
    for span in line.spans {
        if used >= budget {
            break;
        }
        let remaining = budget - used;
        let span_len = span.content.chars().count();
        if span_len <= remaining {
            used += span_len;
            spans.push(span);
        } else {
            let cut: String = span.content.chars().take(remaining).collect();
            spans.push(Span::styled(cut, span.style));
            break;
        }
    }
    spans.push(Span::styled("…", Style::default().fg(theme::DIM)));
    Line::from(spans)
}

/// Like `truncate`, but keeps the *tail* of `s` (ellipsis at the start) —
/// used for the live search box so the cursor at the end of what you just
/// typed always stays visible, instead of scrolling off the right edge on a
/// narrow terminal.
pub fn truncate_start(s: &str, max_width: usize) -> String {
    let len = s.chars().count();
    if len <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return "…".to_string();
    }
    let skip = len - (max_width - 1);
    let mut out = String::from("…");
    out.extend(s.chars().skip(skip));
    out
}

/// Greedily packs `phrases` (each an atomic hint like `"j/k scroll"`, never
/// split mid-phrase) onto as few lines as fit within `max_width` columns,
/// joining same-line phrases with `sep`. Used to grow a footer hint bar to
/// two (or more) lines on a narrow terminal instead of silently clipping
/// whatever hint ran past the edge.
pub fn wrap_hints(phrases: &[&str], sep: &str, max_width: usize) -> Vec<String> {
    if phrases.is_empty() {
        return vec![String::new()];
    }
    let sep_len = sep.chars().count();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for phrase in phrases {
        let phrase_len = phrase.chars().count();
        let extra = if current.is_empty() {
            phrase_len
        } else {
            sep_len + phrase_len
        };
        if !current.is_empty() && current_len + extra > max_width {
            lines.push(std::mem::take(&mut current));
            current_len = 0;
        }
        if !current.is_empty() {
            current.push_str(sep);
            current_len += sep_len;
        }
        current.push_str(phrase);
        current_len += phrase_len;
    }
    lines.push(current);
    lines
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

    use ratatui::style::Style;
    use ratatui::text::{Line, Span};

    use super::{truncate_line, truncate_start, wrap_hints};
    use crate::app::App;
    use crate::feed::Feed;
    use crate::store::{NewArticle, Store};

    #[test]
    fn truncate_line_shrinks_a_multi_span_line_to_fit_with_an_ellipsis() {
        let line = Line::from(vec![
            Span::styled("All", Style::default()),
            Span::styled(" · ", Style::default()),
            Span::styled("Zprávy a Trhy", Style::default()),
            Span::styled(" · ", Style::default()),
            Span::styled("Saved", Style::default()),
        ]);
        let short = truncate_line(line.clone(), 100);
        let short_text: String = short.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(short_text, "All · Zprávy a Trhy · Saved");

        let cut = truncate_line(line, 10);
        let cut_text: String = cut.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(cut_text.chars().count(), 10);
        assert!(
            cut_text.ends_with('…'),
            "expected an ellipsis, got {cut_text:?}"
        );
        assert!(
            !cut_text.contains("Saved"),
            "should have cut before reaching the last tab"
        );
    }

    #[test]
    fn wrap_hints_packs_phrases_onto_as_few_lines_as_fit() {
        let phrases = ["one", "two", "three", "four"];
        // Plenty of room: everything fits on one line.
        assert_eq!(
            wrap_hints(&phrases, "  ", 100),
            vec!["one  two  three  four".to_string()]
        );
        // Only enough room for two phrases per line.
        let wrapped = wrap_hints(&phrases, "  ", 10);
        assert_eq!(wrapped, vec!["one  two", "three", "four"]);
        // Never splits a single phrase across lines, even if it alone
        // exceeds max_width.
        let wrapped = wrap_hints(&["a very long phrase indeed"], "  ", 5);
        assert_eq!(wrapped, vec!["a very long phrase indeed"]);
    }

    #[test]
    fn truncate_start_keeps_the_tail_so_a_trailing_cursor_stays_visible() {
        assert_eq!(truncate_start("hello", 10), "hello");
        assert_eq!(truncate_start("hello world", 6), "…world");
        assert_eq!(truncate_start("hello", 0), "…");
    }

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
                ..Default::default()
            },
            NewArticle {
                category: "Zprávy".into(),
                title: "Nějaká zpráva".into(),
                link: "https://irozhlas.cz/b".into(),
                published: chrono::Utc::now().timestamp(),
                ..Default::default()
            },
        ])
        .unwrap();

        let feeds = vec![
            Feed {
                url: "https://example.com/feed".into(),
                category: "Tech & Dev".into(),
                name: "Example".into(),
            },
            Feed {
                url: "https://irozhlas.cz/feed".into(),
                category: "Zprávy".into(),
                name: "iRozhlas".into(),
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
            ..Default::default()
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
            ..Default::default()
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
    #[test]
    fn reader_max_scroll_lands_on_the_last_line_of_the_article() {
        let path =
            std::env::temp_dir().join(format!("dnews_ui_test_scroll_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        db.upsert_batch(&[NewArticle {
            category: String::new(),
            title: "Long Article".into(),
            link: "https://example.com/long".into(),
            published: chrono::Utc::now().timestamp(),
            ..Default::default()
        }])
        .unwrap();

        let mut app = App::new(db, vec![]);
        app.reload_view().unwrap();
        app.screen = crate::app::Screen::Reader;
        app.reader_article = app.articles.first().cloned();
        let text: Vec<String> = (1..=100).map(|i| format!("line {i:03}")).collect();
        app.reader = Some(crate::reader::ReaderContent {
            text: text.join("\n"),
        });

        dump(100, 30, &app);
        let max = app.reader_max_scroll.get();
        assert!(
            max > 0 && max < 100,
            "max scroll should be the text height minus the visible body, got {max}"
        );

        // Jumping to the end must show the last line, and nothing past it.
        app.reader_scroll = max;
        let out = dump(100, 30, &app);
        assert!(
            out.contains("line 100"),
            "last line not visible at max scroll"
        );
        assert!(
            !out.contains("line 001"),
            "first line still visible, so max scroll is too small"
        );

        // One line less must hide the last line, so `max` is not padded with
        // blank space past the end.
        app.reader_scroll = max - 1;
        let out = dump(100, 30, &app);
        assert!(
            !out.contains("line 100"),
            "max scroll leaves blank space below the last line"
        );

        let _ = std::fs::remove_file(&path);
    }
    #[test]
    fn wrap_words_breaks_at_spaces_and_ellipsizes_the_overflow() {
        use super::wrap_words;
        assert_eq!(wrap_words("short title", 40, 2), vec!["short title"]);
        assert_eq!(
            wrap_words("one two three four five", 9, 2),
            vec!["one two", "three…"]
        );
        // Every line fits, measured in display columns.
        for line in wrap_words("Vláda schválila státní rozpočet na příští rok", 12, 3) {
            assert!(
                unicode_width::UnicodeWidthStr::width(line.as_str()) <= 12,
                "{line:?}"
            );
        }
        // A word longer than the line is split rather than overflowing.
        let long = wrap_words("supercalifragilistic", 8, 2);
        assert_eq!(long[0], "supercal");
        assert!(long[1].ends_with('…'));
    }

    #[test]
    fn list_titles_keep_clear_of_the_right_edge() {
        let path =
            std::env::temp_dir().join(format!("dnews_ui_test_gutter_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        db.upsert_batch(&[NewArticle {
            category: String::new(),
            title: "Several EU countries on track to fail nature targets set for the end of the decade, report finds".into(),
            link: "https://example.com/a".into(),
            published: chrono::Utc::now().timestamp(),
            ..Default::default()
        }])
        .unwrap();
        let mut app = App::new(db, vec![]);
        app.reload_view().unwrap();

        let width = 60;
        let out = dump(width, 20, &app);
        // Rows 3.. are the list (row 0 border, 1 tabs, 2 unread counter).
        for line in out.lines().skip(3).take(4) {
            let tail: String = line.chars().skip(width as usize - 3).collect();
            assert!(tail.trim().is_empty(), "text in the right gutter: {line:?}");
        }
        assert!(out.contains("decade") || out.contains('…'));

        let _ = std::fs::remove_file(&path);
    }
    fn app_with(articles: &[NewArticle], name: &str) -> (App, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("dnews_ui_test_{name}_{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let db = Store::open(&path).unwrap();
        db.init_schema().unwrap();
        db.upsert_batch(articles).unwrap();
        let feeds = vec![Feed {
            url: "https://example.com/feed".into(),
            category: "World".into(),
            name: "Example".into(),
        }];
        let mut app = App::new(db, feeds);
        app.reload_view().unwrap();
        (app, path)
    }

    #[test]
    fn list_groups_articles_under_day_headings_and_shows_the_source_name() {
        let now = chrono::Utc::now().timestamp();
        let (app, path) = app_with(
            &[
                NewArticle {
                    category: "World".into(),
                    source: "BBC World".into(),
                    title: "Fresh story".into(),
                    link: "https://feeds.bbci.co.uk/a".into(),
                    published: now - 60,
                    ..Default::default()
                },
                NewArticle {
                    category: "World".into(),
                    title: "Older story".into(),
                    link: "https://example.com/b".into(),
                    published: now - 3 * 86_400,
                    ..Default::default()
                },
            ],
            "days",
        );
        let out = dump(100, 30, &app);
        assert!(out.contains("Today"), "{out}");
        assert!(
            out.contains("BBC World"),
            "source name should replace the domain"
        );
        assert!(!out.contains("feeds.bbci.co.uk"));
        assert!(
            out.contains("example.com"),
            "falls back to the domain without a source"
        );
        assert!(out.contains("World 2"), "tab shows its unread count");
        let today = out.find("Today").unwrap();
        let older = out.find("Older story").unwrap();
        assert!(today < out.find("Fresh story").unwrap() && today < older);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn day_label_names_recent_days() {
        use chrono::TimeZone;
        let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
        let at = |d: u32, h: u32| {
            chrono::Local
                .with_ymd_and_hms(2026, 10, d, h, 0, 0)
                .unwrap()
                .timestamp()
        };
        assert_eq!(super::list::day_label(at(31, 8), today), "Today");
        assert_eq!(super::list::day_label(at(30, 23), today), "Yesterday");
        assert_eq!(super::list::day_label(at(28, 12), today), "Wed 28 Oct");
        assert_eq!(super::list::day_label(0, today), "Undated");
    }

    #[test]
    fn compact_mode_uses_one_line_per_article() {
        let now = chrono::Utc::now().timestamp();
        let (mut app, path) = app_with(
            &[NewArticle {
                category: "World".into(),
                source: "DW World".into(),
                title: "A compact headline".into(),
                link: "https://example.com/a".into(),
                published: now,
                ..Default::default()
            }],
            "compact",
        );
        app.compact = true;
        let out = dump(100, 20, &app);
        let line = out
            .lines()
            .find(|l| l.contains("A compact headline"))
            .unwrap();
        assert!(
            line.contains("DW World"),
            "source sits on the title line: {line:?}"
        );
        assert!(line.trim_end().ends_with("ago"));
        assert!(
            line.find("DW World").unwrap() > line.find("A compact headline").unwrap() + 20,
            "meta sits in its own right-hand column"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reader_falls_back_to_the_feed_summary_when_fetching_fails() {
        let (mut app, path) = app_with(
            &[NewArticle {
                category: "World".into(),
                title: "Paywalled".into(),
                summary: "The feed's own short summary.".into(),
                link: "https://example.com/a".into(),
                published: chrono::Utc::now().timestamp(),
                ..Default::default()
            }],
            "fallback",
        );
        app.screen = crate::app::Screen::Reader;
        app.reader_article = app.articles.first().cloned();
        app.reader_error = Some("HTTP 403".into());
        let out = dump(100, 20, &app);
        assert!(out.contains("The feed's own short summary."), "{out}");
        assert!(out.contains("open it in the browser"));
        let _ = std::fs::remove_file(&path);
    }
}
