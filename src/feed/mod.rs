pub mod fetch;
pub mod opml;

#[derive(Clone, Debug)]
pub struct Feed {
    pub url: String,
    pub category: String,
    /// Display name from the OPML `text`/`title` attribute, falling back to
    /// the feed's domain.
    pub name: String,
    /// Weight in the Briefing: OPML `priority="high"` is 1.5, `"low"` 0.5.
    pub priority: f32,
}

/// Host part of a URL without "www.", used when a feed has no name.
pub fn domain(url: &str) -> String {
    let without_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = without_scheme.split('/').next().unwrap_or(without_scheme);
    host.strip_prefix("www.").unwrap_or(host).to_string()
}
