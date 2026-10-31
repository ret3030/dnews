//! Modal panels drawn over the current screen: key help and failed feeds.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use super::theme;
use crate::app::{App, Overlay};

/// (section, [(keys, action)])
pub const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Briefing",
        &[
            ("Enter", "expand a story / read an article"),
            ("M", "done reading: mark it all read"),
            ("b", "in an article: back to its story"),
        ],
    ),
    (
        "List",
        &[
            ("j / k", "move down / up"),
            ("g / G", "top / bottom"),
            ("d / u", "page down / up"),
            ("Enter", "read article"),
            ("Tab / S-Tab", "next / previous tab"),
            ("/", "search titles"),
            ("c", "compact list"),
            ("M", "mark all listed as read"),
            ("Ctrl+R", "reload feeds"),
            ("q", "quit"),
        ],
    ),
    (
        "Article",
        &[
            ("j / k", "scroll"),
            ("d / u, Space", "page down / up"),
            ("g / G", "top / end"),
            ("J / K", "next / previous article"),
            ("1 2 … Enter", "open link [n]"),
            ("w", "wide / narrow text"),
            ("q / Esc", "back to list"),
        ],
    ),
    (
        "Anywhere",
        &[
            ("s", "save for later"),
            ("m", "toggle read / unread"),
            ("o", "open in browser"),
            ("y", "copy link"),
            ("!", "show failed feeds"),
            ("?", "this help"),
        ],
    ),
];

pub fn draw(f: &mut Frame, app: &App, area: Rect) {
    let (title, lines) = match app.overlay {
        Overlay::None => return,
        Overlay::Help => (" Keys ", help_lines()),
        Overlay::FeedErrors => (" Failed feeds ", error_lines(app)),
    };

    let width = 64.min(area.width.saturating_sub(4));
    let height = (lines.len() as u16 + 4).min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(Span::styled(
            title,
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Line::styled(
            " any key to close ",
            Style::default().fg(theme::DIM),
        ))
        .style(Style::default().bg(theme::PANEL));
    let inner = block.inner(rect).inner(ratatui::layout::Margin::new(2, 1));

    f.render_widget(Clear, rect);
    f.render_widget(block, rect);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn help_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (i, (section, keys)) in HELP.iter().enumerate() {
        if i > 0 {
            lines.push(Line::raw(""));
        }
        lines.push(Line::styled(
            *section,
            Style::default()
                .fg(theme::FG_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ));
        for (k, action) in keys.iter() {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<14}"), Style::default().fg(theme::ACCENT)),
                Span::styled(*action, Style::default().fg(theme::FG)),
            ]));
        }
    }
    lines
}

fn error_lines(app: &App) -> Vec<Line<'static>> {
    app.feed_errors
        .iter()
        .flat_map(|(name, err)| {
            [
                Line::styled(
                    name.clone(),
                    Style::default()
                        .fg(theme::FG_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                ),
                Line::styled(format!("  {err}"), Style::default().fg(theme::GRAY)),
            ]
        })
        .collect()
}
