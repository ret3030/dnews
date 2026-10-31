use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params};

/// How far back the Briefing looks.
pub const BRIEFING_WINDOW_SECS: i64 = 48 * 3600;

pub struct Store {
    conn: Connection,
    path: std::path::PathBuf,
}

#[derive(Clone, Debug, Default)]
pub struct ArticleRow {
    pub category: String,
    /// Feed name from the OPML; empty for rows stored before it existed.
    pub source: String,
    pub summary: String,
    pub title: String,
    pub link: String,
    pub published: i64,
    pub unread: bool,
    pub saved: bool,
    pub content_text: Option<String>,
    /// Where the reader was left in this article.
    pub scroll_pos: u16,
}

impl ArticleRow {
    /// The feed's name, or the domain for rows stored before names existed.
    pub fn source_name(&self) -> String {
        if self.source.is_empty() {
            crate::feed::domain(&self.link)
        } else {
            self.source.clone()
        }
    }
}

#[derive(Default)]
pub struct NewArticle {
    pub category: String,
    pub source: String,
    pub summary: String,
    pub title: String,
    pub link: String,
    pub published: i64,
}

/// Which articles `Store::list` should return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListFilter {
    /// The last two days, grouped into stories by `cluster`.
    Briefing,
    All,
    Category(String),
    Saved,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        // The feed writer and the article cache write from other threads
        // through their own connections; wait for their lock, don't fail.
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(Self {
            conn,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn init_schema(&self) -> Result<()> {
        let conn = &self.conn;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                category TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL,
                link TEXT NOT NULL UNIQUE,
                published INTEGER NOT NULL DEFAULT 0,
                unread INTEGER NOT NULL DEFAULT 1,
                saved INTEGER NOT NULL DEFAULT 0,
                content_text TEXT
            );
            CREATE INDEX IF NOT EXISTS idx_articles_category ON articles(category);
            CREATE INDEX IF NOT EXISTS idx_articles_published ON articles(published);",
        )?;
        // Columns added after the first release. SQLite has no `ADD COLUMN
        // IF NOT EXISTS`, so the "duplicate column" error is ignored.
        for column in [
            "saved INTEGER NOT NULL DEFAULT 0",
            "source TEXT NOT NULL DEFAULT ''",
            "summary TEXT NOT NULL DEFAULT ''",
            "scroll_pos INTEGER NOT NULL DEFAULT 0",
        ] {
            let _ = conn.execute(&format!("ALTER TABLE articles ADD COLUMN {column}"), []);
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);",
        )?;
        Ok(())
    }

    pub fn list(&self, filter: &ListFilter) -> Result<Vec<ArticleRow>> {
        let briefing = format!("published > strftime('%s', 'now') - {BRIEFING_WINDOW_SECS}");
        let (condition, param) = match filter {
            ListFilter::Briefing => (briefing.as_str(), None),
            ListFilter::All => ("1", None),
            ListFilter::Category(c) => ("category = ?1", Some(c.clone())),
            ListFilter::Saved => ("saved = 1", None),
        };
        self.query(condition, param)
    }

    fn query(&self, condition: &str, param: Option<String>) -> Result<Vec<ArticleRow>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT title, link, published, unread, saved, content_text, source, summary,
                    scroll_pos, category
             FROM articles WHERE {condition} ORDER BY published DESC"
        ))?;
        let rows = stmt.query_map(rusqlite::params_from_iter(param), |r| {
            Ok(ArticleRow {
                title: r.get(0)?,
                link: r.get(1)?,
                published: r.get(2)?,
                unread: r.get(3)?,
                saved: r.get(4)?,
                content_text: r.get(5)?,
                source: r.get(6)?,
                summary: r.get(7)?,
                scroll_pos: r.get(8)?,
                category: r.get(9)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Unread articles per category.
    pub fn unread_by_category(&self) -> Result<std::collections::HashMap<String, i64>> {
        let conn = &self.conn;
        let mut stmt = conn.prepare(
            "SELECT category, count(*) FROM articles WHERE unread = 1 GROUP BY category",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        let conn = &self.conn;
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        Ok(match rows.next()? {
            Some(r) => Some(r.get(0)?),
            None => None,
        })
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        let conn = &self.conn;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn unread_count(&self) -> Result<i64> {
        let conn = &self.conn;
        Ok(
            conn.query_row("SELECT count(*) FROM articles WHERE unread = 1", [], |r| {
                r.get(0)
            })?,
        )
    }

    pub fn set_unread(&self, link: &str, unread: bool) -> Result<()> {
        let conn = &self.conn;
        conn.execute(
            "UPDATE articles SET unread = ?1 WHERE link = ?2",
            params![unread as i64, link],
        )?;
        Ok(())
    }

    pub fn mark_read_many(&self, links: &[String]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE articles SET unread = 0 WHERE link = ?1")?;
            for link in links {
                stmt.execute(params![link])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn set_saved(&self, link: &str, saved: bool) -> Result<()> {
        let conn = &self.conn;
        conn.execute(
            "UPDATE articles SET saved = ?1 WHERE link = ?2",
            params![saved as i64, link],
        )?;
        Ok(())
    }

    pub fn set_scroll(&self, link: &str, pos: u16) -> Result<()> {
        let conn = &self.conn;
        conn.execute(
            "UPDATE articles SET scroll_pos = ?1 WHERE link = ?2",
            params![pos, link],
        )?;
        Ok(())
    }

    pub fn cache_content(&self, link: &str, content: &str) -> Result<()> {
        let conn = &self.conn;
        conn.execute(
            "UPDATE articles SET content_text = ?1 WHERE link = ?2",
            params![content, link],
        )?;
        Ok(())
    }

    pub fn upsert_batch(&self, articles: &[NewArticle]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO articles (category, source, summary, title, link, published, unread)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)
                 ON CONFLICT(link) DO UPDATE SET
                    category = excluded.category,
                    source = excluded.source,
                    summary = excluded.summary,
                    title = excluded.title,
                    published = excluded.published",
            )?;
            for a in articles {
                stmt.execute(params![
                    a.category,
                    a.source,
                    a.summary,
                    a.title,
                    a.link,
                    a.published
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes unsaved articles published before `cutoff` (unix seconds) or
    /// filed under a category that is no longer in `categories`. An empty
    /// list keeps every category (SQLite's `NOT IN ()` would match all).
    pub fn prune(&self, cutoff: i64, categories: &[String]) -> Result<usize> {
        if categories.is_empty() {
            return Ok(self.conn.execute(
                "DELETE FROM articles WHERE saved = 0 AND published < ?1",
                params![cutoff],
            )?);
        }
        let placeholders = vec!["?"; categories.len()].join(", ");
        let sql = format!(
            "DELETE FROM articles WHERE saved = 0
             AND (published < ? OR category NOT IN ({placeholders}))"
        );
        let params = std::iter::once(cutoff.to_string()).chain(categories.iter().cloned());
        Ok(self
            .conn
            .execute(&sql, rusqlite::params_from_iter(params))?)
    }
}

/// Copies the DB to `dnews.db.bak` before a schema migration runs, so a
/// migration bug can't lose saved articles. Nothing to do on the first run.
pub fn backup(path: &Path) -> Result<()> {
    if path.exists() {
        std::fs::copy(path, path.with_extension("db.bak"))?;
    }
    Ok(())
}

/// A fresh DB path per call. The counter matters: the clock alone can repeat
/// between tests running in parallel (macOS has microsecond resolution).
#[cfg(test)]
pub fn temp_db_path(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "{tag}_{}_{}.db",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Store {
        let path = crate::store::temp_db_path("dnews_store_test");
        let store = Store::open(&path).unwrap();
        store.init_schema().unwrap();
        store
    }

    #[test]
    fn prune_keeps_saved_articles_regardless_of_age() {
        let store = test_store();
        store
            .upsert_batch(&[
                NewArticle {
                    category: "".into(),
                    title: "old unsaved".into(),
                    link: "https://example.com/old-unsaved".into(),
                    published: 1000,
                    ..Default::default()
                },
                NewArticle {
                    category: "".into(),
                    title: "old saved".into(),
                    link: "https://example.com/old-saved".into(),
                    published: 1000,
                    ..Default::default()
                },
                NewArticle {
                    category: "".into(),
                    title: "recent".into(),
                    link: "https://example.com/recent".into(),
                    published: 1_000_000,
                    ..Default::default()
                },
            ])
            .unwrap();
        store
            .set_saved("https://example.com/old-saved", true)
            .unwrap();

        let deleted = store.prune(500_000, &[String::new()]).unwrap();
        assert_eq!(deleted, 1);

        let remaining: Vec<String> = store
            .list(&ListFilter::All)
            .unwrap()
            .into_iter()
            .map(|a| a.link)
            .collect();
        assert!(!remaining.contains(&"https://example.com/old-unsaved".to_string()));
        assert!(remaining.contains(&"https://example.com/old-saved".to_string()));
        assert!(remaining.contains(&"https://example.com/recent".to_string()));
    }

    #[test]
    fn backup_copies_saved_articles_to_a_sibling_bak_file() {
        let store = test_store();
        store
            .upsert_batch(&[NewArticle {
                category: "".into(),
                title: "keep me".into(),
                link: "https://example.com/keep-me".into(),
                published: 1000,
                ..Default::default()
            }])
            .unwrap();
        store
            .set_saved("https://example.com/keep-me", true)
            .unwrap();

        backup(store.path()).unwrap();

        let backup_path = store.path().with_extension("db.bak");
        assert!(backup_path.exists(), "backup file was not created");

        // The backup opens as a DB on its own and still has the article.
        let restored = Store::open(&backup_path).unwrap();
        let saved: Vec<String> = restored
            .list(&ListFilter::Saved)
            .unwrap()
            .into_iter()
            .map(|a| a.link)
            .collect();
        assert_eq!(saved, vec!["https://example.com/keep-me".to_string()]);

        let _ = std::fs::remove_file(&backup_path);
    }

    #[test]
    fn backup_is_a_no_op_when_theres_no_db_file_yet() {
        let path = crate::store::temp_db_path("dnews_store_test_nofile");
        backup(&path).unwrap();
        assert!(!path.with_extension("db.bak").exists());
    }
    #[test]
    fn init_schema_upgrades_a_db_from_before_source_and_summary() {
        let store = test_store();
        // Recreate the original table layout, with one existing row.
        let conn = &store.conn;
        conn.execute_batch(
            "DROP TABLE articles;
             CREATE TABLE articles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                category TEXT NOT NULL DEFAULT '',
                title TEXT NOT NULL,
                link TEXT NOT NULL UNIQUE,
                published INTEGER NOT NULL DEFAULT 0,
                unread INTEGER NOT NULL DEFAULT 1,
                content_text TEXT
             );
             INSERT INTO articles (title, link) VALUES ('old', 'https://example.com/old');",
        )
        .unwrap();

        store.init_schema().unwrap();
        let rows = store.list(&ListFilter::All).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].source, "");

        store
            .upsert_batch(&[NewArticle {
                source: "BBC World".into(),
                summary: "A summary.".into(),
                title: "old".into(),
                link: "https://example.com/old".into(),
                ..Default::default()
            }])
            .unwrap();
        let rows = store.list(&ListFilter::All).unwrap();
        assert_eq!(rows[0].source, "BBC World");
        assert_eq!(rows[0].summary, "A summary.");
    }

    #[test]
    fn settings_round_trip_and_unread_counts_group_by_category() {
        let store = test_store();
        assert_eq!(store.setting("compact").unwrap(), None);
        store.set_setting("compact", "1").unwrap();
        store.set_setting("compact", "0").unwrap();
        assert_eq!(store.setting("compact").unwrap().as_deref(), Some("0"));

        store
            .upsert_batch(&[
                NewArticle {
                    category: "World".into(),
                    title: "a".into(),
                    link: "https://example.com/a".into(),
                    ..Default::default()
                },
                NewArticle {
                    category: "World".into(),
                    title: "b".into(),
                    link: "https://example.com/b".into(),
                    ..Default::default()
                },
                NewArticle {
                    category: "Tech".into(),
                    title: "c".into(),
                    link: "https://example.com/c".into(),
                    ..Default::default()
                },
            ])
            .unwrap();
        store.set_unread("https://example.com/c", false).unwrap();
        let counts = store.unread_by_category().unwrap();
        assert_eq!(counts.get("World"), Some(&2));
        assert_eq!(counts.get("Tech"), None);
    }

    #[test]
    fn prune_drops_categories_gone_from_the_opml_but_keeps_saved() {
        let store = test_store();
        let article = |category: &str, link: &str| NewArticle {
            category: category.into(),
            title: "t".into(),
            link: link.into(),
            published: 1_000_000,
            ..Default::default()
        };
        store
            .upsert_batch(&[
                article("World", "https://example.com/kept"),
                article("Old folder", "https://example.com/gone"),
                article("Old folder", "https://example.com/saved"),
            ])
            .unwrap();
        store.set_saved("https://example.com/saved", true).unwrap();

        assert_eq!(
            store.prune(0, &[]).unwrap(),
            0,
            "no list, no category pruning"
        );
        assert_eq!(store.prune(0, &["World".to_string()]).unwrap(), 1);
        let left: Vec<String> = store
            .list(&ListFilter::All)
            .unwrap()
            .into_iter()
            .map(|a| a.link)
            .collect();
        assert_eq!(left.len(), 2);
        assert!(!left.contains(&"https://example.com/gone".to_string()));
    }
}
