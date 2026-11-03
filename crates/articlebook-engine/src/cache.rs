use crate::error::EngineError;
use crate::model::Article;
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::path::Path;

pub struct ArticleCache {
    conn: Connection,
}

impl ArticleCache {
    pub fn open(path: &Path) -> Result<Self, EngineError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=4000;
             CREATE TABLE IF NOT EXISTS article_cache (
               url_hash TEXT PRIMARY KEY,
               url TEXT NOT NULL,
               title TEXT,
               ast_json TEXT NOT NULL,
               fetched_at TEXT NOT NULL
             );",
        )?;
        Ok(Self { conn })
    }

    pub fn get(&self, url: &str) -> Result<Option<Article>, EngineError> {
        let hash = cache_key(url);
        let result = self.conn.query_row(
            "SELECT ast_json FROM article_cache WHERE url_hash = ?1",
            params![hash],
            |row| row.get::<_, String>(0),
        );
        match result {
            Ok(json) => {
                let article = serde_json::from_str(&json)
                    .map_err(|err| EngineError::Message(format!("缓存损坏：{err}")))?;
                Ok(Some(article))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub fn put(&self, article: &Article) -> Result<(), EngineError> {
        let json = serde_json::to_string(article)
            .map_err(|err| EngineError::Message(format!("无法写入缓存：{err}")))?;
        let hash = cache_key(&article.source_url);
        let fetched = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO article_cache (url_hash, url, title, ast_json, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(url_hash) DO UPDATE SET
               url = excluded.url,
               title = excluded.title,
               ast_json = excluded.ast_json,
               fetched_at = excluded.fetched_at",
            params![hash, article.source_url, article.title, json, fetched],
        )?;
        Ok(())
    }
}

pub fn cache_key(url: &str) -> String {
    hex::encode(Sha256::digest(url.as_bytes()))
}
