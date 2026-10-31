//! The Briefing tab: stories grouped by category, best first.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::ListItem;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, BriefingRow};

/// Columns before a story's text: marker and space.
const INDENT: usize = 4;
/// The selected story's summary wraps at this width at most, so it reads as
/// a short paragraph rather than a block running to the pane's edge.
const SUMMARY_W: usize = 72;
/// Source column of an expanded story's articles.
const SOURCE_W: usize = 18;

/// List items for the Briefing, with a heading before each category, and
/// the index of the selected item among them.
pub fn items(app: &App, width: usize) -> (Vec<ListItem<'static>>, usize) {
    let mut items = Vec::new();
    let mut selected_item = 0;
    let mut category: Option<&str> = None;
    let mut in_finished = false;
    for (row_index, row) in app.briefing_rows.iter().enumerate() {
        let row_category = match row {
            BriefingRow::Story(s) | BriefingRow::Member(s) => app.stories[*s].category.as_str(),
            BriefingRow::More(c, _) => c.as_str(),
        };
        // Stories with nothing after `briefing_since` belong to the
        // finished Briefing, which follows under its own divider.
        let finished = match row {
            BriefingRow::Story(s) | BriefingRow::Member(s) => {
                app.stories[*s].newest <= app.briefing_since
            }
            BriefingRow::More(..) => false,
        };
        if finished && !in_finished {
            items.push(finished_item(app.briefing_since, width));
            in_finished = true;
            category = None;
        }
        let first_in_category = category != Some(row_category);
        if first_in_category {
            items.push(super::list::separator_item(row_category, width));
            category = Some(row_category);
        }
        if row_index == app.selected {
            selected_item = items.len();
        }
        let article = app
            .filtered
            .get(row_index)
            .and_then(|&i| app.articles.get(i));
        items.push(match (row, article) {
            (BriefingRow::Story(s), _) => story_item(
                app,
                *s,
                width,
                !first_in_category,
                row_index == app.selected,
            ),
            (BriefingRow::Member(_), Some(article)) => member_item(article, width),
            (BriefingRow::More(c, n), _) => more_item(c, *n),
            (BriefingRow::Member(_), None) => ListItem::new(""),
        });
    }
    (items, selected_item)
}

/// A story: "● Headline" (two lines at most) and "8 outlets · BBC World,
/// Al Jazeera, … · 2h ago". The selected story also shows its summary.
fn story_item(
    app: &App,
    story: usize,
    width: usize,
    spaced: bool,
    selected: bool,
) -> ListItem<'static> {
    let s = &app.stories[story];
    let head = &app.articles[s.head];
    let unread = s.members.iter().any(|&m| app.articles[m].unread);
    let (dot, title_style, count_style) = if unread {
        (
            Span::styled("  ● ", Style::default().fg(theme::ACCENT)),
            Style::default()
                .fg(theme::FG_BRIGHT)
                .add_modifier(Modifier::BOLD),
            Style::default().fg(theme::FG),
        )
    } else {
        (
            Span::raw(" ".repeat(INDENT)),
            Style::default().fg(theme::GRAY),
            Style::default().fg(theme::GRAY),
        )
    };
    let text_width = width.saturating_sub(INDENT);
    let indent = || Span::raw(" ".repeat(INDENT));

    let mut lines = Vec::new();
    if spaced {
        lines.push(Line::raw(""));
    }
    let mut lead = Some(dot);
    for part in super::wrap_words(&head.title, text_width, 2) {
        let first = lead.take().unwrap_or_else(indent);
        lines.push(Line::from(vec![first, Span::styled(part, title_style)]));
    }

    let outlets: Vec<String> = app
        .coverage
        .get(&head.link)
        .cloned()
        .unwrap_or_else(|| vec![head.source_name()]);
    let ago = super::time_ago(s.newest);
    let (count, names) = match outlets.len() {
        1 => (String::new(), format!("{} · {ago}", outlets[0])),
        n => {
            let more = if n > 2 { ", …" } else { "" };
            (
                format!("{n} outlets"),
                format!(" · {}{more} · {ago}", outlets[..2].join(", ")),
            )
        }
    };
    let names = super::truncate(&names, text_width.saturating_sub(count.width()));
    lines.push(Line::from(vec![
        indent(),
        Span::styled(count, count_style),
        Span::styled(names, Style::default().fg(theme::GRAY)),
    ]));

    if selected && !app.expanded.contains(&story) && !head.summary.is_empty() {
        for part in super::wrap_words(&head.summary, text_width.min(SUMMARY_W), 2) {
            lines.push(Line::from(vec![
                indent(),
                Span::styled(part, Style::default().fg(theme::DIM)),
            ]));
        }
    }
    ListItem::new(Text::from(lines))
}

/// One article of an expanded story: "● Outlet   Title   3h ago".
fn member_item(article: &crate::store::ArticleRow, width: usize) -> ListItem<'static> {
    let (dot, title_style) = if article.unread {
        (
            Span::styled("● ", Style::default().fg(theme::ACCENT)),
            Style::default().fg(theme::FG),
        )
    } else {
        (Span::raw("  "), Style::default().fg(theme::GRAY))
    };
    let source = super::truncate(&article.source_name(), SOURCE_W);
    let pad = " ".repeat(SOURCE_W.saturating_sub(source.width()) + 1);
    let ago = format!("  {}", super::time_ago(article.published));
    let lead = INDENT + 2;
    let title_w = width.saturating_sub(lead + 2 + SOURCE_W + 1 + ago.width());
    ListItem::new(Line::from(vec![
        Span::raw(" ".repeat(lead)),
        dot,
        Span::styled(source + &pad, Style::default().fg(theme::GRAY)),
        Span::styled(super::truncate(&article.title, title_w), title_style),
        Span::styled(ago, Style::default().fg(theme::DIM)),
    ]))
}

fn more_item(category: &str, count: usize) -> ListItem<'static> {
    let stories = if count == 1 { "story" } else { "stories" };
    ListItem::new(vec![
        Line::raw(""),
        Line::styled(
            format!("    + {count} more {stories} in {category}"),
            Style::default().fg(theme::GRAY),
        ),
    ])
}

/// The divider above the finished Briefing.
fn finished_item(done: i64, width: usize) -> ListItem<'static> {
    let label = format!(
        " Finished {} · already read ",
        super::list::since_label(done)
    );
    let rule = "─".repeat(width.saturating_sub(label.width() + 2));
    ListItem::new(vec![
        Line::raw(""),
        Line::styled(format!("─{label}{rule}"), Style::default().fg(theme::DIM)),
    ])
}
