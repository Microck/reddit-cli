use crate::{
    backend::SourceKind,
    config::{Paths, ensure_private_dir, file_error, remove_if_present},
    error::{AppError, ErrorCode},
    model::{Item, now},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use serde_json::{Map, Value};
use std::{
    fs::{File, OpenOptions, TryLockError},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

const SCHEMA_VERSION: &str = "1";
const SCHEMA: &str = "
CREATE TABLE meta(key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE http_cache(
    key TEXT PRIMARY KEY, body BLOB NOT NULL, fetched_at INT NOT NULL, ttl INT NOT NULL
);
CREATE TABLE rate_limit(host TEXT PRIMARY KEY, remaining REAL NOT NULL,
                       reset_at INT NOT NULL, window TEXT);
CREATE TABLE items(
    fullname TEXT PRIMARY KEY, kind TEXT NOT NULL, id TEXT NOT NULL,
    subreddit TEXT NOT NULL, author TEXT NOT NULL, title TEXT, body TEXT,
    link_id TEXT, parent_id TEXT, score INT, created_utc INT NOT NULL,
    permalink TEXT NOT NULL, json TEXT NOT NULL, source TEXT NOT NULL, fetched_at INT NOT NULL
);
CREATE INDEX items_fetched_at ON items(fetched_at);
CREATE INDEX http_cache_fetched_at ON http_cache(fetched_at);
CREATE VIRTUAL TABLE items_fts USING fts5(
    title, body, author, subreddit, content='items', content_rowid='rowid'
);
CREATE TRIGGER items_insert AFTER INSERT ON items BEGIN
    INSERT INTO items_fts(rowid, title, body, author, subreddit)
    VALUES (new.rowid, new.title, new.body, new.author, new.subreddit);
END;
CREATE TRIGGER items_delete AFTER DELETE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, body, author, subreddit)
    VALUES ('delete', old.rowid, old.title, old.body, old.author, old.subreddit);
END;
CREATE TRIGGER items_update AFTER UPDATE ON items BEGIN
    INSERT INTO items_fts(items_fts, rowid, title, body, author, subreddit)
    VALUES ('delete', old.rowid, old.title, old.body, old.author, old.subreddit);
    INSERT INTO items_fts(rowid, title, body, author, subreddit)
    VALUES (new.rowid, new.title, new.body, new.author, new.subreddit);
END;
";

#[derive(Clone, Debug, Serialize)]
pub struct RateLimit {
    pub host: String,
    pub remaining: f64,
    pub reset_at: i64,
    pub window: Option<String>,
}

#[derive(Debug)]
pub struct CacheEntry {
    pub body: Vec<u8>,
    pub fetched_at: i64,
}

#[derive(Debug, Serialize)]
pub struct Counts {
    pub http_cache: u64,
    pub items: u64,
}

#[derive(Debug, Serialize)]
pub struct Stats {
    pub http_cache: u64,
    pub items: u64,
    pub posts: u64,
    pub comments: u64,
}

pub struct Store {
    connection: Mutex<Connection>,
    // Clearing an open SQLite database would split readers across two WALs.
    _file_lock: Option<File>,
}

impl Store {
    pub fn open(paths: &Paths, retention: i64) -> Result<Self, AppError> {
        let file_lock = store_lock(paths)?;
        file_lock
            .lock_shared()
            .map_err(|error| file_error(&paths.data_dir, error))?;
        let connection = Connection::open(paths.data_dir.join("store.sqlite")).map_err(db_error)?;
        Self::initialize(connection, retention, Some(file_lock))
    }

    pub fn in_memory() -> Result<Self, AppError> {
        Self::initialize(
            Connection::open_in_memory().map_err(db_error)?,
            30 * 86_400,
            None,
        )
    }

    fn initialize(
        mut connection: Connection,
        retention: i64,
        file_lock: Option<File>,
    ) -> Result<Self, AppError> {
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .pragma_update(None, "journal_mode", "WAL")
            .map_err(db_error)?;
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let tables: i64 = tx.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            [], |row| row.get(0),
        ).map_err(db_error)?;
        if tables == 0 {
            tx.execute_batch(SCHEMA).map_err(db_error)?;
            tx.execute(
                "INSERT INTO meta VALUES ('schema_version', ?1)",
                [SCHEMA_VERSION],
            )
            .map_err(db_error)?;
        }
        let version = tx.query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        );
        if !matches!(version.as_deref(), Ok(SCHEMA_VERSION)) {
            return Err(AppError::new(
                ErrorCode::EStoreSchema,
                "local store schema does not match",
            )
            .with_hint("Run reddit cache clear to create a new store."));
        }
        let now = now();
        let last_prune = tx
            .query_row(
                "SELECT CAST(value AS INT) FROM meta WHERE key = 'last_prune'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?;
        if last_prune.is_none_or(|last| now.saturating_sub(last) >= 3600) {
            prune_before(&tx, now.saturating_sub(retention))?;
            tx.execute(
                "INSERT OR REPLACE INTO meta VALUES ('last_prune', ?1)",
                [now],
            )
            .map_err(db_error)?;
        }
        tx.commit().map_err(db_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            _file_lock: file_lock,
        })
    }

    pub fn clear(paths: &Paths) -> Result<(), AppError> {
        let lock = store_lock(paths)?;
        match lock.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(AppError::new(ErrorCode::EConfig, "store is in use"));
            }
            Err(TryLockError::Error(error)) => return Err(file_error(&paths.data_dir, error)),
        }
        for name in [
            "token.json",
            "store.sqlite",
            "store.sqlite-wal",
            "store.sqlite-shm",
        ] {
            remove_if_present(&paths.data_dir.join(name))?;
        }
        Ok(())
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .expect("store connection lock poisoned")
    }

    pub fn read_cache(&self, key: &str, ignore_ttl: bool) -> Result<Option<CacheEntry>, AppError> {
        self.connection()
            .query_row(
                "SELECT body, fetched_at FROM http_cache
                 WHERE key = ?1 AND (?2 OR fetched_at + ttl > ?3)",
                params![key, ignore_ttl, now()],
                |row| {
                    Ok(CacheEntry {
                        body: row.get(0)?,
                        fetched_at: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    pub fn write_cache(
        &self,
        key: &str,
        body: &[u8],
        fetched_at: i64,
        ttl: i64,
    ) -> Result<(), AppError> {
        // A response may arrive after another command committed a tombstone.
        // Match archive IDs as well as live fullnames before writing the raw body.
        self.connection().execute(
            "INSERT INTO http_cache
             SELECT ?1, ?2, ?3, ?4 WHERE NOT EXISTS (
               SELECT 1 FROM json_tree(CAST(?2 AS TEXT)) AS field
               JOIN items ON items.fullname IN (field.atom, 't3_' || field.atom, 't1_' || field.atom)
               WHERE field.key IN ('id', 'name') AND items.body IN ('[deleted]', '[removed]')
             )
             ON CONFLICT(key) DO UPDATE SET body = excluded.body, fetched_at = excluded.fetched_at, ttl = excluded.ttl
             WHERE excluded.fetched_at >= http_cache.fetched_at",
            params![key, body, fetched_at, ttl],
        ).map_err(db_error)?;
        Ok(())
    }

    pub fn rate_limit(&self, host: &str) -> Result<Option<RateLimit>, AppError> {
        self.connection()
            .query_row(
                "SELECT remaining, reset_at, window FROM rate_limit WHERE host = ?1",
                [host],
                |row| {
                    Ok(RateLimit {
                        host: host.into(),
                        remaining: row.get(0)?,
                        reset_at: row.get(1)?,
                        window: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    pub fn record_rate_limit(&self, rate: &RateLimit) -> Result<(), AppError> {
        self.connection().execute(
            "INSERT INTO rate_limit VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(host) DO UPDATE SET
               remaining = excluded.remaining, reset_at = excluded.reset_at, window = excluded.window",
            params![rate.host, rate.remaining, rate.reset_at, rate.window],
        ).map_err(db_error)?;
        Ok(())
    }

    /// Store tagged records from a command payload. Embedded comments become separate rows,
    /// so an update to one comment also removes its previous body from local search results.
    pub fn upsert(
        &self,
        payload: &Value,
        source: SourceKind,
        fetched_at: i64,
    ) -> Result<(), AppError> {
        let mut connection = self.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO items VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                 ON CONFLICT(fullname) DO UPDATE SET
                   kind = excluded.kind, id = excluded.id, subreddit = excluded.subreddit,
                   author = excluded.author, title = excluded.title, body = excluded.body,
                   link_id = excluded.link_id, parent_id = excluded.parent_id, score = excluded.score,
                   created_utc = excluded.created_utc, permalink = excluded.permalink,
                   json = excluded.json, source = excluded.source, fetched_at = excluded.fetched_at
                 WHERE NOT coalesce(items.body IN ('[deleted]', '[removed]'), 0)
                   AND (excluded.fetched_at >= items.fetched_at OR excluded.body IN ('[deleted]', '[removed]'))",
            ).map_err(db_error)?;
            let mut purge = tx
                .prepare(
                    "DELETE FROM http_cache WHERE instr(body, ?1) > 0
                 OR (key LIKE 'archive|%' AND instr(body, json_quote(?2)) > 0)",
                )
                .map_err(db_error)?;
            let mut pending = vec![payload];
            while let Some(record) = pending.pop() {
                if let Some(items) = record.as_array() {
                    pending.extend(items);
                    continue;
                }
                let Some(kind) = record["kind"].as_str() else {
                    pending.extend(
                        ["post", "comments", "items"]
                            .into_iter()
                            .filter_map(|field| record.get(field)),
                    );
                    continue;
                };
                let (title_field, body_field) = match kind {
                    "post" => (Some("title"), "selftext"),
                    "comment" => (None, "body"),
                    _ => continue,
                };
                let object = record.as_object().expect("tagged item is an object");
                pending.extend(
                    ["top_comments", "replies"]
                        .into_iter()
                        .filter_map(|field| object.get(field)),
                );
                let mut flat: Map<String, Value> = object
                    .iter()
                    .filter(|(key, _)| !matches!(key.as_str(), "top_comments" | "replies"))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                if kind == "comment" {
                    flat.insert("replies".into(), Value::Array(Vec::new()));
                }
                if let Some(marker) = match record["state"].as_str() {
                    Some("deleted") => Some("[deleted]"),
                    Some("removed") => Some("[removed]"),
                    _ => None,
                } {
                    flat.insert(body_field.into(), Value::from(marker));
                    if let Some(title_field) = title_field {
                        flat.insert(title_field.into(), Value::from(marker));
                    }
                }
                let json = serde_json::to_string(&flat).expect("JSON values serialize");
                let title = title_field.and_then(|field| flat[field].as_str());
                let body = flat[body_field].as_str();
                let changed = stmt
                    .execute(params![
                        record["fullname"].as_str(),
                        kind,
                        record["id"].as_str(),
                        record["subreddit"].as_str(),
                        record["author"].as_str(),
                        title,
                        body,
                        record["link_id"].as_str(),
                        record["parent_id"].as_str(),
                        record["score"].as_i64(),
                        record["created_utc"].as_i64(),
                        record["permalink"].as_str(),
                        json,
                        source.as_str(),
                        fetched_at,
                    ])
                    .map_err(db_error)?;
                if changed > 0 && matches!(body, Some("[deleted]" | "[removed]")) {
                    purge
                        .execute(params![record["fullname"].as_str(), record["id"].as_str()])
                        .map_err(db_error)?;
                }
            }
        }
        tx.commit().map_err(db_error)
    }

    pub fn search(
        &self,
        query: &str,
        sub: Option<&str>,
        author: Option<&str>,
        kind: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Item>, AppError> {
        let connection = self.connection();
        let mut stmt = connection.prepare(
            "SELECT items.json FROM items_fts JOIN items ON items.rowid = items_fts.rowid
             WHERE items_fts MATCH ?1 AND (?2 IS NULL OR items.subreddit = ?2 COLLATE NOCASE)
               AND (?3 IS NULL OR items.author = ?3 COLLATE NOCASE) AND (?4 IS NULL OR items.kind = ?4)
             ORDER BY bm25(items_fts), items.fullname LIMIT ?5",
        ).map_err(db_error)?;
        let rows = stmt
            .query_map(params![query, sub, author, kind, limit], |row| {
                row.get::<_, String>(0)
            })
            .map_err(db_error)?;
        rows.map(|row| {
            let json = row.map_err(|error| match error {
                rusqlite::Error::SqliteFailure(failure, Some(message))
                    if failure.code == rusqlite::ErrorCode::Unknown =>
                {
                    AppError::usage(message)
                }
                other => db_error(other),
            })?;
            serde_json::from_str(&json)
                .map_err(|_| AppError::new(ErrorCode::EConfig, "invalid item in local store"))
        })
        .collect()
    }

    pub fn stats(&self) -> Result<Stats, AppError> {
        self.connection()
            .query_row(
                "SELECT (SELECT count(*) FROM http_cache), count(*),
                    count(CASE WHEN kind = 'post' THEN 1 END),
                    count(CASE WHEN kind = 'comment' THEN 1 END) FROM items",
                [],
                |row| {
                    Ok(Stats {
                        http_cache: row.get(0)?,
                        items: row.get(1)?,
                        posts: row.get(2)?,
                        comments: row.get(3)?,
                    })
                },
            )
            .map_err(db_error)
    }

    pub fn prune(&self, age: i64) -> Result<Counts, AppError> {
        let mut connection = self.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let counts = prune_before(&tx, now().saturating_sub(age))?;
        tx.commit().map_err(db_error)?;
        Ok(counts)
    }
}

fn prune_before(connection: &Connection, cutoff: i64) -> Result<Counts, AppError> {
    Ok(Counts {
        http_cache: connection
            .execute("DELETE FROM http_cache WHERE fetched_at < ?1", [cutoff])
            .map_err(db_error)? as u64,
        items: connection
            .execute("DELETE FROM items WHERE fetched_at < ?1", [cutoff])
            .map_err(db_error)? as u64,
    })
}

fn store_lock(paths: &Paths) -> Result<File, AppError> {
    ensure_private_dir(&paths.data_dir)?;
    let path = paths.data_dir.join("store.lock");
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| file_error(&path, error))
}

fn db_error(error: rusqlite::Error) -> AppError {
    AppError::new(ErrorCode::EConfig, format!("local store: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Comment, Post, State};
    use serde_json::json;

    #[test]
    fn fts_updates_and_removes_deleted_text_from_nested_and_flat_records() {
        let store = Store::in_memory().unwrap();
        let comment = Comment {
            id: "cd".into(),
            fullname: "t1_cd".into(),
            body: "sensitive phrase".into(),
            ..Comment::default()
        };
        let mut post = Post {
            id: "ab".into(),
            fullname: "t3_ab".into(),
            title: "sensitive title".into(),
            selftext: "sensitive post".into(),
            top_comments: Some(vec![comment.clone()]),
            ..Post::default()
        };
        store.upsert(&json!(post), SourceKind::Live, now()).unwrap();
        let matches = store.search("sensitive", None, None, None, 10).unwrap();
        assert_eq!(matches.len(), 2);
        assert!(
            matches
                .iter()
                .any(|item| matches!(item, Item::Post(post) if post.top_comments.is_none()))
        );
        post.state = State::Removed;
        post.top_comments.as_mut().unwrap()[0].state = State::Deleted;
        store.upsert(&json!(post), SourceKind::Live, now()).unwrap();
        assert!(
            store
                .search("sensitive", None, None, None, 10)
                .unwrap()
                .is_empty()
        );
        let matches = store
            .search("removed OR deleted", None, None, None, 10)
            .unwrap();
        assert!(matches.iter().any(|item| matches!(item, Item::Post(post) if post.title == "[removed]" && post.selftext == "[removed]")));
        assert!(
            matches
                .iter()
                .any(|item| matches!(item, Item::Comment(comment) if comment.body == "[deleted]"))
        );
    }
}
