use chrono::TimeZone;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use super::theme;
use crate::app::App;

const READ_WIDTH: u16 = 88;

/// Renders the article view. `panel` distinguishes the full-screen reader
/// (narrow terminals, opened via `Enter`, `q`/`Esc` to leave) from the wide
/// split layout's side panel (see `ui::SPLIT_MIN_WIDTH`) — same content and
/// scrolling, but the panel's footer hint further depends on
/// `App::panel_focused` (see `render_footer`).
pub fn draw(f: &mut Frame, app: &App, area: Rect, panel: bool) {
    let area = super::centered(area, READ_WIDTH);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(f, app, chunks[0]);
    render_body(f, app, chunks[1]);
    render_footer(f, app, chunks[2], panel);
}

fn render_header(f: &mut Frame, app: &App, area: Rect) {
    let row = app.reader_article.as_ref();
    let (title, domain, date, saved) = match row {
        Some(r) => {
            let domain = super::domain(&r.link);
            let date = chrono::Utc
                .timestamp_opt(r.published, 0)
                .single()
                .map(|d| d.format("%a %d %b %Y  ·  %H:%M").to_string())
                .unwrap_or_default();
            (r.title.clone(), domain, date, r.saved)
        }
        None => (String::new(), String::new(), String::new(), false),
    };

    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_type(BorderType::Plain)
        .border_style(Style::default().fg(theme::BORDER));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let mut title_spans = vec![Span::styled(
        title,
        Style::default()
            .fg(theme::FG_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )];
    if saved {
        title_spans.push(Span::styled(
            " \u{f005}",
            Style::default().fg(theme::YELLOW),
        ));
    }

    let header = Paragraph::new(vec![
        Line::from(title_spans),
        Line::from(Span::styled(
            format!("{date}  ·  {domain}"),
            Style::default().fg(theme::GRAY),
        )),
    ])
    .alignment(Alignment::Center)
    .wrap(Wrap { trim: true });
    f.render_widget(header, inner);
}

fn render_body(f: &mut Frame, app: &App, area: Rect) {
    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(2), Constraint::Min(0)])
        .split(area)[1];

    let body = if app.reader_article.is_none() {
        Paragraph::new(Line::from(Span::styled(
            "Select an article",
            Style::default().fg(theme::DIM),
        )))
        .alignment(Alignment::Center)
    } else if app.reader_loading {
        Paragraph::new(Line::from(Span::styled(
            format!("{} Loading article...", super::spinner(app.spin_frame)),
            Style::default().fg(theme::GRAY),
        )))
        .alignment(Alignment::Center)
    } else if let Some(err) = &app.reader_error {
        Paragraph::new(Line::from(Span::styled(
            format!("Could not load article: {err}"),
            Style::default().fg(theme::RED),
        )))
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
    } else if let Some(content) = &app.reader {
        Paragraph::new(
            content
                .text
                .lines()
                .map(|l| {
                    let trimmed = l.trim_start();
                    if trimmed.starts_with("[image:") {
                        Line::from(Span::styled(
                            l.to_string(),
                            Style::default()
                                .fg(theme::MAUVE)
                                .add_modifier(Modifier::ITALIC),
                        ))
                    } else {
                        Line::from(Span::styled(l.to_string(), Style::default().fg(theme::FG)))
                    }
                })
                .collect::<Vec<_>>(),
        )
        .wrap(Wrap { trim: false })
        .scroll((app.reader_scroll, 0))
    } else {
        Paragraph::new("")
    };
    f.render_widget(body, inner);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect, panel: bool) {
    // The split-layout panel's hint depends on whether it currently owns
    // plain-vim-key input (see `App::panel_focused` / `keys::handle_list_wide`).
    let hint = if !panel {
        " q/Esc back   j/k scroll   g/G top/bottom   d/u page   s save"
    } else if app.panel_focused {
        " q/Esc unfocus   j/k scroll   g/G top/bottom   d/u page   s save"
    } else {
        " ↵ focus to scroll   Shift+J/K browse+preview"
    };
    let line = if app.reader_loading {
        Line::from(Span::styled(hint, Style::default().fg(theme::DIM)))
    } else {
        Line::from(vec![Span::styled(hint, Style::default().fg(theme::GRAY))])
    };
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme::PANEL)),
        area,
    );
}
