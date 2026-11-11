use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::NaiveDate;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::core::{
    Article, Bookmark, CachedArticle, Feed, FeedDate, HistoryEntry, LibrarySnapshot, LoadError,
    ReadingList, SearchHit, Source,
};

/// Articles expire three days after they were last opened. Pinned bookmarks
/// and reading-list entries remain available until the user removes them.
pub const CACHE_TTL_DAYS: u32 = 3;

const MAX_BLOB_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialization error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, CacheError>;

impl From<CacheError> for LoadError {
    fn from(value: CacheError) -> Self {
        LoadError::new(crate::core::LoadErrorKind::Unknown, value.to_string())
    }
}

/// SQLite-backed offline cache. The connection is serialized with a mutex so
/// the cache can be shared by blocking tasks without ever touching the UI
/// thread. Article and feed payloads are compressed with zstd at rest.
pub struct Cache {
    conn: Mutex<Connection>,
    path: PathBuf,
}

impl Cache {
    pub fn open() -> Result<Self> {
        let dir = crate::core::Config::cache_dir();
        std::fs::create_dir_all(&dir)?;
        Self::open_at(dir.join("cache.db"))
    }

    pub fn open_at(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA temp_store=MEMORY;",
        )?;
        let cache = Self {
            conn: Mutex::new(conn),
            path,
        };
        cache.migrate()?;
        Ok(cache)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn migrate(&self) -> Result<()> {
        self.conn().execute_batch(
            "CREATE TABLE IF NOT EXISTS cached_articles(
                title TEXT PRIMARY KEY COLLATE NOCASE,
                payload BLOB NOT NULL,
                summary TEXT NOT NULL DEFAULT '',
                plain_text TEXT NOT NULL DEFAULT '',
                pageid INTEGER,
                revision INTEGER,
                fetched_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL,
                size_bytes INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS cached_articles_opened
                ON cached_articles(last_opened_at);
             CREATE TABLE IF NOT EXISTS feed_cache(
                date TEXT PRIMARY KEY,
                payload BLOB NOT NULL,
                fetched_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS bookmarks(
                title TEXT PRIMARY KEY COLLATE NOCASE,
                added_at INTEGER NOT NULL,
                pinned INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS history(
                title TEXT PRIMARY KEY COLLATE NOCASE,
                visited_at INTEGER NOT NULL,
                visits INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS reading_lists(
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL UNIQUE,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS reading_list_items(
                list_id INTEGER NOT NULL REFERENCES reading_lists(id) ON DELETE CASCADE,
                title TEXT NOT NULL COLLATE NOCASE,
                added_at INTEGER NOT NULL,
                PRIMARY KEY(list_id, title)
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS article_fts USING fts5(
                title, summary, plain_text, tokenize='unicode61 remove_diacritics 2'
             );
             CREATE TABLE IF NOT EXISTS settings(
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
             );",
        )?;
        Ok(())
    }

    pub fn get_article(&self, title: &str) -> Result<Option<Article>> {
        let row = self
            .conn()
            .query_row(
                "SELECT payload FROM cached_articles WHERE title = ?1",
                [title],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        let Some(blob) = row else {
            return Ok(None);
        };
        let mut article: Article = decompress(&blob)?;
        article.source = Source::Cache;
        Ok(Some(article))
    }

    pub fn put_article(&self, article: &Article) -> Result<u64> {
        let blob = compress(article)?;
        let size = blob.len() as u64;
        let now = now();
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO cached_articles(
                title, payload, summary, plain_text, pageid, revision,
                fetched_at, last_opened_at, size_bytes
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8)
             ON CONFLICT(title) DO UPDATE SET
                payload=excluded.payload,
                summary=excluded.summary,
                plain_text=excluded.plain_text,
                pageid=excluded.pageid,
                revision=excluded.revision,
                fetched_at=excluded.fetched_at,
                last_opened_at=excluded.last_opened_at,
                size_bytes=excluded.size_bytes",
            params![
                article.title,
                blob,
                article.summary,
                article.plain_text,
                article.pageid.map(|v| v as i64),
                article.revision.map(|v| v as i64),
                article.fetched_at.max(now),
                size as i64,
            ],
        )?;
        tx.execute("DELETE FROM article_fts WHERE title = ?1", [&article.title])?;
        tx.execute(
            "INSERT INTO article_fts(title, summary, plain_text) VALUES (?1, ?2, ?3)",
            params![article.title, article.summary, article.plain_text],
        )?;
        tx.commit()?;
        Ok(size)
    }

    pub fn touch_article(&self, title: &str) -> Result<bool> {
        let changed = self.conn().execute(
            "UPDATE cached_articles SET last_opened_at = ?1 WHERE title = ?2",
            params![now(), title],
        )?;
        Ok(changed > 0)
    }

    #[cfg(test)]
    pub fn has_article(&self, title: &str) -> Result<bool> {
        Ok(self
            .conn()
            .query_row(
                "SELECT 1 FROM cached_articles WHERE title = ?1",
                [title],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    pub fn put_feed(&self, feed: &Feed) -> Result<()> {
        let blob = compress(feed)?;
        let now = now();
        self.conn().execute(
            "INSERT INTO feed_cache(date, payload, fetched_at, last_opened_at)
             VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(date) DO UPDATE SET
                payload=excluded.payload,
                fetched_at=excluded.fetched_at,
                last_opened_at=excluded.last_opened_at",
            params![feed.date.to_string(), blob, feed.fetched_at.max(now)],
        )?;
        Ok(())
    }

    pub fn get_feed(&self, date: NaiveDate) -> Result<Option<Feed>> {
        let row = self
            .conn()
            .query_row(
                "SELECT payload FROM feed_cache WHERE date = ?1",
                [date.to_string()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?;
        let Some(blob) = row else {
            return Ok(None);
        };
        let mut feed: Feed = decompress(&blob)?;
        feed.source = Source::Cache;
        Ok(Some(feed))
    }

    pub fn touch_feed(&self, date: NaiveDate) -> Result<()> {
        self.conn().execute(
            "UPDATE feed_cache SET last_opened_at = ?1 WHERE date = ?2",
            params![now(), date.to_string()],
        )?;
        Ok(())
    }

    pub fn bookmarks(&self) -> Result<Vec<Bookmark>> {
        let conn = self.conn();
        let mut stmt =
            conn.prepare("SELECT title, added_at, pinned FROM bookmarks ORDER BY added_at DESC")?;
        let rows = stmt.query_map([], |row| {
            Ok(Bookmark {
                title: row.get(0)?,
                added_at: row.get(1)?,
                pinned: row.get::<_, i64>(2)? != 0,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn is_bookmarked(&self, title: &str) -> Result<bool> {
        Ok(self
            .conn()
            .query_row("SELECT 1 FROM bookmarks WHERE title = ?1", [title], |_| {
                Ok(())
            })
            .optional()?
            .is_some())
    }

    pub fn toggle_bookmark(&self, title: &str) -> Result<bool> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        let exists = tx
            .query_row("SELECT 1 FROM bookmarks WHERE title = ?1", [title], |_| {
                Ok(())
            })
            .optional()?
            .is_some();
        if exists {
            tx.execute("DELETE FROM bookmarks WHERE title = ?1", [title])?;
        } else {
            tx.execute(
                "INSERT INTO bookmarks(title, added_at, pinned) VALUES (?1, ?2, 1)",
                params![title, now()],
            )?;
        }
        tx.commit()?;
        Ok(!exists)
    }

    pub fn record_history(&self, title: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO history(title, visited_at, visits) VALUES (?1, ?2, 1)
             ON CONFLICT(title) DO UPDATE SET
                visited_at=excluded.visited_at,
                visits=history.visits + 1",
            params![title, now()],
        )?;
        Ok(())
    }

    pub fn history(&self) -> Result<Vec<HistoryEntry>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT title, visited_at, visits FROM history
             ORDER BY visited_at DESC LIMIT 500",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(HistoryEntry {
                title: row.get(0)?,
                visited_at: row.get(1)?,
                visits: row.get::<_, i64>(2)? as u32,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn reading_lists(&self) -> Result<Vec<ReadingList>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT l.id, l.name, l.created_at, COUNT(i.title)
             FROM reading_lists l
             LEFT JOIN reading_list_items i ON i.list_id = l.id
             GROUP BY l.id, l.name, l.created_at
             ORDER BY l.created_at ASC, l.id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok(ReadingList {
                id: row.get(0)?,
                name: row.get(1)?,
                created_at: row.get(2)?,
                item_count: row.get::<_, i64>(3)? as usize,
            })
        })?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn create_reading_list(&self, name: &str) -> Result<ReadingList> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CacheError::Invalid("reading list needs a name".into()));
        }
        let now = now();
        self.conn().execute(
            "INSERT INTO reading_lists(name, created_at) VALUES (?1, ?2)
             ON CONFLICT(name) DO NOTHING",
            params![name, now],
        )?;
        let id: i64 = self.conn().query_row(
            "SELECT id FROM reading_lists WHERE name = ?1",
            [name],
            |row| row.get(0),
        )?;
        Ok(ReadingList {
            id,
            name: name.to_string(),
            created_at: now,
            item_count: 0,
        })
    }

    pub fn delete_reading_list(&self, id: i64) -> Result<()> {
        self.conn()
            .execute("DELETE FROM reading_lists WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn add_to_list(&self, list_id: i64, title: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO reading_list_items(list_id, title, added_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(list_id, title) DO NOTHING",
            params![list_id, title, now()],
        )?;
        Ok(())
    }

    pub fn remove_from_list(&self, list_id: i64, title: &str) -> Result<()> {
        self.conn().execute(
            "DELETE FROM reading_list_items WHERE list_id = ?1 AND title = ?2",
            params![list_id, title],
        )?;
        Ok(())
    }

    pub fn list_items(&self, list_id: i64) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT title FROM reading_list_items WHERE list_id = ?1
             ORDER BY added_at DESC, title ASC",
        )?;
        let rows = stmt.query_map([list_id], |row| row.get::<_, String>(0))?;
        Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
    }

    pub fn list_items_with_thumbnails(
        &self,
        list_id: i64,
    ) -> Result<Vec<(String, Option<String>)>> {
        let titles = self.list_items(list_id)?;
        let mut out = Vec::with_capacity(titles.len());
        for title in titles {
            let thumb = self.get_article(&title).ok().flatten().and_then(|a| {
                a.media.iter().find_map(|m| match m.kind {
                    crate::core::MediaKind::Image | crate::core::MediaKind::Gif | crate::core::MediaKind::Svg => Some(m.url.clone()),
                    _ => None,
                })
            });
            out.push((title, thumb));
        }
        Ok(out)
    }

    pub fn local_search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let terms = query
            .split_whitespace()
            .map(|term| format!("\"{}\"", term.replace('"', "")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let conn = self.conn();
        let mut hits = Vec::new();
        if let Ok(mut stmt) = conn.prepare(
            "SELECT title, snippet(article_fts, 2, '', '', ' … ', 18)
             FROM article_fts WHERE article_fts MATCH ?1 ORDER BY rank LIMIT ?2",
        ) && let Ok(rows) = stmt.query_map(params![terms, limit as i64], |row| {
            Ok(SearchHit {
                title: row.get(0)?,
                snippet: row.get(1)?,
                pageid: None,
                local: true,
            })
        }) {
            hits.extend(rows.flatten());
        }
        if hits.is_empty() {
            let like = format!("%{}%", query.replace('%', ""));
            if let Ok(mut stmt) = conn.prepare(
                "SELECT title, summary FROM cached_articles
                 WHERE title LIKE ?1 OR summary LIKE ?1
                 ORDER BY last_opened_at DESC LIMIT ?2",
            ) && let Ok(rows) = stmt.query_map(params![like, limit as i64], |row| {
                Ok(SearchHit {
                    title: row.get(0)?,
                    snippet: row.get(1)?,
                    pageid: None,
                    local: true,
                })
            }) {
                hits.extend(rows.flatten());
            }
        }
        hits.truncate(limit);
        Ok(hits)
    }

    pub fn library(&self) -> Result<LibrarySnapshot> {
        let articles = {
            let conn = self.conn();
            let mut stmt = conn.prepare(
                "SELECT title, summary, size_bytes, fetched_at, last_opened_at
                 FROM cached_articles ORDER BY last_opened_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(CachedArticle {
                    title: row.get(0)?,
                    summary: row.get(1)?,
                    plain_text: String::new(),
                    size_bytes: row.get::<_, i64>(2)?.max(0) as u64,
                    fetched_at: row.get(3)?,
                    last_opened_at: row.get(4)?,
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let feeds = {
            let conn = self.conn();
            let mut stmt =
                conn.prepare("SELECT date, fetched_at FROM feed_cache ORDER BY date DESC")?;
            let rows = stmt.query_map([], |row| {
                let date: String = row.get(0)?;
                Ok(FeedDate {
                    date: date
                        .parse()
                        .unwrap_or_else(|_| NaiveDate::from_ymd_opt(1970, 1, 1).unwrap()),
                    sections: 0,
                    items: 0,
                    fetched_at: row.get(1)?,
                })
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        let bytes = articles.iter().map(|a| a.size_bytes).sum();
        Ok(LibrarySnapshot {
            articles,
            feeds,
            bookmarks: self.bookmarks()?,
            history: self.history()?,
            lists: self.reading_lists()?,
            bytes,
        })
    }

    /// Remove expired, unpinned content and enforce the disk budget. Returns
    /// the number of bytes released.
    pub fn purge(&self, ttl_secs: i64, max_size_bytes: u64) -> Result<u64> {
        let cutoff = now() - ttl_secs.max(0);
        let mut conn = self.conn();
        let before: i64 = conn.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM cached_articles",
            [],
            |row| row.get(0),
        )?;
        let tx = conn.transaction()?;
        let expired = tx.execute(
            "DELETE FROM cached_articles
             WHERE last_opened_at < ?1
               AND title NOT IN (SELECT title FROM bookmarks)
               AND title NOT IN (SELECT title FROM reading_list_items)",
            [cutoff],
        )?;
        let mut removed_bytes = 0u64;
        if expired > 0 {
            // size_bytes of deleted rows is no longer available; report the
            // difference measured from the remaining rows below.
        }
        tx.execute("DELETE FROM feed_cache WHERE last_opened_at < ?1", [cutoff])?;
        tx.commit()?;

        let mut after: i64 = conn.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM cached_articles",
            [],
            |row| row.get(0),
        )?;
        if max_size_bytes > 0 && (after as u64) > max_size_bytes {
            let mut stmt = conn.prepare(
                "SELECT title, size_bytes FROM cached_articles
                 WHERE title NOT IN (SELECT title FROM bookmarks)
                   AND title NOT IN (SELECT title FROM reading_list_items)
                 ORDER BY last_opened_at ASC",
            )?;
            let victims = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
                })
                .map(|rows| rows.flatten().collect::<Vec<_>>())?;
            drop(stmt);
            for (title, size) in victims {
                if (after as u64) <= max_size_bytes {
                    break;
                }
                conn.execute("DELETE FROM cached_articles WHERE title = ?1", [&title])?;
                conn.execute("DELETE FROM article_fts WHERE title = ?1", [&title])?;
                after -= size.max(0);
                removed_bytes += size.max(0) as u64;
            }
        }
        if before > after {
            removed_bytes += (before - after) as u64;
        }
        Ok(removed_bytes)
    }
}

fn compress<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let json = serde_json::to_vec(value)?;
    let compressed = zstd::stream::encode_all(Cursor::new(json), 9)?;
    Ok(compressed)
}

fn decompress<T: DeserializeOwned>(blob: &[u8]) -> Result<T> {
    let raw = zstd::bulk::decompress(blob, MAX_BLOB_BYTES)?;
    Ok(serde_json::from_slice(&raw)?)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{Block, Inline};

    fn cache() -> (Cache, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("temp");
        let cache = Cache::open_at(dir.path().join("test.db")).expect("cache");
        (cache, dir)
    }

    fn article(title: &str, body: &str) -> Article {
        Article {
            title: title.to_string(),
            display_title: title.to_string(),
            pageid: Some(7),
            revision: Some(3),
            language: "en".to_string(),
            html: format!("<p>{body}</p>"),
            summary: body.to_string(),
            blocks: vec![Block::Paragraph(vec![Inline::plain(body)])],
            plain_text: body.to_string(),
            media: Vec::new(),
            fetched_at: now(),
            source: Source::Network,
        }
    }

    #[test]
    fn article_roundtrip_is_compressed_and_marks_cache_source() {
        let (cache, _dir) = cache();
        let body = "terminal reading ".repeat(200);
        let size = cache.put_article(&article("Rust", &body)).expect("put");
        let raw = body.len() as u64 * 4;
        assert!(size < raw, "compressed payload should be smaller");
        assert!(cache.touch_article("Rust").unwrap());
        let loaded = cache.get_article("Rust").unwrap().expect("article");
        assert_eq!(loaded.plain_text, body);
        assert_eq!(loaded.source, Source::Cache);
        assert!(cache.has_article("rust").unwrap());
    }

    #[test]
    fn lists_bookmarks_history_and_search() {
        let (cache, _dir) = cache();
        cache
            .put_article(&article("Ferris", "a small crab"))
            .unwrap();
        let list = cache.create_reading_list("Systems").unwrap();
        cache.add_to_list(list.id, "Ferris").unwrap();
        assert_eq!(cache.reading_lists().unwrap()[0].item_count, 1);
        assert_eq!(cache.list_items(list.id).unwrap(), vec!["Ferris"]);
        assert!(cache.toggle_bookmark("Ferris").unwrap());
        assert!(cache.is_bookmarked("Ferris").unwrap());
        cache.record_history("Ferris").unwrap();
        cache.record_history("Ferris").unwrap();
        assert_eq!(cache.history().unwrap()[0].visits, 2);
        let hits = cache.local_search("crab", 5).unwrap();
        assert_eq!(hits[0].title, "Ferris");
        cache.remove_from_list(list.id, "Ferris").unwrap();
        assert!(cache.list_items(list.id).unwrap().is_empty());
    }

    #[test]
    fn purge_keeps_bookmarked_and_listed_articles() {
        let (cache, _dir) = cache();
        cache.put_article(&article("Keep", "pinned")).unwrap();
        cache.put_article(&article("Also", "listed")).unwrap();
        cache.put_article(&article("Drop", "expired")).unwrap();
        cache.toggle_bookmark("Keep").unwrap();
        let list = cache.create_reading_list("Keepers").unwrap();
        cache.add_to_list(list.id, "Also").unwrap();
        // Force every row to look old.
        cache
            .conn()
            .execute("UPDATE cached_articles SET last_opened_at = 0", [])
            .unwrap();
        cache.purge(1, u64::MAX).unwrap();
        assert!(cache.has_article("Keep").unwrap());
        assert!(cache.has_article("Also").unwrap());
        assert!(!cache.has_article("Drop").unwrap());
    }

    #[test]
    fn touch_extends_the_sliding_ttl() {
        let (cache, _dir) = cache();
        cache.put_article(&article("Touch", "still here")).unwrap();
        cache
            .conn()
            .execute("UPDATE cached_articles SET last_opened_at = 0", [])
            .unwrap();
        cache.purge(3, u64::MAX).unwrap();
        assert!(!cache.has_article("Touch").unwrap());

        cache.put_article(&article("Touch", "still here")).unwrap();
        cache
            .conn()
            .execute("UPDATE cached_articles SET last_opened_at = 0", [])
            .unwrap();
        assert!(cache.touch_article("Touch").unwrap());
        cache.purge(3, u64::MAX).unwrap();
        assert!(cache.has_article("Touch").unwrap());
    }

    #[test]
    fn feed_roundtrip() {
        let (cache, _dir) = cache();
        let date = NaiveDate::from_ymd_opt(2026, 10, 26).unwrap();
        let feed = Feed {
            date,
            sections: Vec::new(),
            fetched_at: now(),
            source: Source::Network,
        };
        cache.put_feed(&feed).unwrap();
        let loaded = cache.get_feed(date).unwrap().unwrap();
        assert_eq!(loaded.date, date);
        assert_eq!(loaded.source, Source::Cache);
    }
}
