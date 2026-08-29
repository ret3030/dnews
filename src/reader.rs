use anyhow::Result;
use ego_tree::NodeRef;
use readability_rust::Readability;
use scraper::{Html, Node};

pub struct ReaderContent {
    pub text: String,
}

pub async fn fetch_article(url: &str) -> Result<ReaderContent> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (dnews reader)")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let html = client.get(url).send().await?.text().await?;

    let html_owned = html;
    let url_owned = url.to_string();
    tokio::task::spawn_blocking(move || extract(&html_owned, &url_owned)).await?
}

fn extract(html: &str, url: &str) -> Result<ReaderContent> {
    let mut parser = Readability::new_with_base_uri(html, url, None)
        .map_err(|e| anyhow::anyhow!("readability init failed: {e:?}"))?;
    let article = parser
        .parse()
        .ok_or_else(|| anyhow::anyhow!("no readable content found"))?;

    // Readability's own `title` extraction can land on the wrong DOM node
    // (e.g. a whitespace-formatted site-logo block) and there's no reliable
    // way to detect that from the string alone. We already have a clean
    // title from the feed itself (shown in the reader header via the DB
    // row), so don't bother extracting or trusting one here at all.
    let content_html = article.content.unwrap_or_default();
    let text = html_to_text(&content_html);

    if text.trim().is_empty() {
        anyhow::bail!("empty article body");
    }

    Ok(ReaderContent { text })
}

/// Converts extracted article HTML to plain text, keeping block-level
/// paragraph breaks (readability's `text_content` alone collapses everything
/// into one run-on block) and replacing `<img>` with a `[image: alt]`
/// placeholder so the reader at least knows an image was there — actual
/// inline image rendering isn't supported in a plain terminal buffer.
fn html_to_text(html: &str) -> String {
    // Sites occasionally emit bare <tr>/<td> rows with no <table> ancestor
    // (a "table" built via CSS grid on a non-table element rather than real
    // markup). Per the HTML5 tree-construction algorithm, a <tr> start tag
    // outside proper table-insertion-mode context is a parse error and gets
    // dropped outright (its *contents* still land as loose text), which is
    // why those rows would otherwise come out as one run-on paragraph with
    // no row/cell structure at all. Wrapping in a synthetic <table> fixes
    // the parse context; only done when nothing already provides one.
    let wrapped;
    let html = if html.contains("<tr") && !html.to_lowercase().contains("<table") {
        wrapped = format!("<table><tbody>{html}</tbody></table>");
        &wrapped
    } else {
        html
    };

    let doc = Html::parse_fragment(html);
    let mut out = String::new();
    walk(doc.tree.root(), &mut out);
    normalize_blank_lines(&out)
}

fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "li"
            | "blockquote"
            | "pre"
            | "table"
            | "ul"
            | "ol"
            | "figure"
    )
}

fn walk(node: NodeRef<Node>, out: &mut String) {
    match node.value() {
        Node::Text(t) => out.push_str(t),
        Node::Element(el) => {
            let tag = el.name();
            match tag {
                "script" | "style" | "svg" | "iframe" | "form" | "button" => return,
                "noscript" => {
                    // Lazy-loaded images are commonly stashed here as a
                    // fallback <img>. HTML5's tokenizer treats <noscript> as
                    // RAWTEXT (scripting assumed enabled), so its content
                    // arrives as one literal, *un-decoded* text node — e.g.
                    // `&lt;img src="x.jpg" alt="..." /&gt;`, entities and
                    // all. Decode it and, if it looks like markup, re-parse
                    // and walk that instead of dropping or leaking it as text.
                    let mut inner = String::new();
                    collect_raw_text(node, &mut inner);
                    let decoded = decode_entities(inner.trim());
                    if decoded.starts_with('<') {
                        let frag = Html::parse_fragment(&decoded);
                        walk(frag.tree.root(), out);
                    }
                    return;
                }
                "br" => {
                    out.push('\n');
                    return;
                }
                "img" => {
                    let alt = el
                        .attr("alt")
                        .map(str::trim)
                        .filter(|a| !a.is_empty())
                        .unwrap_or("image");
                    out.push_str("\n\n[image: ");
                    out.push_str(alt);
                    out.push_str("]\n\n");
                    return;
                }
                // Table rows read better as one tight line per row (cells
                // separated by a middle dot) than as fully blank-line-spaced
                // paragraphs — this isn't real column alignment (that needs
                // a two-pass width computation this walker doesn't do), but
                // it keeps each row scannable on its own line.
                "tr" => {
                    // Same whitespace-sibling-text-node issue as the trailing
                    // separator below, but between rows: the newline/indent
                    // between `</tr>` and the next `<tr>` is a text-node
                    // sibling, so a plain `ends_with('\n')` check would leave
                    // it in place and produce a blank line between rows.
                    while out.ends_with(char::is_whitespace) {
                        out.pop();
                    }
                    if !out.is_empty() {
                        out.push('\n');
                    }
                    let mut child = node.first_child();
                    while let Some(c) = child {
                        walk(c, out);
                        child = c.next_sibling();
                    }
                    // Trim trailing whitespace — not just a single space —
                    // before checking for the separator: the whitespace
                    // *between* tags in the source (e.g. the newline/indent
                    // between `</td>` and `</tr>`) is itself a text node and
                    // a sibling of the cells, so it lands in `out` *after*
                    // the last cell's " · ", which would otherwise dodge an
                    // exact `" · "` suffix match.
                    while out.ends_with(char::is_whitespace) {
                        out.pop();
                    }
                    if out.ends_with('·') {
                        out.pop();
                        while out.ends_with(char::is_whitespace) {
                            out.pop();
                        }
                    }
                    out.push('\n');
                    return;
                }
                "td" | "th" => {
                    let mut child = node.first_child();
                    while let Some(c) = child {
                        walk(c, out);
                        child = c.next_sibling();
                    }
                    while out.ends_with(char::is_whitespace) {
                        out.pop();
                    }
                    out.push_str(" · ");
                    return;
                }
                _ => {}
            }

            let block = is_block(tag);
            if block {
                out.push_str("\n\n");
            }
            if tag == "li" {
                out.push_str("• ");
            }

            let mut child = node.first_child();
            while let Some(c) = child {
                walk(c, out);
                child = c.next_sibling();
            }

            if block {
                out.push_str("\n\n");
            } else if !tag.is_empty() {
                out.push(' ');
            }
        }
        _ => {
            let mut child = node.first_child();
            while let Some(c) = child {
                walk(c, out);
                child = c.next_sibling();
            }
        }
    }
}

/// Concatenates every text node under `node`, ignoring element structure —
/// used to pull the raw RAWTEXT string out of a `<noscript>`.
fn collect_raw_text(node: NodeRef<Node>, out: &mut String) {
    if let Node::Text(t) = node.value() {
        out.push_str(t);
    }
    let mut child = node.first_child();
    while let Some(c) = child {
        collect_raw_text(c, out);
        child = c.next_sibling();
    }
}

/// Decodes the handful of HTML character references that show up in
/// escaped-markup fallbacks (RAWTEXT content isn't decoded by the tokenizer).
/// Not a general-purpose entity decoder — just enough to recover a `<img>`
/// tag's angle brackets and quotes.
fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

/// Collapses runs of whitespace within lines and caps consecutive blank
/// lines at one, so paragraph breaks stay crisp instead of ragged.
fn normalize_blank_lines(text: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
        lines.push(collapsed);
    }

    let mut out = Vec::with_capacity(lines.len());
    let mut blank_run = false;
    for line in lines {
        if line.is_empty() {
            if !blank_run && !out.is_empty() {
                out.push(String::new());
            }
            blank_run = true;
        } else {
            out.push(line);
            blank_run = false;
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_paragraph_breaks_and_flags_images() {
        let html = "<p>First paragraph.</p><p>Second paragraph.</p><img src=\"x.jpg\" alt=\"a cat\"><p>Third.</p>";
        let text = html_to_text(html);
        let paras: Vec<&str> = text.split("\n\n").collect();
        assert_eq!(paras.len(), 4, "expected 4 blocks, got: {text:?}");
        assert_eq!(paras[0], "First paragraph.");
        assert_eq!(paras[1], "Second paragraph.");
        assert_eq!(paras[2], "[image: a cat]");
        assert_eq!(paras[3], "Third.");
    }

    #[test]
    fn unwraps_lazy_loaded_image_stashed_in_noscript() {
        // Mirrors the real markup irozhlas.cz (and similar Drupal-based sites)
        // emit for lazy-loaded images: the actual <img> only exists inside a
        // <noscript>, HTML-escaped, since html5ever parses <noscript> content
        // as raw text rather than real child elements.
        let html = concat!(
            r#"<p>Before.</p>"#,
            r#"<span class="img__holder"><noscript>"#,
            r#"&lt;img src="/x.png" alt="A chart" /&gt;"#,
            r#"</noscript></span>"#,
            r#"<p>After.</p>"#,
        );
        let text = html_to_text(html);
        assert!(
            text.contains("[image: A chart]"),
            "expected the noscript-wrapped image to surface as a placeholder, got: {text:?}"
        );
        assert!(
            !text.contains("<img"),
            "raw escaped markup leaked into the output: {text:?}"
        );
    }

    #[test]
    fn renders_orphaned_table_rows_one_per_line_without_trailing_separator() {
        // Real markup (irozhlas.cz's live score widget): bare <tr>/<td> with
        // no <table> ancestor, and — critically — whitespace *between* tags
        // as its own sibling text node, including right before </tr>. Both
        // are what previously caused (a) rows collapsing into one run-on
        // paragraph and (b) a dangling " ·" surviving at each line's end.
        let html = "\n<tr> <td>25. 7.</td> <td>17:00</td> <td>Zlín – Baník</td> <td>0:1</td> </tr>\n<tr> <td>26. 7.</td> <td>15:00</td> <td>Slavia – Slovácko</td> <td>5:1</td> </tr>\n";
        let text = html_to_text(html);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2, "expected one line per row, got: {text:?}");
        assert_eq!(lines[0], "25. 7. · 17:00 · Zlín – Baník · 0:1");
        assert_eq!(lines[1], "26. 7. · 15:00 · Slavia – Slovácko · 5:1");
    }

    #[tokio::test]
    #[ignore = "hits the real network"]
    async fn extracts_a_real_article() {
        let content = fetch_article("https://en.wikipedia.org/wiki/Rust_(programming_language)")
            .await
            .expect("fetch_article failed");
        assert!(content.text.len() > 500, "article body looked too short");
        assert!(
            content.text.contains("\n\n"),
            "expected paragraph breaks in real article"
        );
    }

    #[tokio::test]
    #[ignore = "hits the real network"]
    async fn extracts_noscript_lazy_image_from_a_real_article() {
        // Regression check for the irozhlas.cz-style lazy-loaded <noscript>
        // image case — see `unwraps_lazy_loaded_image_stashed_in_noscript`
        // for the offline/synthetic version of this same bug.
        let content = fetch_article(
            "https://www.irozhlas.cz/zpravy-domov/pripomina-zavod-a-je-i-pro-deti-v-bechovicich-maji-nove-namesti-drive-na-miste_2608291634_vsn",
        )
        .await
        .expect("fetch_article failed");
        assert!(
            content.text.contains("[image:"),
            "expected at least one image placeholder, got: {}",
            content.text
        );
        assert!(
            !content.text.contains("&lt;img") && !content.text.contains("<img "),
            "raw/escaped <img> markup leaked into the output"
        );
    }
}
