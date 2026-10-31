//! The Briefing tab: stories grouped by category, best first.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::ListItem;
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::app::{App, BriefingRow};

/// Columns before a story's text: marker and space.
const INDENT: usize = 4;
/// Source column of an expanded story's articles.
const SOURCE_W: usize = 18;

/// List items for the Briefing, with a heading before each category, and
/// the index of the selected item among them.
pub fn items(app: &App, width: usize) -> (Vec<ListItem<'static>>, usize) {
    let mut items = Vec::new();
    let mut selected_item = 0;
    let mut category: Option<&str> = None;
    for (row_index, row) in app.briefing_rows.iter().enumerate() {
        let row_category = match row {
            BriefingRow::Story(s) | BriefingRow::Member(s) => app.stories[*s].category.as_str(),
            BriefingRow::More(c, _) => c.as_str(),
        };
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
            (BriefingRow::Story(s), _) => story_item(app, *s, width, !first_in_category),
            (BriefingRow::Member(_), Some(article)) => member_item(article, width),
            (BriefingRow::More(c, n), _) => more_item(c, *n),
            (BriefingRow::Member(_), None) => ListItem::new(""),
        });
    }
    (items, selected_item)
}

/// "▸ Headline" (two lines at most), the outlets and age, and a line of the
/// headline article's summary.
fn story_item(app: &App, story: usize, width: usize, spaced: bool) -> ListItem<'static> {
    let s = &app.stories[story];
    let head = &app.articles[s.head];
    let unread = s.members.iter().any(|&m| app.articles[m].unread);
    let expanded = app.expanded.contains(&story);

    let marker_style = Style::default().fg(if unread { theme::ACCENT } else { theme::DIM });
    let title_style = if unread {
        Style::default()
            .fg(theme::FG_BRIGHT)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::GRAY)
    };
    let text_width = width.saturating_sub(INDENT);
    let indent = || Span::raw(" ".repeat(INDENT));

    let mut lines = Vec::new();
    if spaced {
        lines.push(Line::raw(""));
    }
    for (i, part) in super::wrap_words(&head.title, text_width, 2)
        .into_iter()
        .enumerate()
    {
        let lead = if i > 0 {
            indent()
        } else {
            Span::styled(if expanded { "  ▾ " } else { "  ▸ " }, marker_style)
        };
        lines.push(Line::from(vec![lead, Span::styled(part, title_style)]));
    }

    let own = [head.source_name()];
    let outlets: Vec<&str> = match app.coverage.get(&head.link) {
        Some(all) => all.iter().map(String::as_str).collect(),
        None => own.iter().map(String::as_str).collect(),
    };
    let mut meta = super::outlets_label(&outlets);
    let newest = app.articles[s.members[0]].published;
    meta.push_str(&format!(" · {}", super::time_ago(newest)));
    lines.push(Line::from(vec![
        indent(),
        Span::styled(
            super::truncate(&meta, text_width),
            Style::default().fg(theme::GRAY),
        ),
    ]));

    if !head.summary.is_empty() && !expanded {
        lines.push(Line::from(vec![
            indent(),
            Span::styled(
                super::truncate(&head.summary, text_width),
                Style::default().fg(theme::DIM),
            ),
        ]));
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
            format!("    + {count} more {stories} in {category}  ↵"),
            Style::default().fg(theme::GRAY),
        ),
    ])
}
