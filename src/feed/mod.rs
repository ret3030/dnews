pub mod fetch;
pub mod opml;

#[derive(Clone, Debug)]
pub struct Feed {
    pub url: String,
    pub category: String,
    /// Display name from the OPML `text`/`title` attribute, falling back to
    /// the feed's domain.
    pub name: String,
}
