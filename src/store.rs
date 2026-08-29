use std::path::Path;

use anyhow::Result;
use rusqlite::{Connection, params};

pub struct Store {
    path: std::path::PathBuf,
}

#[derive(Clone, Debug)]
pub struct ArticleRow {
    pub title: String,
    pub link: String,
    pub published: i64,
    pub unread: bool,
    pub saved: bool,
    pub content_text: Option<String>,
}

pub struct NewArticle {
    pub category: String,
    pub title: String,
    pub link: String,
    pub published: i64,
}

/// Which articles `Store::list` should return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListFilter {
    All,
    Category(String),
    Saved,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    fn conn(&self) -> Result<Connection> {
        Ok(Connection::open(&self.path)?)
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.path.clone()
    }

    pub fn init_schema(&self) -> Result<()> {
        let conn = self.conn()?;
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
        // Older dev DBs predate the `saved` column; add it if missing rather
        // than requiring a fresh DB. SQLite has no `ADD COLUMN IF NOT
        // EXISTS`, so just ignore the "duplicate column" error.
        let _ = conn.execute(
            "ALTER TABLE articles ADD COLUMN saved INTEGER NOT NULL DEFAULT 0",
            [],
        );
        Ok(())
    }

    pub fn list(&self, filter: &ListFilter) -> Result<Vec<ArticleRow>> {
        let conn = self.conn()?;
        let mut rows = Vec::new();
        let mut push = |r: &rusqlite::Row| -> rusqlite::Result<()> {
            rows.push(ArticleRow {
                title: r.get(0)?,
                link: r.get(1)?,
                published: r.get(2)?,
                unread: r.get::<_, i64>(3)? != 0,
                saved: r.get::<_, i64>(4)? != 0,
                content_text: r.get(5)?,
            });
            Ok(())
        };

        const COLS: &str = "title, link, published, unread, saved, content_text";
        match filter {
            ListFilter::All => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLS} FROM articles ORDER BY published DESC"
                ))?;
                let mut it = stmt.query([])?;
                while let Some(r) = it.next()? {
                    push(r)?;
                }
            }
            ListFilter::Category(cat) => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLS} FROM articles WHERE category = ?1 ORDER BY published DESC"
                ))?;
                let mut it = stmt.query(params![cat])?;
                while let Some(r) = it.next()? {
                    push(r)?;
                }
            }
            ListFilter::Saved => {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {COLS} FROM articles WHERE saved = 1 ORDER BY published DESC"
                ))?;
                let mut it = stmt.query([])?;
                while let Some(r) = it.next()? {
                    push(r)?;
                }
            }
        }
        Ok(rows)
    }

    pub fn unread_count(&self) -> Result<i64> {
        let conn = self.conn()?;
        Ok(
            conn.query_row("SELECT count(*) FROM articles WHERE unread = 1", [], |r| {
                r.get(0)
            })?,
        )
    }

    pub fn mark_read(&self, link: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE articles SET unread = 0 WHERE link = ?1",
            params![link],
        )?;
        Ok(())
    }

    pub fn set_saved(&self, link: &str, saved: bool) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE articles SET saved = ?1 WHERE link = ?2",
            params![saved as i64, link],
        )?;
        Ok(())
    }

    pub fn cache_content(&self, link: &str, content: &str) -> Result<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE articles SET content_text = ?1 WHERE link = ?2",
            params![content, link],
        )?;
        Ok(())
    }

    pub fn upsert_batch(&self, articles: &[NewArticle]) -> Result<()> {
        let mut conn = self.conn()?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO articles (category, title, link, published, unread)
                 VALUES (?1, ?2, ?3, ?4, 1)
                 ON CONFLICT(link) DO UPDATE SET
                    category = excluded.category,
                    title = excluded.title,
                    published = excluded.published",
            )?;
            for a in articles {
                stmt.execute(params![a.category, a.title, a.link, a.published])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes articles published before `cutoff` (unix epoch seconds) —
    /// saved articles are always kept, regardless of age.
    pub fn prune_old(&self, cutoff: i64) -> Result<usize> {
        let conn = self.conn()?;
        Ok(conn.execute(
            "DELETE FROM articles WHERE published < ?1 AND saved = 0",
            params![cutoff],
        )?)
    }

    /// Copies the DB file to a sibling `.bak` (overwriting any previous
    /// one) — a rolling one-generation safety net, meant to be called right
    /// before `init_schema` runs on startup so a future migration bug can't
    /// silently lose saved articles: restore with `cp dnews.db.bak
    /// dnews.db`. A no-op if there's nothing to back up yet (first run).
    pub fn backup(&self) -> Result<()> {
        if !self.path.exists() {
            return Ok(());
        }
        let backup_path = self.path.with_extension("db.bak");
        std::fs::copy(&self.path, &backup_path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> Store {
        let path = std::env::temp_dir().join(format!(
            "dnews_store_test_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store::open(&path).unwrap();
        store.init_schema().unwrap();
        store
    }

    #[test]
    fn prune_old_keeps_saved_articles_regardless_of_age() {
        let store = test_store();
        store
            .upsert_batch(&[
                NewArticle {
                    category: "".into(),
                    title: "old unsaved".into(),
                    link: "https://example.com/old-unsaved".into(),
                    published: 1000,
                },
                NewArticle {
                    category: "".into(),
                    title: "old saved".into(),
                    link: "https://example.com/old-saved".into(),
                    published: 1000,
                },
                NewArticle {
                    category: "".into(),
                    title: "recent".into(),
                    link: "https://example.com/recent".into(),
                    published: 1_000_000,
                },
            ])
            .unwrap();
        store
            .set_saved("https://example.com/old-saved", true)
            .unwrap();

        let deleted = store.prune_old(500_000).unwrap();
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
            }])
            .unwrap();
        store
            .set_saved("https://example.com/keep-me", true)
            .unwrap();

        store.backup().unwrap();

        let backup_path = store.path().with_extension("db.bak");
        assert!(backup_path.exists(), "backup file was not created");

        // The backup is a real, independently openable DB with the same
        // saved article — not just a byte-identical file we trust blindly.
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
        let path = std::env::temp_dir().join(format!(
            "dnews_store_test_nofile_{}_{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = Store::open(&path).unwrap();
        // Deliberately not calling init_schema(): the file doesn't exist yet.
        store.backup().unwrap();
        assert!(!path.with_extension("db.bak").exists());
    }
}
