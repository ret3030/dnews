use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{List, ListItem, ListState, Paragraph};

use chrono::{Local, TimeZone};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::App;
use crate::store::{ArticleRow, ListFilter};

const MAX_WIDTH: u16 = 130;
/// Empty columns kept to the right of every title.
const RIGHT_GUTTER: u16 = 3;
/// Columns before a title starts: two spaces plus the unread dot.
const TITLE_INDENT: usize = 4;
/// Source name column width in compact mode.
const COMPACT_SOURCE_W: usize = 16;
const LEFT_CAP: char = '\u{e0b6}';
const RIGHT_CAP: char = '\u{e0b4}';

pub fn draw(f: &mut Frame, app: &App, area: Rect, split: bool) {
    let area = super::centered(area, MAX_WIDTH);
    let footer = footer_lines(app, split, area.width as usize);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(footer.len() as u16),
        ])
        .split(area);

    render_tabs(f, app, chunks[0]);
    render_unread(f, app, chunks[1]);
    render_list(f, app, chunks[2]);
    f.render_widget(
        Paragraph::new(footer).style(Style::default().bg(theme::PANEL)),
        chunks[3],
    );
}

/// The pill-style tab bar needs a Nerd Font for its powerline cap glyphs
/// (`LEFT_CAP`/`RIGHT_CAP`) and enough columns for every category to get its
/// own pill. Below this, `render_tabs` falls back to plain-text tabs with no
/// special glyphs — still fully functional, just less decorative, and immune
/// to the column-width ambiguity those glyphs have without a Nerd Font.
const COMPACT_TABS_MAX_WIDTH: u16 = 70;

fn tab_labels(app: &App) -> Vec<(String, bool)> {
    let label = |name: &str, unread: i64| {
        if unread > 0 {
            format!("{name} {unread}")
        } else {
            name.to_string()
        }
    };
    let mut labels = vec![(
        label("All", app.unread_count),
        app.filter == ListFilter::All,
    )];
    labels.extend(app.categories.iter().map(|c| {
        let unread = app.tab_unread.get(c).copied().unwrap_or(0);
        (
            label(c, unread),
            app.filter == ListFilter::Category(c.clone()),
        )
    }));
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
        let text = if *label == "Saved" {
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
    let shown = app.filtered.len();
    let unread = app
        .filtered
        .iter()
        .filter_map(|&i| app.articles.get(i))
        .filter(|a| a.unread)
        .count();
    let line = Line::from(vec![Span::styled(
        format!(" {unread} unread of {shown}"),
        Style::default().fg(theme::GRAY),
    )]);
    f.render_widget(Paragraph::new(line), area);
}

/// Heading for the day an article was published, in local time.
pub fn day_label(published: i64, today: chrono::NaiveDate) -> String {
    let Some(date) = Local
        .timestamp_opt(published, 0)
        .single()
        .filter(|_| published > 0)
        .map(|d| d.date_naive())
    else {
        return "Undated".into();
    };
    match (today - date).num_days() {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => date.format("%a %-d %b").to_string(),
    }
}

/// Builds one list entry; `width` is the usable column count.
fn article_item(row: &ArticleRow, width: usize, compact: bool) -> ListItem<'static> {
    let source = if row.source.is_empty() {
        super::domain(&row.link)
    } else {
        row.source.clone()
    };
    let ago = super::time_ago(row.published);

    let (dot, title_style, source_style, meta_style) = if row.unread {
        (
            Span::styled("● ", Style::default().fg(theme::ACCENT)),
            Style::default()
                .fg(theme::FG_BRIGHT)
                .add_modifier(Modifier::BOLD),
            Style::default()
                .fg(theme::source_color(&source))
                .add_modifier(Modifier::BOLD),
            Style::default().fg(theme::GRAY),
        )
    } else {
        // Read items step back but stay legible.
        (
            Span::raw("  "),
            Style::default().fg(theme::GRAY),
            Style::default().fg(theme::DIM),
            Style::default().fg(theme::DIM),
        )
    };
    let star = row
        .saved
        .then(|| Span::styled(" \u{f005}", Style::default().fg(theme::YELLOW)));
    let star_w = if row.saved { 2 } else { 0 };

    if compact {
        // Fixed-width meta column so every title ends at the same place:
        // "  ● Title ……………   BBC World          3h ago"
        let source = super::truncate(&source, COMPACT_SOURCE_W);
        let pad = " ".repeat(COMPACT_SOURCE_W.saturating_sub(source.width()));
        let meta = format!("  {source}{pad} {ago:>7}");
        let title_w = width.saturating_sub(TITLE_INDENT + star_w + meta.width());
        let title = super::truncate(&row.title, title_w);
        let pad = title_w.saturating_sub(title.width());
        let mut spans = vec![Span::raw("  "), dot, Span::styled(title, title_style)];
        spans.extend(star);
        spans.push(Span::raw(" ".repeat(pad)));
        spans.push(Span::styled(meta, meta_style));
        return ListItem::new(Line::from(spans));
    }

    // `List` clips at the area edge instead of wrapping, so wrap the title
    // ourselves at word boundaries: up to two lines, then an ellipsis.
    let title_w = width.saturating_sub(TITLE_INDENT + star_w);
    let mut title_lines = super::wrap_words(&row.title, title_w, 2).into_iter();
    let mut first = vec![
        Span::raw("  "),
        dot,
        Span::styled(title_lines.next().unwrap_or_default(), title_style),
    ];
    first.extend(star);
    let mut lines = vec![Line::from(first)];
    for rest in title_lines {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(TITLE_INDENT)),
            Span::styled(rest, title_style),
        ]));
    }
    let suffix = format!(" · {ago}");
    let source = super::truncate(&source, width.saturating_sub(TITLE_INDENT + suffix.width()));
    lines.push(Line::from(vec![
        Span::raw(" ".repeat(TITLE_INDENT)),
        Span::styled(source, source_style),
        Span::styled(suffix, meta_style),
    ]));
    lines.push(Line::raw(""));
    ListItem::new(Text::from(lines))
}

fn separator_item(label: &str, width: usize) -> ListItem<'static> {
    let rule = "─".repeat(width.saturating_sub(label.width() + 3));
    ListItem::new(Line::from(vec![
        Span::styled(
            format!(" {label} "),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(rule, Style::default().fg(theme::BORDER)),
    ]))
}

fn render_list(f: &mut Frame, app: &App, area: Rect) {
    // A gutter on both sides keeps titles off the pane edge (and, in the
    // split layout, away from the article panel).
    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(RIGHT_GUTTER),
        ])
        .split(area);
    let list_area = inner[1];
    let width = list_area.width as usize;

    // Only articles that can be on screen are built: a screen's worth on
    // each side of the selection (every entry is at least one line tall).
    // Day separators are extra items, so the selection is re-indexed.
    let today = Local::now().date_naive();
    let reach = list_area.height as usize;
    let start = app.selected.saturating_sub(reach);
    let end = (app.selected + reach + 1).min(app.filtered.len());
    let article_at = |pos: usize| app.filtered.get(pos).and_then(|&i| app.articles.get(i));
    let mut last_day = start
        .checked_sub(1)
        .and_then(article_at)
        .map(|row| day_label(row.published, today));
    let mut items = Vec::with_capacity(2 * (end - start));
    let mut selected_item = 0;
    for pos in start..end {
        let Some(row) = article_at(pos) else {
            continue;
        };
        let day = day_label(row.published, today);
        if last_day.as_ref() != Some(&day) {
            items.push(separator_item(&day, width));
            last_day = Some(day);
        }
        if pos == app.selected {
            selected_item = items.len();
        }
        items.push(article_item(row, width, app.compact));
    }

    if items.is_empty() {
        let empty_msg = if app.loading {
            format!("  {} Fetching feeds...", super::spinner(app.spin_frame))
        } else if app.filter == ListFilter::Saved {
            "  Nothing saved yet. Press s on an article to save it.".to_string()
        } else if app.search_active || !app.search.is_empty() {
            "  No matches".to_string()
        } else {
            "  No articles yet".to_string()
        };
        f.render_widget(
            Paragraph::new(empty_msg).style(Style::default().fg(theme::GRAY)),
            list_area,
        );
        return;
    }

    let list = List::new(items).highlight_style(Style::default().bg(theme::SEL_BG));
    let mut state = ListState::default().with_selected(Some(selected_item));
    f.render_stateful_widget(list, list_area, &mut state);
}

/// The default (no search/loading/status) footer hint, as atomic phrases —
/// used both to render it and, via `footer_hint_lines`, to size the footer
/// row so a narrow terminal wraps onto a second line instead of clipping.
fn default_hints(app: &App, split: bool) -> &'static [&'static str] {
    if !split {
        &["↵ read", "/ search", "Tab tabs", "? help", "q quit"]
    } else if app.panel_focused {
        &["J/K next/prev", "q back to list", "? help"]
    } else {
        &[
            "↵ read",
            "J/K preview",
            "/ search",
            "Tab tabs",
            "? help",
            "q quit",
        ]
    }
}

/// The footer: search box, reload progress, a notice or warning, or the key
/// hints (wrapped onto more lines on a narrow terminal).
fn footer_lines(app: &App, split: bool, width: usize) -> Vec<Line<'static>> {
    let width = width.saturating_sub(2);
    let one = |text: String, color| vec![Line::styled(text, Style::default().fg(color))];

    if app.search_active {
        let query = super::truncate_start(&app.search, width.saturating_sub(3));
        return vec![Line::from(vec![
            Span::styled(" / ", Style::default().fg(theme::YELLOW)),
            Span::styled(query, Style::default().fg(theme::FG)),
            Span::styled("█", Style::default().fg(theme::YELLOW)),
        ])];
    }
    if app.loading {
        let prefix = format!(" {} Fetching feeds  ", super::spinner(app.spin_frame));
        let total = app.total_feeds.max(1);
        let done = app.done_feeds.min(total);
        let suffix = format!("  {done}/{total}");
        // The bar shrinks so the line never wraps.
        let bar_width = width
            .saturating_sub(prefix.width() + suffix.width())
            .clamp(4, 24);
        let filled = done * bar_width / total;
        let bar = "█".repeat(filled) + &"░".repeat(bar_width - filled);
        return vec![Line::from(vec![
            Span::styled(prefix, Style::default().fg(theme::ACCENT)),
            Span::styled(bar, Style::default().fg(theme::ACCENT)),
            Span::styled(suffix, Style::default().fg(theme::GRAY)),
        ])];
    }
    if let Some(notice) = list_notice(app, split) {
        return one(
            format!(" {}", super::truncate(notice, width.saturating_sub(1))),
            theme::ACCENT,
        );
    }
    if !app.status.is_empty() {
        return one(
            format!(
                " ⚠ {}",
                super::truncate(&app.status, width.saturating_sub(3))
            ),
            theme::YELLOW,
        );
    }
    super::wrap_hints(default_hints(app, split), "   ", width)
        .into_iter()
        .map(|l| Line::styled(format!(" {l}"), Style::default().fg(theme::GRAY)))
        .collect()
}

/// The notice belongs to the list footer unless the split layout's article
/// panel has focus (then the panel's footer shows it).
fn list_notice(app: &App, split: bool) -> Option<&String> {
    app.notice
        .as_ref()
        .filter(|_| !(split && app.panel_focused))
}
