pub mod article;
pub mod briefing;
pub mod document;
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

/// From this terminal width the list and the article sit side by side;
/// below it the article opens full screen.
pub const SPLIT_MIN_WIDTH: u16 = 150;

/// Draws the frame (top and bottom rule, "dnews" title) and the active screen.
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

/// "BBC World, Guardian World +2": the first two names and a count.
pub fn outlets_label(outlets: &[&str]) -> String {
    let mut label = outlets
        .iter()
        .take(2)
        .copied()
        .collect::<Vec<_>>()
        .join(", ");
    if outlets.len() > 2 {
        label.push_str(&format!(" +{}", outlets.len() - 2));
    }
    label
}

/// Cuts `s` to `max_width` display columns, ending in an ellipsis if cut.
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

/// Word-wraps plain text to `width` columns, keeping at most `max_lines`
/// lines; the last one ends in an ellipsis if text was left over.
pub fn wrap_words(s: &str, width: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = wrap(
        vec![(s.to_string(), Style::default())],
        width,
        vec![],
        vec![],
    )
    .iter()
    .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
    .collect();
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            // `truncate` keeps the ellipsis when the line is already full.
            *last = truncate(&format!("{last}…"), width);
        }
    }
    lines
}

enum Token {
    /// Text with no break opportunity inside, possibly spanning styles
    /// ("report" + "[1]" + ",").
    Word(Vec<(String, Style)>),
    Space(Style),
    Break,
}

fn tokenize(runs: Vec<(String, Style)>) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word: Vec<(String, Style)> = Vec::new();
    let finish = |word: &mut Vec<(String, Style)>, tokens: &mut Vec<Token>| {
        if !word.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    for (text, style) in runs {
        for c in text.chars() {
            if c == '\n' {
                finish(&mut word, &mut tokens);
                tokens.push(Token::Break);
            } else if c.is_whitespace() {
                finish(&mut word, &mut tokens);
                tokens.push(Token::Space(style));
            } else {
                match word.last_mut() {
                    Some((s, st)) if *st == style => s.push(c),
                    _ => word.push((c.to_string(), style)),
                }
            }
        }
    }
    finish(&mut word, &mut tokens);
    tokens
}

/// Word-wraps styled text to `width` columns. `first` prefixes the first
/// line, `rest` every following one (for hanging indents).
pub fn wrap(
    runs: Vec<(String, Style)>,
    width: usize,
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
) -> Vec<Line<'static>> {
    let prefix_w = |p: &[Span]| p.iter().map(|s| s.content.width()).sum::<usize>();
    // Always leave room for some text, however deep the indent.
    let width = width.max(prefix_w(&first).max(prefix_w(&rest)) + 8);

    let mut lines = Vec::new();
    let mut cur: Vec<Span<'static>> = first;
    let mut cur_w = prefix_w(&cur);
    let mut has_text = false;
    let mut space: Option<Style> = None;

    let new_line = |lines: &mut Vec<Line<'static>>, cur: &mut Vec<Span<'static>>| {
        lines.push(Line::from(std::mem::replace(cur, rest.clone())));
        prefix_w(cur)
    };

    for token in tokenize(runs) {
        match token {
            Token::Space(style) => {
                if has_text {
                    space = Some(style);
                }
            }
            Token::Break => {
                cur_w = new_line(&mut lines, &mut cur);
                has_text = false;
                space = None;
            }
            Token::Word(segments) => {
                let word_w: usize = segments.iter().map(|(s, _)| s.width()).sum();
                let space_w = usize::from(space.is_some());
                if has_text && cur_w + space_w + word_w > width {
                    cur_w = new_line(&mut lines, &mut cur);
                    space = None;
                } else if let Some(style) = space.take() {
                    cur.push(Span::styled(" ", style));
                    cur_w += 1;
                }
                if cur_w + word_w <= width {
                    for (s, style) in segments {
                        cur.push(Span::styled(s, style));
                    }
                    cur_w += word_w;
                } else {
                    // Longer than a whole line: split it character by character.
                    for (s, style) in segments {
                        let mut piece = String::new();
                        for c in s.chars() {
                            let cw = c.width().unwrap_or(0);
                            if cur_w + cw > width {
                                cur.push(Span::styled(std::mem::take(&mut piece), style));
                                cur_w = new_line(&mut lines, &mut cur);
                            }
                            piece.push(c);
                            cur_w += cw;
                        }
                        cur.push(Span::styled(piece, style));
                    }
                }
                has_text = true;
            }
        }
    }
    if has_text || lines.is_empty() {
        lines.push(Line::from(cur));
    }
    lines
}

/// `truncate` for a styled line, such as the tab bar.
pub fn truncate_line(line: Line<'static>, max_width: usize) -> Line<'static> {
    let total_width: usize = line.spans.iter().map(|s| s.content.width()).sum();
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
        let span_w = span.content.width();
        if span_w <= remaining {
            used += span_w;
            spans.push(span);
        } else {
            let mut cut = String::new();
            let mut cut_w = 0;
            for c in span.content.chars() {
                let w = c.width().unwrap_or(0);
                if cut_w + w > remaining {
                    break;
                }
                cut.push(c);
                cut_w += w;
            }
            spans.push(Span::styled(cut, span.style));
            break;
        }
    }
    spans.push(Span::styled("…", Style::default().fg(theme::DIM)));
    Line::from(spans)
}

/// `truncate` that keeps the end, so the search box cursor stays visible.
pub fn truncate_start(s: &str, max_width: usize) -> String {
    if s.width() <= max_width {
        return s.to_string();
    }
    let mut tail: Vec<char> = Vec::new();
    let mut used = 0;
    for c in s.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > max_width.saturating_sub(1) {
            break;
        }
        tail.push(c);
        used += w;
    }
    std::iter::once('…').chain(tail.into_iter().rev()).collect()
}

/// Packs key hints onto as few lines as fit, never splitting a hint.
pub fn wrap_hints(phrases: &[&str], sep: &str, max_width: usize) -> Vec<String> {
    if phrases.is_empty() {
        return vec![String::new()];
    }
    let sep_len = sep.width();
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for phrase in phrases {
        let phrase_len = phrase.width();
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

/// Spinner frame for background loading.
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
                priority: 1.0,
            },
            Feed {
                url: "https://irozhlas.cz/feed".into(),
                category: "Zprávy".into(),
                name: "iRozhlas".into(),
                priority: 1.0,
            },
        ];
        let mut app = App::new(db, feeds);
        app.filter = crate::store::ListFilter::All;
        app.reload_view().unwrap();

        let out = dump(120, 30, &app);
        println!("{out}");

        assert!(out.contains("dnews"), "app title missing from top border");
        assert!(out.contains("All"), "All tab missing");
        assert!(out.contains("Tech & Dev"), "category tab missing");
        assert!(out.contains("Rust is great"), "article title missing");
        assert!(out.contains("unread"), "unread counter missing");
        assert!(out.contains('─'), "top/bottom border rule missing");
        // Top and bottom rules only, no side borders.
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

        app.filter = crate::store::ListFilter::All;
        app.reload_view().unwrap();
        app.screen = crate::app::Screen::Reader;
        app.reader = Some(crate::reader::ReaderContent::from_plain(
            "Hello world, this is the article body.",
        ));

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

        app.filter = crate::store::ListFilter::All;
        app.reload_view().unwrap();
        // Simulate the debounced preview already having loaded, as it would
        // shortly after the selection settles.
        app.reader_article = app.articles.first().cloned();
        app.reader = Some(crate::reader::ReaderContent::from_plain(
            "Previewed article body text.",
        ));

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

        app.filter = crate::store::ListFilter::All;
        app.reload_view().unwrap();
        app.screen = crate::app::Screen::Reader;
        app.reader_article = app.articles.first().cloned();
        let text: Vec<String> = (1..=100).map(|i| format!("line {i:03}")).collect();
        app.reader = Some(crate::reader::ReaderContent::from_plain(&text.join(
            "
",
        )));

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
        app.filter = crate::store::ListFilter::All;
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
            priority: 1.0,
        }];
        let mut app = App::new(db, feeds);
        app.filter = crate::store::ListFilter::All;
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

    #[test]
    fn footer_hints_are_never_cut_off_at_any_width() {
        let (app, path) = app_with(&[], "footer_widths");
        for width in 30..=80 {
            let out = dump(width, 20, &app);
            assert!(out.contains("q quit"), "hint cut at width {width}:\n{out}");
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn the_selection_stays_visible_with_correct_day_headings() {
        let now = chrono::Utc::now().timestamp();
        // Three articles a day over twenty days, newest first.
        let articles: Vec<NewArticle> = (0..60)
            .map(|i| NewArticle {
                category: "World".into(),
                title: format!("Story {i:02}"),
                link: format!("https://example.com/{i}"),
                published: now - (i as i64 / 3) * 86_400 - (i as i64 % 3) * 60,
                ..Default::default()
            })
            .collect();
        let (mut app, path) = app_with(&articles, "window");
        let today = chrono::Local::now().date_naive();
        for selected in [0, 1, 13, 30, 47, 59] {
            app.selected = selected;
            let out = dump(80, 20, &app);
            let story = format!("Story {selected:02}");
            assert!(
                out.contains(&story),
                "selected {story} not on screen:\n{out}"
            );
            // The heading above it (if shown) belongs to its day.
            let lines: Vec<&str> = out.lines().collect();
            let at = lines.iter().position(|l| l.contains(&story)).unwrap();
            if let Some(heading) = lines[..at]
                .iter()
                .rev()
                .find(|l| l.contains('─') && !l.contains("dnews"))
            {
                let expected = super::list::day_label(app.articles[selected].published, today);
                assert!(
                    heading.contains(&expected),
                    "{story} under {heading:?}, expected {expected}"
                );
            }
        }
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn text_helpers_measure_wide_characters_by_display_width() {
        use unicode_width::UnicodeWidthStr;
        // Each of these characters takes two columns.
        let wide = "日本語のニュース";
        assert!(super::truncate(wide, 7).width() <= 7);
        assert!(truncate_start(wide, 7).width() <= 7);
        assert!(truncate_start(wide, 7).ends_with('ス'));
        let line = Line::from(vec![Span::raw("新聞 "), Span::raw("天気予報")]);
        let cut = truncate_line(line, 7);
        let text: String = cut.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.width() <= 7, "{text:?}");
        for l in wrap_hints(&["日本 語", "ニュース"], " ", 6) {
            assert!(l.width() <= 8, "{l:?}");
        }
    }

    #[test]
    fn the_briefing_reader_and_list_show_story_coverage() {
        let mut app = crate::keys::tests::briefing_app();
        let out = dump(110, 46, &app);
        println!("{out}");
        assert!(out.contains("Briefing"));
        assert!(out.contains("Since "), "window line");
        assert!(out.contains("▸ "), "story rows");
        assert!(out.contains("more stories in") || out.contains("more story in"));

        // The reader names the other outlets.
        app.set_filter(crate::store::ListFilter::All).unwrap();
        let binance = app
            .filtered
            .iter()
            .position(|&i| {
                app.articles[i]
                    .title
                    .starts_with("What we know about Binance")
            })
            .unwrap();
        app.selected = binance;
        let out = dump(110, 30, &app);
        assert!(out.contains("[+"), "badge in the list:\n{out}");
        app.screen = crate::app::Screen::Reader;
        app.reader_article = app.selected_article().cloned();
        let out = dump(110, 30, &app);
        assert!(out.contains("Also covered by"), "{out}");
    }

    #[test]
    fn outlets_label_names_two_and_counts_the_rest() {
        assert_eq!(super::outlets_label(&["BBC"]), "BBC");
        assert_eq!(super::outlets_label(&["BBC", "DW"]), "BBC, DW");
        assert_eq!(
            super::outlets_label(&["BBC", "DW", "ČT24", "NPR"]),
            "BBC, DW +2"
        );
    }
}
