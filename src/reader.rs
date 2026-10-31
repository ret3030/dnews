//! Fetches an article, runs Readability on it and turns the result into a
//! small document model (`Block`s of styled `Inline` runs) for the reader.

use anyhow::Result;
use ego_tree::NodeRef;
use readability_rust::Readability;
use scraper::{Html, Node};
use serde::{Deserialize, Serialize};

/// Bumped whenever the extracted format changes, so stale cache entries are
/// re-fetched instead of misread.
const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ReaderContent {
    pub version: u32,
    pub blocks: Vec<Block>,
    /// Link targets; `Inline::link` and the `[n]` markers are 1-based indexes.
    pub links: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Block {
    Heading(Vec<Inline>),
    Para(Vec<Inline>),
    Quote(Vec<Inline>),
    /// A list item. `marker` is "•", "3." or "" for a continuation paragraph.
    Item {
        depth: u8,
        marker: String,
        content: Vec<Inline>,
    },
    Image(String),
    /// Pre-formatted lines (table rows, code): shown one per line, no reflow.
    Rows(Vec<String>),
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Inline {
    pub text: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<usize>,
    /// The dim `[n]` marker after a link.
    #[serde(default, skip_serializing_if = "is_false")]
    pub note: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl ReaderContent {
    /// Parses a cache entry; `None` for an older format or a garbled entry,
    /// so the article is fetched again.
    pub fn from_cache(s: &str) -> Option<Self> {
        if !looks_like_text(s) {
            return None;
        }
        serde_json::from_str::<Self>(s)
            .ok()
            .filter(|c| c.version == FORMAT_VERSION)
    }

    pub fn to_cache(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn word_count(&self) -> usize {
        let words = |inl: &[Inline]| -> usize {
            inl.iter()
                .filter(|i| !i.note)
                .map(|i| i.text.split_whitespace().count())
                .sum()
        };
        self.blocks
            .iter()
            .map(|b| match b {
                Block::Heading(c) | Block::Para(c) | Block::Quote(c) => words(c),
                Block::Item { content, .. } => words(content),
                Block::Image(_) => 0,
                Block::Rows(rows) => rows.iter().map(|r| r.split_whitespace().count()).sum(),
            })
            .sum()
    }

    /// Estimated reading time at ~200 words per minute.
    pub fn reading_minutes(&self) -> usize {
        self.word_count().div_ceil(200).max(1)
    }
}

#[cfg(test)]
impl ReaderContent {
    /// Plain paragraphs, split on blank lines (test fixtures).
    pub fn from_plain(text: &str) -> Self {
        let blocks = text
            .split("\n\n")
            .map(|p| {
                Block::Para(vec![Inline {
                    text: p.to_string(),
                    ..Default::default()
                }])
            })
            .collect();
        Self {
            version: FORMAT_VERSION,
            blocks,
            links: Vec::new(),
        }
    }

    pub fn plain_text(&self) -> String {
        self.blocks
            .iter()
            .filter_map(|b| match b {
                Block::Para(r) | Block::Heading(r) | Block::Quote(r) => {
                    Some(r.iter().map(|i| i.text.as_str()).collect::<String>())
                }
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

pub async fn fetch_article(url: &str) -> Result<ReaderContent> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (dnews reader)")
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let html = client
        .get(url)
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    if !looks_like_text(&html) {
        anyhow::bail!("the server sent unreadable data");
    }
    let url = url.to_string();
    tokio::task::spawn_blocking(move || extract(&html, &url)).await?
}

/// False for binary data decoded as text (e.g. a compressed body that was
/// never decompressed): real text has almost no replacement or control
/// characters.
fn looks_like_text(s: &str) -> bool {
    let total = s.chars().count();
    let bad = s
        .chars()
        .filter(|&c| c == char::REPLACEMENT_CHARACTER || (c.is_control() && !c.is_whitespace()))
        .count();
    bad * 50 <= total
}

fn extract(html: &str, url: &str) -> Result<ReaderContent> {
    let mut parser = Readability::new_with_base_uri(html, url, None)
        .map_err(|e| anyhow::anyhow!("readability init failed: {e:?}"))?;
    let article = parser
        .parse()
        .ok_or_else(|| anyhow::anyhow!("no readable content found"))?;

    // Readability's own title is unreliable; the reader header uses the
    // feed's title instead.
    let content = html_to_content(&article.content.unwrap_or_default());
    if content.blocks.is_empty() {
        anyhow::bail!("empty article body");
    }
    Ok(content)
}

pub fn html_to_content(html: &str) -> ReaderContent {
    // Bare <tr> rows without a <table> are dropped by the HTML5 parser, so
    // give them one (some sites build tables out of loose rows).
    let wrapped;
    let html = if html.contains("<tr") && !html.to_lowercase().contains("<table") {
        wrapped = format!("<table><tbody>{html}</tbody></table>");
        &wrapped
    } else {
        html
    };

    let doc = Html::parse_fragment(html);
    let mut w = Walker::default();
    w.walk(doc.tree.root());
    w.flush();
    ReaderContent {
        version: FORMAT_VERSION,
        blocks: w.blocks,
        links: w.links,
    }
}

#[derive(Default)]
struct Walker {
    blocks: Vec<Block>,
    links: Vec<String>,
    /// Inline runs of the block being built.
    cur: Vec<Inline>,
    bold: u32,
    italic: u32,
    link: Option<usize>,
    heading: bool,
    quote_depth: u32,
    /// One entry per open list: `Some(next number)` for <ol>, `None` for <ul>.
    lists: Vec<Option<u32>>,
    /// Marker for the next flushed block inside a list item; becomes "" after
    /// the item's first block so further paragraphs indent under it.
    item_marker: Option<String>,
}

impl Walker {
    fn walk(&mut self, node: NodeRef<Node>) {
        match node.value() {
            Node::Text(t) => self.push_text(t),
            Node::Element(el) => {
                let tag = el.name();
                match tag {
                    "script" | "style" | "svg" | "iframe" | "form" | "button" | "nav" | "aside" => {
                    }
                    "noscript" => self.noscript(node),
                    "br" => self.push_text_raw("\n"),
                    "img" => {
                        let alt = el.attr("alt").map(str::trim).unwrap_or_default();
                        self.flush();
                        self.blocks.push(Block::Image(alt.to_string()));
                    }
                    "tr" => self.table_row(node),
                    "pre" => {
                        self.flush();
                        let mut raw = String::new();
                        collect_raw_text(node, &mut raw);
                        let rows: Vec<String> =
                            raw.trim_matches('\n').lines().map(str::to_string).collect();
                        self.push_rows(rows);
                    }
                    "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        self.flush();
                        self.heading = true;
                        self.children(node);
                        self.flush();
                        self.heading = false;
                    }
                    "blockquote" => {
                        self.flush();
                        self.quote_depth += 1;
                        self.children(node);
                        self.flush();
                        self.quote_depth -= 1;
                    }
                    "ul" | "ol" => {
                        self.flush();
                        let start = el.attr("start").and_then(|s| s.parse().ok()).unwrap_or(1);
                        self.lists.push((tag == "ol").then_some(start));
                        let outer_marker = self.item_marker.take();
                        self.children(node);
                        self.flush();
                        self.lists.pop();
                        self.item_marker = outer_marker.map(|_| String::new());
                    }
                    "li" => {
                        self.flush();
                        let marker = match self.lists.last_mut() {
                            Some(Some(n)) => {
                                *n += 1;
                                format!("{}.", *n - 1)
                            }
                            _ => "•".to_string(),
                        };
                        let outer = self.item_marker.replace(marker);
                        self.children(node);
                        self.flush();
                        self.item_marker = outer;
                    }
                    "strong" | "b" => {
                        self.bold += 1;
                        self.children(node);
                        self.bold -= 1;
                    }
                    "em" | "i" | "cite" => {
                        self.italic += 1;
                        self.children(node);
                        self.italic -= 1;
                    }
                    "a" => self.anchor(node, el.attr("href")),
                    _ if is_block(tag) => {
                        self.flush();
                        self.children(node);
                        self.flush();
                    }
                    _ => self.children(node),
                }
            }
            _ => self.children(node),
        }
    }

    fn children(&mut self, node: NodeRef<Node>) {
        let mut child = node.first_child();
        while let Some(c) = child {
            self.walk(c);
            child = c.next_sibling();
        }
    }

    /// Lazy-loaded images are often stashed in <noscript> as escaped markup
    /// (the parser treats its content as raw text). Re-parse that markup.
    fn noscript(&mut self, node: NodeRef<Node>) {
        let mut inner = String::new();
        collect_raw_text(node, &mut inner);
        let decoded = decode_entities(inner.trim());
        if decoded.starts_with('<') {
            let frag = Html::parse_fragment(&decoded);
            self.walk(frag.tree.root());
        }
    }

    fn anchor(&mut self, node: NodeRef<Node>, href: Option<&str>) {
        let href = href
            .map(str::trim)
            .filter(|h| crate::platform::is_web_url(h));
        let Some(href) = href else {
            self.children(node);
            return;
        };
        let idx = match self.links.iter().position(|l| l == href) {
            Some(i) => i + 1,
            None => {
                self.links.push(href.to_string());
                self.links.len()
            }
        };
        let before = self.cur.len();
        let outer = self.link.replace(idx);
        self.children(node);
        self.link = outer;
        // A link wrapping block content (a teaser card) has been flushed into
        // its own blocks already, and gets no marker.
        let has_text = self
            .cur
            .get(before..)
            .is_some_and(|runs| runs.iter().any(|i| !i.text.trim().is_empty()));
        if has_text {
            self.cur.push(Inline {
                text: format!("[{idx}]"),
                note: true,
                ..Default::default()
            });
        }
    }

    /// One table row becomes one line, cells joined by " · ".
    fn table_row(&mut self, node: NodeRef<Node>) {
        self.flush();
        let cells: Vec<String> = node
            .children()
            .filter(|c| matches!(c.value(), Node::Element(e) if matches!(e.name(), "td" | "th")))
            .map(|c| {
                let mut raw = String::new();
                collect_raw_text(c, &mut raw);
                raw.split_whitespace().collect::<Vec<_>>().join(" ")
            })
            .filter(|c| !c.is_empty())
            .collect();
        if !cells.is_empty() {
            self.push_rows(vec![cells.join(" · ")]);
        }
    }

    /// Appends rows, merging into the previous block if that is also rows.
    fn push_rows(&mut self, rows: Vec<String>) {
        if rows.is_empty() {
            return;
        }
        if let Some(Block::Rows(prev)) = self.blocks.last_mut() {
            prev.extend(rows);
        } else {
            self.blocks.push(Block::Rows(rows));
        }
    }

    fn push_text(&mut self, t: &str) {
        // Source line breaks are just whitespace; only <br> breaks a line.
        self.push_text_raw(&t.replace(['\n', '\r', '\t'], " "));
    }

    fn push_text_raw(&mut self, t: &str) {
        let style = Inline {
            bold: self.bold > 0,
            italic: self.italic > 0,
            link: self.link,
            ..Default::default()
        };
        match self.cur.last_mut() {
            Some(last)
                if last.bold == style.bold
                    && last.italic == style.italic
                    && last.link == style.link
                    && !last.note =>
            {
                last.text.push_str(t);
            }
            _ => self.cur.push(Inline {
                text: t.to_string(),
                ..style
            }),
        }
    }

    /// Ends the current block: normalizes whitespace and pushes it with the
    /// kind implied by the surrounding context.
    fn flush(&mut self) {
        let runs = normalize_whitespace(std::mem::take(&mut self.cur));
        if runs.iter().all(|i| i.note || i.text.trim().is_empty()) {
            return;
        }
        let block = if self.heading {
            Block::Heading(runs)
        } else if let Some(marker) = self.item_marker.as_mut() {
            Block::Item {
                depth: self.lists.len().clamp(1, 9) as u8,
                marker: std::mem::take(marker),
                content: runs,
            }
        } else if self.quote_depth > 0 {
            Block::Quote(runs)
        } else {
            Block::Para(runs)
        };
        if !is_boilerplate(&block) {
            self.blocks.push(block);
        }
    }
}

fn is_block(tag: &str) -> bool {
    matches!(
        tag,
        "p" | "div"
            | "section"
            | "article"
            | "header"
            | "footer"
            | "main"
            | "table"
            | "figure"
            | "figcaption"
            | "dl"
            | "dt"
            | "dd"
            | "hr"
    )
}

/// Collapses whitespace runs into single spaces across all runs of a block,
/// keeps `<br>` line breaks, and trims the block's start and end.
fn normalize_whitespace(runs: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::with_capacity(runs.len());
    // Start as if after a space so leading whitespace is dropped.
    let mut after_space = true;
    for mut run in runs {
        // A link marker hugs the word before it: "text[1] next", never
        // "text [1]next".
        if run.note {
            let had_space = out.last().is_some_and(|p| p.text.ends_with(' '));
            if had_space && let Some(prev) = out.last_mut() {
                prev.text.pop();
            }
            out.push(run);
            if had_space {
                out.push(Inline {
                    text: " ".into(),
                    ..Default::default()
                });
            }
            after_space = had_space;
            continue;
        }
        let mut text = String::with_capacity(run.text.len());
        for c in run.text.chars() {
            if c == '\n' {
                // A line break swallows the space before it, which may sit at
                // the end of the previous run.
                if text.ends_with(' ') {
                    text.pop();
                } else if text.is_empty()
                    && let Some(prev) = out.last_mut()
                    && prev.text.ends_with(' ')
                {
                    prev.text.pop();
                }
                text.push('\n');
                after_space = true;
            } else if c.is_whitespace() {
                if !after_space {
                    text.push(' ');
                    after_space = true;
                }
            } else {
                text.push(c);
                after_space = false;
            }
        }
        run.text = text;
        if !run.text.is_empty() {
            out.push(run);
        }
    }
    // Trim the end of the block.
    while let Some(last) = out.last_mut() {
        let trimmed = last.text.trim_end().len();
        last.text.truncate(trimmed);
        if last.text.is_empty() {
            out.pop();
        } else {
            break;
        }
    }
    out
}

/// Short paragraphs that are site chrome rather than article text.
fn is_boilerplate(block: &Block) -> bool {
    const PATTERNS: &[&str] = &[
        "share on",
        "share this",
        "sdílet",
        "advertisement",
        "reklama",
        "read more",
        "čtěte také",
        "přečtěte si také",
        "related articles",
        "související",
        "sign up for",
        "subscribe to",
        "newsletter",
        "follow us",
        "sledujte nás",
        "click here",
    ];
    let Block::Para(runs) = block else {
        return false;
    };
    let text: String = runs.iter().map(|r| r.text.as_str()).collect();
    if text.chars().count() > 80 {
        return false;
    }
    let lower = text.to_lowercase();
    PATTERNS.iter().any(|p| lower.contains(p))
}

/// Concatenates every text node under `node`, ignoring element structure.
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

/// Just enough entity decoding to recover an escaped `<img>` tag.
fn decode_entities(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(runs: &[Inline]) -> String {
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    fn texts(content: &ReaderContent) -> Vec<String> {
        content
            .blocks
            .iter()
            .map(|b| match b {
                Block::Heading(r) => format!("# {}", plain(r)),
                Block::Para(r) => plain(r),
                Block::Quote(r) => format!("> {}", plain(r)),
                Block::Item {
                    depth,
                    marker,
                    content,
                } => format!("{depth}{marker} {}", plain(content)),
                Block::Image(alt) => format!("[image: {alt}]"),
                Block::Rows(rows) => rows.join("\n"),
            })
            .collect()
    }

    #[test]
    fn keeps_paragraphs_and_flags_images() {
        let c = html_to_content(
            "<p>First paragraph.</p><p>Second paragraph.</p><img src=\"x.jpg\" alt=\"a cat\"><p>Third.</p>",
        );
        assert_eq!(
            texts(&c),
            [
                "First paragraph.",
                "Second paragraph.",
                "[image: a cat]",
                "Third."
            ]
        );
    }

    #[test]
    fn inline_tags_do_not_add_spaces_before_punctuation() {
        let c = html_to_content(
            "<p>See <a href=\"https://x.com/a\">this report</a>, and <b>bold</b>. Also <i>this</i>!</p>",
        );
        assert_eq!(texts(&c), ["See this report[1], and bold. Also this!"]);
        assert_eq!(c.links, ["https://x.com/a"]);
    }

    #[test]
    fn styles_and_links_are_kept_as_runs() {
        let c = html_to_content(
            "<p>A <strong>bold</strong> and <em>italic</em> <a href=\"https://x.com\">link</a> end</p>",
        );
        let Block::Para(runs) = &c.blocks[0] else {
            panic!("expected a paragraph");
        };
        assert!(runs.iter().any(|r| r.bold && r.text == "bold"));
        assert!(runs.iter().any(|r| r.italic && r.text == "italic"));
        assert!(runs.iter().any(|r| r.link == Some(1) && r.text == "link"));
        assert!(runs.iter().any(|r| r.note && r.text == "[1]"));

        let c = html_to_content("<p><a href=\"https://x.com\">trailing </a>next</p>");
        assert_eq!(texts(&c), ["trailing[1] next"]);
        assert_eq!(plain(runs), "A bold and italic link[1] end");
    }

    #[test]
    fn a_link_wrapping_block_elements_does_not_panic() {
        // irozhlas.cz teaser cards: <a> around <div>/<h3>.
        let c = html_to_content(concat!(
            "<p>Intro <a href=\"https://x.com/card\"><div><h3>Card title</h3>",
            "<p>Card text</p></div></a> after.</p>",
        ));
        // The HTML parser re-opens the <a> inside each block, so each keeps
        // its link marker.
        assert_eq!(
            texts(&c),
            ["Intro", "# Card title[1]", "Card text[1]", "after."]
        );
    }

    #[test]
    fn relative_and_non_web_links_are_plain_text() {
        let c = html_to_content(
            "<p><a href=\"/local\">local</a> <a href=\"javascript:void(0)\">js</a></p>",
        );
        assert!(c.links.is_empty());
        assert_eq!(texts(&c), ["local js"]);
    }

    #[test]
    fn headings_quotes_and_lists_become_their_own_blocks() {
        let c = html_to_content(concat!(
            "<h2>Background</h2>",
            "<blockquote><p>Quoted one.</p><p>Quoted two.</p></blockquote>",
            "<ul><li>Apple</li><li><p>Pear</p><p>More about pears.</p>",
            "<ol start=\"3\"><li>Three</li><li>Four</li></ol></li></ul>",
            "<p>After.</p>",
        ));
        assert_eq!(
            texts(&c),
            [
                "# Background",
                "> Quoted one.",
                "> Quoted two.",
                "1• Apple",
                "1• Pear",
                "1 More about pears.",
                "23. Three",
                "24. Four",
                "After.",
            ]
        );
    }

    #[test]
    fn source_newlines_are_spaces_but_br_breaks_the_line() {
        let c = html_to_content("<p>one\n   two<br>  three</p>");
        assert_eq!(texts(&c), ["one two\nthree"]);
    }

    #[test]
    fn drops_short_boilerplate_paragraphs_only() {
        let c = html_to_content(concat!(
            "<p>Share on Facebook</p>",
            "<p>Real text.</p>",
            "<p>Čtěte také: Něco jiného</p>",
            "<p>This long paragraph mentions a newsletter in passing but is clearly part of the article body itself.</p>",
        ));
        assert_eq!(texts(&c).len(), 2);
        assert_eq!(texts(&c)[0], "Real text.");
    }

    #[test]
    fn unwraps_lazy_loaded_image_stashed_in_noscript() {
        // irozhlas.cz style: the real <img> only exists, escaped, in <noscript>.
        let c = html_to_content(concat!(
            r#"<p>Before.</p>"#,
            r#"<span class="img__holder"><noscript>"#,
            r#"&lt;img src="/x.png" alt="A chart" /&gt;"#,
            r#"</noscript></span>"#,
            r#"<p>After.</p>"#,
        ));
        assert_eq!(texts(&c), ["Before.", "[image: A chart]", "After."]);
    }

    #[test]
    fn renders_orphaned_table_rows_one_per_line() {
        // irozhlas.cz's score widget: bare <tr>/<td>, whitespace between tags.
        let c = html_to_content(
            "\n<tr> <td>25. 7.</td> <td>17:00</td> <td>Zlín – Baník</td> <td>0:1</td> </tr>\n<tr> <td>26. 7.</td> <td>15:00</td> <td>Slavia – Slovácko</td> <td>5:1</td> </tr>\n",
        );
        assert_eq!(
            c.blocks,
            [Block::Rows(vec![
                "25. 7. · 17:00 · Zlín – Baník · 0:1".into(),
                "26. 7. · 15:00 · Slavia – Slovácko · 5:1".into(),
            ])]
        );
    }

    #[test]
    fn cache_round_trips_and_rejects_old_plain_text() {
        let c = html_to_content("<h2>T</h2><p>Body <b>bold</b>.</p>");
        assert_eq!(ReaderContent::from_cache(&c.to_cache()), Some(c));
        assert_eq!(
            ReaderContent::from_cache("Old plain text cache.\n\nSecond."),
            None
        );
    }

    #[test]
    fn reading_time_rounds_up_to_whole_minutes() {
        let words = "word ".repeat(450);
        let c = html_to_content(&format!("<p>{words}</p>"));
        assert_eq!(c.word_count(), 450);
        assert_eq!(c.reading_minutes(), 3);
    }

    #[tokio::test]
    async fn a_gzip_body_sent_without_being_asked_is_decompressed() {
        // Like seznamzpravy.cz: gzip even when the request asked for none.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let body = include_bytes!("../testdata/article.html.gz");
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/article", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            let head = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\n\
                 content-encoding: gzip\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            socket.write_all(head.as_bytes()).await.unwrap();
            socket.write_all(body).await.unwrap();
        });

        let c = fetch_article(&url).await.expect("fetch_article failed");
        assert!(c.plain_text().contains("Čtenářská vášeň"), "{c:?}");
    }

    #[test]
    fn garbled_text_is_rejected_on_download_and_in_the_cache() {
        assert!(looks_like_text(
            "Čtenářská vášeň v Česku slábne.\n\tDruhý řádek."
        ));
        // What the undecompressed gzip page looked like once decoded.
        let garbled =
            "\u{1f}\u{FFFD}\u{8}\u{4}\u{3}\u{FFFD}ے䶕(\u{FFFD}(\u{FFFD}\u{FFFD}JJ0\u{FFFD}Y"
                .repeat(20);
        assert!(!looks_like_text(&garbled));

        let good = html_to_content("<p>Fine text.</p>");
        assert!(ReaderContent::from_cache(&good.to_cache()).is_some());
        let bad = html_to_content(&format!("<p>{garbled}</p>"));
        assert_eq!(ReaderContent::from_cache(&bad.to_cache()), None);
    }

    #[tokio::test]
    #[ignore = "hits the real network"]
    async fn extracts_a_real_article() {
        let c = fetch_article("https://en.wikipedia.org/wiki/Rust_(programming_language)")
            .await
            .expect("fetch_article failed");
        assert!(c.word_count() > 300, "article body looked too short");
        assert!(c.blocks.iter().any(|b| matches!(b, Block::Heading(_))));
    }
}
