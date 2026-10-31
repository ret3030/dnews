//! Lays out a `ReaderContent` as styled, word-wrapped lines.
//!
//! Wrapping is done here rather than by ratatui's `Paragraph` so list items
//! and quotes get a hanging indent, and so the line count (needed to stop
//! scrolling at the end) is simply `lines.len()`.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

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
            Block::Para(runs) => lines.extend(wrap(styled(runs, body), width, vec![], vec![])),
            Block::Heading(runs) => {
                let style = Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD);
                lines.extend(wrap(styled(runs, style), width, vec![], vec![]));
            }
            Block::Quote(runs) => {
                let style = Style::default()
                    .fg(theme::GRAY)
                    .add_modifier(Modifier::ITALIC);
                let bar = || vec![Span::styled("│ ", Style::default().fg(theme::BORDER))];
                lines.extend(wrap(styled(runs, style), width, bar(), bar()));
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
                lines.extend(wrap(styled(content, body), width, first, rest));
            }
            Block::Image(alt) => {
                let alt = if alt.is_empty() { "image" } else { alt };
                let style = Style::default()
                    .fg(theme::MAUVE)
                    .add_modifier(Modifier::ITALIC);
                lines.extend(wrap(
                    vec![(alt.to_string(), style)],
                    width,
                    vec![Span::styled("▣ ", style)],
                    vec![Span::raw("  ")],
                ));
            }
            Block::Rows(rows) => {
                let style = Style::default().fg(theme::GRAY);
                for row in rows {
                    lines.extend(wrap(
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

enum Token {
    /// Text with no break opportunity inside, possibly spanning styles
    /// ("report" + "[1]" + ",").
    Word(Vec<(String, Style)>),
    Space(Style),
    Break,
}

fn tokenize(runs: Vec<(String, Style)>) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut word: Vec<(String, Style)> = Vec::new();
    let finish = |word: &mut Vec<(String, Style)>, tokens: &mut Vec<Token>| {
        if !word.is_empty() {
            tokens.push(Token::Word(std::mem::take(word)));
        }
    };
    for (text, style) in runs {
        for c in text.chars() {
            if c == '\n' {
                finish(&mut word, &mut tokens);
                tokens.push(Token::Break);
            } else if c.is_whitespace() {
                finish(&mut word, &mut tokens);
                tokens.push(Token::Space(style));
            } else {
                match word.last_mut() {
                    Some((s, st)) if *st == style => s.push(c),
                    _ => word.push((c.to_string(), style)),
                }
            }
        }
    }
    finish(&mut word, &mut tokens);
    tokens
}

/// Word-wraps styled text to `width` columns. `first` prefixes the first
/// line, `rest` every following one (for hanging indents).
pub fn wrap(
    runs: Vec<(String, Style)>,
    width: usize,
    first: Vec<Span<'static>>,
    rest: Vec<Span<'static>>,
) -> Vec<Line<'static>> {
    let prefix_w = |p: &[Span]| p.iter().map(|s| s.content.width()).sum::<usize>();
    // Always leave room for some text, however deep the indent.
    let width = width.max(prefix_w(&first).max(prefix_w(&rest)) + 8);

    let mut lines = Vec::new();
    let mut cur: Vec<Span<'static>> = first;
    let mut cur_w = prefix_w(&cur);
    let mut has_text = false;
    let mut space: Option<Style> = None;

    let new_line = |lines: &mut Vec<Line<'static>>, cur: &mut Vec<Span<'static>>| {
        lines.push(Line::from(std::mem::replace(cur, rest.clone())));
        prefix_w(cur)
    };

    for token in tokenize(runs) {
        match token {
            Token::Space(style) => {
                if has_text {
                    space = Some(style);
                }
            }
            Token::Break => {
                cur_w = new_line(&mut lines, &mut cur);
                has_text = false;
                space = None;
            }
            Token::Word(segments) => {
                let word_w: usize = segments.iter().map(|(s, _)| s.width()).sum();
                let space_w = usize::from(space.is_some());
                if has_text && cur_w + space_w + word_w > width {
                    cur_w = new_line(&mut lines, &mut cur);
                    space = None;
                } else if let Some(style) = space.take() {
                    cur.push(Span::styled(" ", style));
                    cur_w += 1;
                }
                if cur_w + word_w <= width {
                    for (s, style) in segments {
                        cur.push(Span::styled(s, style));
                    }
                    cur_w += word_w;
                } else {
                    // Longer than a whole line: split it character by character.
                    for (s, style) in segments {
                        let mut piece = String::new();
                        for c in s.chars() {
                            let cw = c.width().unwrap_or(0);
                            if cur_w + cw > width {
                                cur.push(Span::styled(std::mem::take(&mut piece), style));
                                cur_w = new_line(&mut lines, &mut cur);
                            }
                            piece.push(c);
                            cur_w += cw;
                        }
                        cur.push(Span::styled(piece, style));
                    }
                }
                has_text = true;
            }
        }
    }
    if has_text || lines.is_empty() {
        lines.push(Line::from(cur));
    }
    lines
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
