use std::path::Path;

use anyhow::Result;
use roxmltree::{Document, Node};

use super::Feed;

/// Parses an OPML file into a flat list of feeds, tagging each with the
/// top-level `<outline>` folder it was nested under (or "" if flat).
pub fn parse(path: &Path) -> Result<Vec<Feed>> {
    let xml = std::fs::read_to_string(path)?;
    let doc = Document::parse(&xml)?;

    let body = doc
        .descendants()
        .find(|n| n.has_tag_name("body"))
        .ok_or_else(|| anyhow::anyhow!("OPML has no <body>"))?;

    let mut feeds = Vec::new();
    for outline in body.children().filter(|n| n.has_tag_name("outline")) {
        walk(outline, "", &mut feeds);
    }
    Ok(feeds)
}

fn walk(node: Node, category: &str, feeds: &mut Vec<Feed>) {
    if let Some(url) = node.attribute("xmlUrl") {
        feeds.push(Feed {
            url: url.to_string(),
            category: category.to_string(),
        });
        return;
    }

    let this_category = node
        .attribute("title")
        .or_else(|| node.attribute("text"))
        .unwrap_or_default();

    for child in node.children().filter(|n| n.has_tag_name("outline")) {
        walk(child, this_category, feeds);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_repo_feeds_opml() {
        let feeds = parse(Path::new("feeds.opml")).expect("parse feeds.opml");
        assert!(!feeds.is_empty(), "expected at least one feed");
        for f in &feeds {
            assert!(f.url.starts_with("http"), "bad url: {}", f.url);
            assert!(!f.category.is_empty(), "feed missing category: {}", f.url);
        }
        let categories: std::collections::HashSet<_> =
            feeds.iter().map(|f| f.category.clone()).collect();
        assert!(categories.contains("Tech & Dev"));
    }
}
