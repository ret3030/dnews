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
    let footer_lines = footer_hint_lines(app, split, area.width).max(1);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(footer_lines),
        ])
        .split(area);

    render_tabs(f, app, chunks[0]);
    render_unread(f, app, chunks[1]);
    render_list(f, app, chunks[2]);
    render_footer(f, app, chunks[3], split);
}

/// The pill-style tab bar needs a Nerd Font for its powerline cap glyphs
/// (`LEFT_CAP`/`RIGHT_CAP`) and enough columns for every category to get its
/// own pill. Below this, `render_tabs` falls back to plain-text tabs with no
/// special glyphs — still fully functional, just less decorative, and immune
/// to the column-width ambiguity those glyphs have without a Nerd Font.
const COMPACT_TABS_MAX_WIDTH: u16 = 70;

fn tab_labels(app: &App) -> Vec<(String, bool)> {
    let mut labels: Vec<(String, bool)> = vec![("All".into(), app.filter == ListFilter::All)];
    labels.extend(
        app.categories
            .iter()
            .map(|c| (c.clone(), app.filter == ListFilter::Category(c.clone()))),
    );
    labels.push(("Saved".into(), app.filter == ListFilter::Saved));
    labels
}

fn render_tabs(f: &mut Frame, app: &App, area: Rect) {
    let labels = tab_labels(app);
    let line = if area.width <= COMPACT_TABS_MAX_WIDTH {
        compact_tabs_line(&labels)
    } else {
        pill_tabs_line(&labels)
    };
    let line = super::truncate_line(line, area.width as usize);
    f.render_widget(
        Paragraph::new(line).style(Style::default().bg(theme::PANEL)),
        area,
    );
}

/// Plain-text fallback tab bar: no powerline caps, active tab shown via
/// bold accent color and `·` separators only — every glyph here is a normal
/// single-width ASCII/Unicode character, so it never misaligns regardless of
/// font support, and reads fine even heavily wrapped/clipped on tiny widths.
fn compact_tabs_line(labels: &[(String, bool)]) -> Line<'static> {
    let panel = Style::default().bg(theme::PANEL);
    let mut spans: Vec<Span> = vec![Span::styled(" ", panel)];
    for (i, (label, active)) in labels.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                " · ",
                Style::default().fg(theme::DIM).bg(theme::PANEL),
            ));
        }
        let style = if *active {
            Style::default()
                .fg(theme::ACCENT)
                .bg(theme::PANEL)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::GRAY).bg(theme::PANEL)
        };
        spans.push(Span::styled(label.clone(), style));
    }
    Line::from(spans)
}

fn pill_tabs_line(labels: &[(String, bool)]) -> Line<'static> {
    let panel = Style::default().bg(theme::PANEL);
    let mut spans: Vec<Span> = vec![Span::styled(" ", panel)];

    for (label, active) in labels {
        // The Saved tab's bookmark icon is decorative and Nerd-Font-
        // dependent like the pill caps themselves, so it only shows up
        // here — the compact fallback tabs never use it.
        let text = if label == "Saved" {
            format!(" \u{f005} {label} ")
        } else {
            format!(" {label} ")
        };
        if *active {
            spans.push(Span::styled(
                LEFT_CAP.to_string(),
                Style::default().fg(theme::ACCENT).bg(theme::PANEL),
            ));
            spans.push(Span::styled(
                text,
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
                text,
                Style::default().fg(theme::GRAY).bg(theme::PANEL),
            ));
        }
        spans.push(Span::styled(" ", panel));
    }

    Line::from(spans)
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

            // `List`/`ListItem` clip overflowing text at the area edge instead of
            // wrapping it, which hard-cuts long titles mid-word — truncate with an
            // ellipsis ourselves instead, budgeting for the fixed-width prefix/
            // suffix spans on each line.
            let line1_prefix_width = 5 + 2 + if row.saved { 2 } else { 0 };
            let title = super::truncate(
                &row.title,
                (list_area.width as usize).saturating_sub(line1_prefix_width),
            );
            let mut line1_spans = vec![
                Span::styled(format!("{num}  "), Style::default().fg(theme::DIM)),
                dot,
                Span::styled(title, title_style),
            ];
            if row.saved {
                line1_spans.push(Span::styled(
                    " \u{f005}",
                    Style::default().fg(theme::YELLOW),
                ));
            }
            let line1 = Line::from(line1_spans);

            let meta_suffix = format!(" · {ago}");
            let line2_prefix_width = 7 + meta_suffix.chars().count();
            let domain_trunc = super::truncate(
                &domain,
                (list_area.width as usize).saturating_sub(line2_prefix_width),
            );
            let line2 = Line::from(vec![
                Span::raw("       "),
                Span::styled(domain_trunc, domain_style),
                Span::styled(meta_suffix, meta_style),
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

/// The default (no search/loading/status) footer hint, as atomic phrases —
/// used both to render it and, via `footer_hint_lines`, to size the footer
/// row so a narrow terminal wraps onto a second line instead of clipping.
fn default_hints(app: &App, split: bool) -> &'static [&'static str] {
    if !split {
        &[
            "↵ read",
            "/ search",
            "Tab/S-Tab tabs",
            "s save",
            "^R reload",
            "j/k/g/G nav",
            "q quit",
        ]
    } else if app.panel_focused {
        &[
            "Shift+J/K browse+preview",
            "/ search",
            "Tab/S-Tab tabs",
            "^R reload",
            "q back to list",
        ]
    } else {
        &[
            "↵ focus article",
            "/ search",
            "Tab/S-Tab tabs",
            "s save",
            "^R reload",
            "j/k/g/G nav",
            "q quit",
        ]
    }
}

/// How many rows the footer needs at `width` columns: 1 for the search box,
/// loading bar, or a status message (all bounded/truncated to fit one line),
/// or however many the default hint text wraps to (see `super::wrap_hints`).
fn footer_hint_lines(app: &App, split: bool, width: u16) -> u16 {
    if app.search_active || app.loading || !app.status.is_empty() {
        return 1;
    }
    let usable = width.saturating_sub(1) as usize;
    super::wrap_hints(default_hints(app, split), "   ", usable).len() as u16
}

fn render_footer(f: &mut Frame, app: &App, area: Rect, split: bool) {
    let width = area.width.saturating_sub(2) as usize;
    let lines: Vec<Line> = if app.search_active {
        let query = super::truncate_start(&app.search, width.saturating_sub(3));
        vec![Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme::YELLOW)),
            Span::styled(query, Style::default().fg(theme::FG)),
            Span::styled("█", Style::default().fg(theme::YELLOW)),
        ])]
    } else if app.loading {
        let prefix = format!(" {} Fetching feeds  ", super::spinner(app.spin_frame));
        let total = app.total_feeds.max(1);
        let done = app.done_feeds.min(total);
        let suffix = format!("  {done}/{total}");
        // Shrink the bar itself (not the surrounding text) to whatever's
        // left, so the whole line always fits one row instead of wrapping
        // a progress bar mid-fill.
        let bar_width = (area.width as usize)
            .saturating_sub(prefix.chars().count() + suffix.chars().count())
            .clamp(4, 24);
        let filled = done * bar_width / total;
        let bar: String = "█".repeat(filled) + &"░".repeat(bar_width - filled);
        vec![Line::from(vec![
            Span::styled(prefix, Style::default().fg(theme::ACCENT)),
            Span::styled(bar, Style::default().fg(theme::ACCENT)),
            Span::styled(suffix, Style::default().fg(theme::GRAY)),
        ])]
    } else if !app.status.is_empty() {
        vec![Line::from(Span::styled(
            format!(" {}", super::truncate(&app.status, width.saturating_sub(1))),
            Style::default().fg(theme::RED),
        ))]
    } else {
        // The split layout's hint depends on where plain-vim-key input
        // currently goes (see `App::panel_focused` / `keys::handle_list_wide`):
        // while the panel is focused, only Shift+J/K still reach the list.
        super::wrap_hints(default_hints(app, split), "   ", width)
            .into_iter()
            .map(|l| {
                Line::from(Span::styled(
                    format!(" {l}"),
                    Style::default().fg(theme::GRAY),
                ))
            })
            .collect()
    };

    f.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::PANEL)),
        area,
    );
}
