pub mod fetch;
pub mod opml;

#[derive(Clone, Debug)]
pub struct Feed {
    pub url: String,
    pub category: String,
}
