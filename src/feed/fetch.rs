use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};

use super::Feed;
use crate::store::{NewArticle, Store};

pub enum FetchEvent {
    Started(usize),
    FeedDone,
    Complete,
    /// A feed failed to download or parse: (feed name, error message).
    Error(String, String),
}

const MAX_CONCURRENT: usize = 16;
const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(1);

pub fn spawn_reload(
    feeds: Vec<Feed>,
    db_path: std::path::PathBuf,
    progress_tx: mpsc::UnboundedSender<FetchEvent>,
) {
    tokio::spawn(async move {
        let total = feeds.len();
        let _ = progress_tx.send(FetchEvent::Started(total));

        let (write_tx, write_rx) = std::sync::mpsc::channel::<Vec<NewArticle>>();
        let writer = std::thread::spawn(move || {
            if let Ok(store) = Store::open(&db_path) {
                while let Ok(batch) = write_rx.recv() {
                    if !batch.is_empty() {
                        let _ = store.upsert_batch(&batch);
                    }
                }
            }
        });

        let client = reqwest::Client::builder()
            .user_agent("dnews/0.1")
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT));
        let mut handles = Vec::with_capacity(total);

        for feed in feeds {
            let permit = semaphore.clone();
            let client = client.clone();
            let write_tx = write_tx.clone();
            let progress_tx = progress_tx.clone();

            handles.push(tokio::spawn(async move {
                let _permit = permit.acquire_owned().await;
                match fetch_one(&client, &feed).await {
                    Ok(articles) => {
                        let _ = write_tx.send(articles);
                    }
                    Err(e) => {
                        let _ = progress_tx
                            .send(FetchEvent::Error(feed.name.clone(), format!("{e:#}")));
                    }
                }
                let _ = progress_tx.send(FetchEvent::FeedDone);
            }));
        }

        drop(write_tx);
        for h in handles {
            let _ = h.await;
        }

        let _ = tokio::task::spawn_blocking(move || writer.join()).await;
        let _ = progress_tx.send(FetchEvent::Complete);
    });
}

async fn fetch_one(client: &reqwest::Client, feed: &Feed) -> anyhow::Result<Vec<NewArticle>> {
    let get = || async {
        client
            .get(&feed.url)
            .send()
            .await?
            .error_for_status()?
            .bytes()
            .await
    };
    // A dropped connection or a DNS blip usually passes within a second; an
    // HTTP error or a broken feed would not, so only those get no retry.
    let bytes = match get().await {
        Err(e) if e.status().is_none() && !e.is_decode() => {
            tokio::time::sleep(RETRY_AFTER).await;
            get().await?
        }
        result => result?,
    };
    let parsed = feed_rs::parser::parse(&bytes[..])?;

    let mut out = Vec::with_capacity(parsed.entries.len());
    for entry in parsed.entries {
        let title = entry
            .title
            .map(|t| t.content)
            .unwrap_or_else(|| "(untitled)".to_string());
        let Some(link) = entry.links.first().map(|l| l.href.clone()) else {
            continue;
        };
        let summary = entry
            .summary
            .map(|s| s.content)
            .or_else(|| entry.content.and_then(|c| c.body))
            .map(|html| plain_summary(&html))
            .unwrap_or_default();
        let published = entry
            .published
            .or(entry.updated)
            .map(|d| d.timestamp())
            .unwrap_or(0);

        out.push(NewArticle {
            category: feed.category.clone(),
            source: feed.name.clone(),
            summary,
            title,
            link,
            published,
        });
    }
    Ok(out)
}

/// Longest summary kept per article, in characters.
const SUMMARY_MAX_CHARS: usize = 400;

/// Feed descriptions are often HTML (and sometimes a whole article): strip
/// the markup, collapse whitespace and keep the first few sentences.
fn plain_summary(html: &str) -> String {
    // Text nodes are joined as-is so inline tags (`<b>world</b>.`) don't gain
    // a space; block boundaries get one so paragraphs don't run together.
    let mut spaced = html.to_string();
    for tag in [
        "<p", "</p", "<br", "<div", "</div", "<li", "<h", "</h", "<tr", "<td",
    ] {
        spaced = spaced.replace(tag, &format!(" {tag}"));
    }
    let text: String = scraper::Html::parse_fragment(&spaced)
        .root_element()
        .text()
        .collect();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if text.chars().count() <= SUMMARY_MAX_CHARS {
        return text;
    }
    let cut: String = text.chars().take(SUMMARY_MAX_CHARS).collect();
    match cut.rfind(' ') {
        Some(i) => format!("{}…", &cut[..i]),
        None => format!("{cut}…"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "hits the real network"]
    async fn fetches_a_real_feed() {
        let client = reqwest::Client::new();
        let feed = Feed {
            url: "https://news.ycombinator.com/rss".to_string(),
            category: "Tech".to_string(),
            name: "Hacker News".to_string(),
            priority: 1.0,
        };
        let articles = fetch_one(&client, &feed).await.expect("fetch_one failed");
        assert!(!articles.is_empty(), "expected at least one entry");
        assert!(!articles[0].title.is_empty());
        assert!(articles[0].link.starts_with("http"));
    }

    #[test]
    fn summary_is_plain_text_and_bounded() {
        assert_eq!(
            plain_summary("<p>Hello <b>world</b>.</p>\n<p>Second   line.</p>"),
            "Hello world. Second line."
        );
        let long = "word ".repeat(200);
        let s = plain_summary(&long);
        assert!(s.chars().count() <= SUMMARY_MAX_CHARS + 1);
        assert!(s.ends_with('…'));
    }

    /// A local server that answers each connection with the next response
    /// (`None` closes the connection unanswered). Returns the feed URL and a
    /// counter of connections.
    async fn serve(
        responses: Vec<Option<&'static str>>,
    ) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::Ordering;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/feed", listener.local_addr().unwrap());
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        tokio::spawn(async move {
            for response in responses {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                seen.fetch_add(1, Ordering::SeqCst);
                let mut request = [0u8; 2048];
                let _ = socket.read(&mut request).await;
                if let Some(response) = response {
                    let _ = socket.write_all(response.as_bytes()).await;
                }
            }
        });
        (url, count)
    }

    const RSS: &str = "HTTP/1.1 200 OK\r\ncontent-type: application/rss+xml\r\nconnection: close\r\n\r\n\
        <rss version=\"2.0\"><channel><title>T</title><item><title>Story</title>\
        <link>https://example.com/story</link></item></channel></rss>";

    fn test_feed(url: String) -> Feed {
        Feed {
            url,
            category: "World".into(),
            name: "Test".into(),
            priority: 1.0,
        }
    }

    #[tokio::test]
    async fn a_dropped_connection_is_retried_once() {
        let (url, connections) = serve(vec![None, Some(RSS)]).await;
        let articles = fetch_one(&reqwest::Client::new(), &test_feed(url))
            .await
            .expect("the retry should succeed");
        assert_eq!(articles[0].title, "Story");
        assert_eq!(connections.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn an_http_error_fails_at_once_and_says_why() {
        let (url, connections) = serve(vec![Some(
            "HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
        )])
        .await;
        let Err(err) = fetch_one(&reqwest::Client::new(), &test_feed(url)).await else {
            panic!("a 404 must fail");
        };
        assert!(format!("{err:#}").contains("404"), "{err:#}");
        assert_eq!(
            connections.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "no retry"
        );
    }
}
