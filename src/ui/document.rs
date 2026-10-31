//! Lays out a `ReaderContent` as styled, word-wrapped lines.
//!
//! Wrapping is done with `ui::wrap` rather than ratatui's `Paragraph` so
//! list items and quotes get a hanging indent, and so the line count (needed
//! to stop scrolling at the end) is simply `lines.len()`.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::theme;
use crate::reader::{Block, Inline, ReaderContent};

pub fn render(content: &ReaderContent, width: usize) -> Vec<Line<'static>> {
    let body = Style::default().fg(theme::FG);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut prev: Option<&Block> = None;

    for block in &content.blocks {
        // Blank line between blocks; list items stay tight.
        let tight = matches!(
            (prev, block),
            (Some(Block::Item { .. }), Block::Item { .. })
        );
        if prev.is_some() && !tight {
            lines.push(Line::raw(""));
        }
        prev = Some(block);

        match block {
            Block::Para(runs) => {
                lines.extend(super::wrap(styled(runs, body), width, vec![], vec![]))
            }
            Block::Heading(runs) => {
                let style = Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD);
                lines.extend(super::wrap(styled(runs, style), width, vec![], vec![]));
            }
            Block::Quote(runs) => {
                let style = Style::default()
                    .fg(theme::GRAY)
                    .add_modifier(Modifier::ITALIC);
                let bar = || vec![Span::styled("│ ", Style::default().fg(theme::BORDER))];
                lines.extend(super::wrap(styled(runs, style), width, bar(), bar()));
            }
            Block::Item {
                depth,
                marker,
                content,
            } => {
                let indent = " ".repeat(2 * (*depth as usize - 1));
                // Continuation paragraphs align under the first item's text.
                let marker_w = if marker.is_empty() {
                    2
                } else {
                    marker.width() + 1
                };
                let first = vec![
                    Span::raw(indent.clone()),
                    Span::styled(
                        format!("{marker:<marker_w$}"),
                        Style::default().fg(theme::ACCENT),
                    ),
                ];
                let rest = vec![Span::raw(format!("{indent}{}", " ".repeat(marker_w)))];
                lines.extend(super::wrap(styled(content, body), width, first, rest));
            }
            Block::Image(alt) => {
                let alt = if alt.is_empty() { "image" } else { alt };
                let style = Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::ITALIC);
                lines.extend(super::wrap(
                    vec![(alt.to_string(), style)],
                    width,
                    vec![Span::styled("▣ ", style)],
                    vec![Span::raw("  ")],
                ));
            }
            Block::Rows(rows) => {
                let style = Style::default().fg(theme::GRAY);
                for row in rows {
                    lines.extend(super::wrap(
                        vec![(row.clone(), style)],
                        width,
                        vec![],
                        vec![Span::raw("  ")],
                    ));
                }
            }
        }
    }

    if !content.links.is_empty() {
        let dim = Style::default().fg(theme::DIM);
        lines.push(Line::raw(""));
        lines.push(Line::styled("Links", dim.add_modifier(Modifier::BOLD)));
        for (i, url) in content.links.iter().enumerate() {
            let label = format!("[{}] ", i + 1);
            let url = super::truncate(url, width.saturating_sub(label.width()));
            lines.push(Line::styled(format!("{label}{url}"), dim));
        }
    }
    lines
}

/// Turns inline runs into (text, style) pairs on top of `base`.
fn styled(runs: &[Inline], base: Style) -> Vec<(String, Style)> {
    runs.iter()
        .map(|r| {
            let mut style = base;
            if r.note {
                style = Style::default().fg(theme::DIM);
            } else if r.link.is_some() {
                style = style.fg(theme::LINK).add_modifier(Modifier::UNDERLINED);
            }
            if r.bold {
                style = style.add_modifier(Modifier::BOLD);
                if r.link.is_none() && !r.note {
                    style = style.fg(theme::FG_BRIGHT);
                }
            }
            if r.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            (r.text.clone(), style)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::html_to_content;

    fn text_of(line: &Line) -> String {
        line.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn render_html(html: &str, width: usize) -> Vec<String> {
        render(&html_to_content(html), width)
            .iter()
            .map(text_of)
            .collect()
    }

    #[test]
    fn lines_never_exceed_the_width() {
        let html = "<p>Vláda schválila státní rozpočet na příští rok, opozice kritizuje schodek. \
                    A verylongwordwithoutanyspacesthatmustbesplit ends it.</p>";
        for line in render_html(html, 20) {
            assert!(line.width() <= 20, "{line:?}");
        }
    }

    #[test]
    fn list_items_get_a_hanging_indent() {
        let lines = render_html(
            "<ul><li>First item that is long enough to wrap onto a second line</li><li>Second</li></ul>",
            24,
        );
        assert!(lines[0].starts_with("• First"), "{lines:?}");
        assert!(
            lines[1].starts_with("  "),
            "wrapped text aligns under the item: {lines:?}"
        );
        assert!(lines.iter().any(|l| l.starts_with("• Second")));
        assert!(
            !lines.iter().any(String::is_empty),
            "items stay tight: {lines:?}"
        );
    }

    #[test]
    fn quotes_keep_their_bar_on_every_line() {
        let lines = render_html(
            "<blockquote>A quoted sentence long enough to need wrapping.</blockquote>",
            20,
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| l.starts_with("│ ")), "{lines:?}");
    }

    #[test]
    fn link_marker_never_wraps_away_from_its_word() {
        let lines = render_html(
            "<p>aaaa bbbb <a href=\"https://x.com\">cccc</a>, dddd</p>",
            12,
        );
        assert!(lines.iter().any(|l| l.contains("cccc[1],")), "{lines:?}");
        assert!(
            lines
                .iter()
                .any(|l| l.starts_with("[1] https:") && l.width() <= 12),
            "{lines:?}"
        );
    }

    #[test]
    fn br_breaks_and_blocks_are_spaced() {
        let lines = render_html("<h2>Title</h2><p>one<br>two</p>", 40);
        assert_eq!(lines, ["Title", "", "one", "two"]);
    }
}
