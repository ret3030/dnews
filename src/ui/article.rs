use chrono::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

use super::{document, theme};
use crate::app::App;

/// Text column width: comfortable for reading by default, wider with `w`.
const TEXT_WIDTH: u16 = 76;
const TEXT_WIDTH_WIDE: u16 = 100;
/// Left padding plus a gap and the scrollbar on the right.
const SIDE_COLUMNS: u16 = 5;

/// Renders the article: full screen on narrow terminals, or as the side
/// panel of the wide split layout (`panel`).
pub fn draw(f: &mut Frame, app: &App, area: Rect, panel: bool) {
    let text_width = if app.reader_wide {
        TEXT_WIDTH_WIDE
    } else {
        TEXT_WIDTH
    };
    let area = super::centered(area, text_width + SIDE_COLUMNS);
    let title = app
        .reader_article
        .as_ref()
        .map(|a| a.title.as_str())
        .unwrap_or_default();
    let title = super::wrap_words(
        title,
        area.width.saturating_sub(4) as usize,
        MAX_TITLE_LINES,
    );
    let footer = footer_lines(app, panel, area.width as usize);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            // Title lines, the meta line and the bottom rule.
            Constraint::Length(title.len() as u16 + 2),
            Constraint::Min(0),
            Constraint::Length(footer.len() as u16),
        ])
        .split(area);

    render_header(f, app, title, chunks[0]);
    render_body(f, app, chunks[1]);
    f.render_widget(
        Paragraph::new(footer).style(Style::default().bg(theme::PANEL)),
        chunks[2],
    );
    if let Some(pct) = progress(app) {
        let row = Rect {
            height: 1,
            ..chunks[2]
        };
        let pct = Line::styled(format!("{pct} "), Style::default().fg(theme::GRAY));
        f.render_widget(Paragraph::new(pct.right_aligned()), row);
    }
}

const MAX_TITLE_LINES: usize = 3;

fn render_header(f: &mut Frame, app: &App, title: Vec<String>, area: Rect) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(theme::BORDER));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(article) = app.reader_article.as_ref() else {
        return;
    };
    let source = if article.source.is_empty() {
        super::domain(&article.link)
    } else {
        article.source.clone()
    };
    let mut meta = vec![source];
    if let Some(date) = chrono::Local.timestamp_opt(article.published, 0).single() {
        meta.insert(0, date.format("%a %-d %b %Y, %H:%M").to_string());
    }
    if let Some(content) = app.reader.as_ref().filter(|_| !app.reader_loading) {
        meta.push(format!("{} min read", content.reading_minutes()));
    }

    let title_style = Style::default()
        .fg(theme::FG_BRIGHT)
        .add_modifier(Modifier::BOLD);
    let mut lines: Vec<Line> = title
        .into_iter()
        .map(|l| Line::from(Span::styled(l, title_style)))
        .collect();
    if article.saved
        && let Some(last) = lines.last_mut()
    {
        last.push_span(Span::styled(
            " \u{f005}",
            Style::default().fg(theme::YELLOW),
        ));
    }
    lines.push(Line::styled(
        meta.join("  ·  "),
        Style::default().fg(theme::GRAY),
    ));
    f.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

fn render_body(f: &mut Frame, app: &App, area: Rect) {
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .split(area);
    let (text_area, bar_area) = (cols[1], cols[3]);
    let width = text_area.width as usize;

    let lines: Vec<Line> = if app.reader_article.is_none() {
        vec![Line::styled("Select an article", Style::default().fg(theme::DIM)).centered()]
    } else if app.reader_loading {
        vec![
            Line::styled(
                format!("{} Loading article...", super::spinner(app.spin_frame)),
                Style::default().fg(theme::GRAY),
            )
            .centered(),
        ]
    } else if let Some(err) = &app.reader_error {
        error_lines(app, err, width)
    } else if let Some(content) = &app.reader {
        document::render(content, width)
    } else {
        Vec::new()
    };

    // Remember how far the body can scroll, so the scroll keys stop at the
    // last line (see `App::reader_max_scroll`). Also clamp what is drawn, in
    // case the terminal grew since the last key press.
    let max = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_sub(text_area.height);
    app.reader_max_scroll.set(max);
    let scroll = app.reader_scroll.min(max);
    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), text_area);

    if max > 0 {
        let mut state = ScrollbarState::new(max as usize).position(scroll as usize);
        let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .track_symbol(Some("│"))
            .track_style(Style::default().fg(theme::BORDER))
            .thumb_symbol("┃")
            .thumb_style(Style::default().fg(theme::GRAY));
        f.render_stateful_widget(bar, bar_area, &mut state);
    }
}

/// When the full article can't be fetched, show why plus the feed's own
/// summary, so there is still something to read.
fn error_lines(app: &App, err: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = document::wrap(
        vec![(
            format!("Could not load the full article: {err}"),
            Style::default().fg(theme::RED),
        )],
        width,
        vec![],
        vec![],
    );
    lines.push(Line::styled(
        "Press o to open it in the browser.",
        Style::default().fg(theme::GRAY),
    ));
    let summary = app
        .reader_article
        .as_ref()
        .map(|a| a.summary.clone())
        .unwrap_or_default();
    if !summary.is_empty() {
        lines.push(Line::raw(""));
        lines.extend(document::wrap(
            vec![(summary, Style::default().fg(theme::FG))],
            width,
            vec![],
            vec![],
        ));
    }
    lines
}

fn footer_hints(app: &App, panel: bool) -> &'static [&'static str] {
    if !panel {
        &[
            "j/k scroll",
            "J/K next/prev",
            "o browser",
            "q back",
            "? help",
        ]
    } else if app.panel_focused {
        &["j/k scroll", "o browser", "q back", "? help"]
    } else {
        &["↵ read here", "J/K preview"]
    }
}

/// How far through the article the view is, e.g. "42%".
fn progress(app: &App) -> Option<String> {
    let max = app.reader_max_scroll.get();
    if app.reader.is_none() || max == 0 || max == u16::MAX {
        return None;
    }
    let pos = app.reader_scroll.min(max) as usize;
    Some(format!("{}%", pos * 100 / max as usize))
}

fn footer_lines(app: &App, panel: bool, width: usize) -> Vec<Line<'static>> {
    let focus = reader_has_focus(app, panel);
    let accent = Style::default().fg(theme::ACCENT);
    if focus && !app.link_input.is_empty() {
        return vec![Line::styled(
            format!(" Open link {}: Enter   Esc cancel", app.link_input),
            accent,
        )];
    }
    if let Some(notice) = app.notice.as_ref().filter(|_| focus) {
        return vec![Line::styled(
            format!(" {}", super::truncate(notice, width.saturating_sub(2))),
            accent,
        )];
    }
    let color = if app.reader_loading {
        theme::DIM
    } else {
        theme::GRAY
    };
    // Room on the right for the progress percentage.
    super::wrap_hints(footer_hints(app, panel), "   ", width.saturating_sub(6))
        .into_iter()
        .map(|l| Line::styled(format!(" {l}"), Style::default().fg(color)))
        .collect()
}

/// Whether keys currently go to the article (so its footer shows notices).
fn reader_has_focus(app: &App, panel: bool) -> bool {
    !panel || app.panel_focused
}
