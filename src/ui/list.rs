use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use super::theme;
use crate::app::App;
use crate::store::ListFilter;

const MAX_WIDTH: u16 = 130;
const LEFT_CAP: char = '\u{e0b6}';
const RIGHT_CAP: char = '\u{e0b4}';

pub fn draw(f: &mut Frame, app: &App, area: Rect, split: bool) {
    let area = super::centered(area, MAX_WIDTH);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    render_tabs(f, app, chunks[0]);
    render_unread(f, app, chunks[1]);
    render_list(f, app, chunks[2]);
    render_footer(f, app, chunks[3], split);
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let mut labels: Vec<(String, bool)> = vec![("All".into(), app.filter == ListFilter::All)];
    labels.extend(
        app.categories
            .iter()
            .map(|c| (c.clone(), app.filter == ListFilter::Category(c.clone()))),
    );
    labels.push(("\u{f005} Saved".into(), app.filter == ListFilter::Saved));

    let panel = Style::default().bg(theme::PANEL);
    let mut spans: Vec<Span> = vec![Span::styled(" ", panel)];

    for (label, active) in labels {
        if active {
            spans.push(Span::styled(
                LEFT_CAP.to_string(),
                Style::default().fg(theme::ACCENT).bg(theme::PANEL),
            ));
            spans.push(Span::styled(
                format!(" {label} "),
                Style::default()
                    .fg(theme::BG)
                    .bg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::styled(
                RIGHT_CAP.to_string(),
                Style::default().fg(theme::ACCENT).bg(theme::PANEL),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {label} "),
                Style::default().fg(theme::GRAY).bg(theme::PANEL),
            ));
        }
        spans.push(Span::styled(" ", panel));
    }

    f.render_widget(Paragraph::new(Line::from(spans)).style(panel), area);
}

fn render_unread(f: &mut Frame, app: &App, area: Rect) {
    let line = Line::from(vec![Span::styled(
        format!(" {} unread", app.unread_count),
        Style::default().fg(theme::GRAY),
    )]);
    f.render_widget(Paragraph::new(line), area);
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(area);
    let list_area = inner[1];

    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .enumerate()
        .filter_map(|(display_idx, &article_idx)| {
            let row = app.articles.get(article_idx)?;
            let domain = super::domain(&row.link);
            let ago = super::time_ago(row.published);
            let num = format!("{:>3}", display_idx + 1);

            let (dot, title_style, meta_style, domain_style) = if row.unread {
                (
                    Span::styled("● ", Style::default().fg(theme::ACCENT)),
                    Style::default()
                        .fg(theme::FG_BRIGHT)
                        .add_modifier(Modifier::BOLD),
                    Style::default().fg(theme::GRAY),
                    Style::default()
                        .fg(theme::source_color(&domain))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                let dim = Style::default().fg(theme::DIM);
                (Span::styled("  ", dim), dim, dim, dim)
            };

            let mut line1_spans = vec![
                Span::styled(format!("{num}  "), Style::default().fg(theme::DIM)),
                dot,
                Span::styled(row.title.clone(), title_style),
            ];
            if row.saved {
                line1_spans.push(Span::styled(
                    " \u{f005}",
                    Style::default().fg(theme::YELLOW),
                ));
            }
            let line1 = Line::from(line1_spans);

            let line2 = Line::from(vec![
                Span::raw("       "),
                Span::styled(domain, domain_style),
                Span::styled(format!(" · {ago}"), meta_style),
            ]);

            Some(ListItem::new(Text::from(vec![line1, line2, Line::raw("")])))
        })
        .collect();

    let empty_msg = if app.loading {
        format!("  {} Fetching feeds...", super::spinner(app.spin_frame))
    } else if app.filter == ListFilter::Saved {
        "  Nothing saved yet — press 's' on an article to save it".to_string()
    } else if app.search_active || !app.search.is_empty() {
        "  No matches".to_string()
    } else {
        "  No articles yet".to_string()
    };

    if items.is_empty() {
        f.render_widget(
            Paragraph::new(empty_msg).style(Style::default().fg(theme::GRAY)),
            list_area,
        );
        return;
    }

    let list = List::new(items).highlight_style(Style::default().bg(theme::SEL_BG));

    let mut state = ListState::default().with_selected(Some(app.selected));
    f.render_stateful_widget(list, list_area, &mut state);
}

fn render_footer(f: &mut Frame, app: &App, area: Rect, split: bool) {
    let width = area.width.saturating_sub(2) as usize;
    let line = if app.search_active {
        Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme::YELLOW)),
            Span::styled(app.search.clone(), Style::default().fg(theme::FG)),
            Span::styled("█", Style::default().fg(theme::YELLOW)),
        ])
    } else if app.loading {
        let bar_width = 24usize;
        let total = app.total_feeds.max(1);
        let done = app.done_feeds.min(total);
        let filled = done * bar_width / total;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);
        Line::from(vec![
            Span::styled(
                format!(" {} Fetching feeds  ", super::spinner(app.spin_frame)),
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(bar, Style::default().fg(theme::ACCENT)),
            Span::styled(
                format!("  {done}/{total}"),
                Style::default().fg(theme::GRAY),
            ),
        ])
    } else if !app.status.is_empty() {
        Line::from(Span::styled(
            format!(" {}", super::truncate(&app.status, width.saturating_sub(1))),
            Style::default().fg(theme::RED),
        ))
    } else {
        // The split layout's hint depends on where plain-vim-key input
        // currently goes (see `App::panel_focused` / `keys::handle_list_wide`):
        // while the panel is focused, only Shift+J/K still reach the list.
        let hint = if !split {
            " ↵ read   / search   Tab/S-Tab tabs   s save   ^R reload   j/k/g/G nav   q quit"
        } else if app.panel_focused {
            " Shift+J/K browse+preview   / search   Tab/S-Tab tabs   ^R reload   q back to list"
        } else {
            " ↵ focus article   / search   Tab/S-Tab tabs   s save   ^R reload   j/k/g/G nav   q quit"
        };
        Line::from(Span::styled(hint, Style::default().fg(theme::GRAY)))
    };

    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme::PANEL)),
        area,
    );
}
