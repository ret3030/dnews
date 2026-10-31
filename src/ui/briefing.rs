//! The Briefing tab: the top stories, then the best of each category; with
//! a story open, its articles.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::App;
use crate::store::ArticleRow;

const TOP_HEADING: &str = "Top stories";
/// Columns before a headline: two spaces and a rank or unread dot.
const INDENT: usize = 5;
/// Right-hand column with "8 outlets" or a single story's outlet.
const LABEL_W: usize = 18;
/// An open story's headline and summary wrap at this width at most.
const TEXT_W: usize = 76;
/// Source column of an open story's articles.
const SOURCE_W: usize = 18;

/// List items for the Briefing and the index of the selected one.
pub fn items(app: &App, width: usize) -> (Vec<ListItem<'static>>, usize) {
    match app.story {
        Some(story) => story_items(app, story, width),
        None => briefing_items(app, width),
    }
}

/// The stories under a heading for the top ones and one per category.
fn briefing_items(app: &App, width: usize) -> (Vec<ListItem<'static>>, usize) {
    let mut items = Vec::new();
    let mut selected_item = 0;
    let mut heading: Option<&str> = None;
    for (row, &story) in app.briefing_rows.iter().enumerate() {
        let top = row < app.briefing_top;
        let row_heading = if top {
            TOP_HEADING
        } else {
            app.stories[story].category.as_str()
        };
        if heading != Some(row_heading) {
            // Full story rows end in a blank line already.
            if heading.is_some() && app.compact {
                items.push(ListItem::new(""));
            }
            items.push(ListItem::new(super::list::separator_line(
                row_heading,
                width,
            )));
            heading = Some(row_heading);
        }
        if row == app.selected {
            selected_item = items.len();
        }
        items.push(story_item(app, story, top.then_some(row + 1), width));
    }
    (items, selected_item)
}

/// One story: " 1  Headline …   8 outlets  2h ago", the headline on up to
/// two lines (one in compact mode). `rank` numbers a top story.
fn story_item(app: &App, story: usize, rank: Option<usize>, width: usize) -> ListItem<'static> {
    let s = &app.stories[story];
    let head = &app.articles[s.head];
    let unread = s.members.iter().any(|&m| app.articles[m].unread);
    let (title_style, meta_style) = if unread {
        (
            Style::default()
                .fg(theme::FG_BRIGHT)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(theme::GRAY),
        )
    } else {
        (
            Style::default().fg(theme::GRAY),
            Style::default().fg(theme::DIM),
        )
    };
    let mark = match (rank, unread) {
        (Some(n), _) => Span::styled(
            format!("{n:>2} "),
            Style::default()
                .fg(if unread { theme::ACCENT } else { theme::DIM })
                .add_modifier(Modifier::BOLD),
        ),
        (None, true) => Span::styled(" ● ", Style::default().fg(theme::ACCENT)),
        (None, false) => Span::raw("   "),
    };

    let label = if s.sources > 1 {
        format!("{} outlets", s.sources)
    } else {
        super::truncate(&head.source_name(), LABEL_W)
    };
    let meta = format!(
        "  {}{label}  {:>7}",
        " ".repeat(LABEL_W.saturating_sub(label.width())),
        super::time_ago(s.newest)
    );
    let title_w = width.saturating_sub(INDENT + meta.width());
    let max_lines = if app.compact { 1 } else { 2 };
    let mut parts = super::wrap_words(&head.title, title_w, max_lines).into_iter();

    let first = parts.next().unwrap_or_default();
    let pad = " ".repeat(title_w.saturating_sub(first.width()));
    let mut lines = vec![Line::from(vec![
        Span::raw("  "),
        mark,
        Span::styled(first + &pad, title_style),
        Span::styled(meta, meta_style),
    ])];
    for part in parts {
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(INDENT)),
            Span::styled(part, title_style),
        ]));
    }
    if !app.compact {
        lines.push(Line::raw(""));
    }
    ListItem::new(Text::from(lines))
}

/// An open story: its header, then its articles.
fn story_items(app: &App, story: usize, width: usize) -> (Vec<ListItem<'static>>, usize) {
    let mut items = vec![
        ListItem::new(Text::from(story_header(app, story, width))),
        ListItem::new(super::list::separator_line("Articles", width)),
    ];
    let mut selected_item = 0;
    for (row, &i) in app.filtered.iter().enumerate() {
        if row == app.selected {
            selected_item = items.len();
        }
        items.push(ListItem::new(article_line(&app.articles[i], width)));
    }
    (items, selected_item)
}

/// The wide layout's right side while browsing the Briefing: the selected
/// story as it would look opened, so j/k alone show what each one is about.
pub fn draw_card(f: &mut Frame, app: &App, area: Rect) {
    let Some(&story) = app.briefing_rows.get(app.selected) else {
        return;
    };
    let area = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(3),
        ..area
    };
    let width = area.width as usize;
    let mut members = app.stories[story].members.clone();
    members.sort_by_key(|&m| std::cmp::Reverse(app.articles[m].published));

    let mut lines = story_header(app, story, width);
    lines.push(super::list::separator_line("Articles", width));
    lines.extend(
        members
            .iter()
            .map(|&m| article_line(&app.articles[m], width)),
    );
    f.render_widget(Paragraph::new(lines), area);
}

/// A story's headline, outlets and summary.
fn story_header(app: &App, story: usize, width: usize) -> Vec<Line<'static>> {
    let s = &app.stories[story];
    let head = &app.articles[s.head];
    let text_w = width.saturating_sub(2).min(TEXT_W);
    let line = |text: String, style| Line::from(vec![Span::raw("  "), Span::styled(text, style)]);

    let mut lines = vec![Line::raw("")];
    let title_style = Style::default()
        .fg(theme::FG_BRIGHT)
        .add_modifier(Modifier::BOLD);
    for part in super::wrap_words(&head.title, text_w, 3) {
        lines.push(line(part, title_style));
    }
    let outlets = if s.sources > 1 {
        format!("{} outlets", s.sources)
    } else {
        head.source_name()
    };
    let meta = format!("{outlets} · updated {}", super::time_ago(s.newest));
    lines.push(line(meta, Style::default().fg(theme::GRAY)));
    if !head.summary.is_empty() {
        lines.push(Line::raw(""));
        for part in super::wrap_words(&head.summary, text_w, 4) {
            lines.push(line(part, Style::default().fg(theme::FG)));
        }
    }
    lines.push(Line::raw(""));
    lines
}

/// One article of an open story: "● Outlet   Title   3h ago".
fn article_line(article: &ArticleRow, width: usize) -> Line<'static> {
    let (dot, title_style) = if article.unread {
        (
            Span::styled("● ", Style::default().fg(theme::ACCENT)),
            Style::default().fg(theme::FG_BRIGHT),
        )
    } else {
        (Span::raw("  "), Style::default().fg(theme::GRAY))
    };
    let source = super::truncate(&article.source_name(), SOURCE_W);
    let pad = " ".repeat(SOURCE_W.saturating_sub(source.width()) + 1);
    let ago = format!("  {:>7}", super::time_ago(article.published));
    let title_w = width.saturating_sub(4 + SOURCE_W + 1 + ago.width());
    let title = super::truncate(&article.title, title_w);
    let title_pad = " ".repeat(title_w.saturating_sub(title.width()));
    Line::from(vec![
        Span::raw("  "),
        dot,
        Span::styled(source + &pad, Style::default().fg(theme::GRAY)),
        Span::styled(title + &title_pad, title_style),
        Span::styled(ago, Style::default().fg(theme::DIM)),
    ])
}
