use std::sync::Arc;

use tokio::sync::{Semaphore, mpsc};

use super::Feed;
use crate::store::{NewArticle, Store};

pub enum FetchEvent {
    Started(usize),
    FeedDone,
    Complete,
    Error(String),
}

const MAX_CONCURRENT: usize = 16;

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
                        let domain = crate::ui::domain(&feed.url);
                        let _ = progress_tx.send(FetchEvent::Error(format!("{domain}: {e}")));
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
    let bytes = client.get(&feed.url).send().await?.bytes().await?;
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
        let published = entry
            .published
            .or(entry.updated)
            .map(|d| d.timestamp())
            .unwrap_or(0);

        out.push(NewArticle {
            category: feed.category.clone(),
            title,
            link,
            published,
        });
    }
    Ok(out)
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
        };
        let articles = fetch_one(&client, &feed).await.expect("fetch_one failed");
        assert!(!articles.is_empty(), "expected at least one entry");
        assert!(!articles[0].title.is_empty());
        assert!(articles[0].link.starts_with("http"));
    }
}
