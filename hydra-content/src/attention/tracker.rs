use anyhow::{Context, Result};
use crate::models::{AttentionEvent, InteractionType};
use chrono::Utc;
use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;
use tracing::info;

/// Tracks user attention events: what was expanded, how long was read, etc.
/// Stores data in a local SQLite database on the device.
pub struct AttentionTracker {
    db: Mutex<Connection>,
}

impl AttentionTracker {
    /// Open or create the attention tracking database at the given path.
    pub fn new(db_path: &Path) -> Result<Self> {
        let conn = Connection::open(db_path)
            .context("Failed to open attention tracking database")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS attention_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id TEXT NOT NULL,
                chat_id INTEGER NOT NULL,
                max_depth_reached INTEGER NOT NULL,
                total_read_time_ms INTEGER NOT NULL,
                interaction TEXT NOT NULL,
                timestamp TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_attention_chat ON attention_events(chat_id);
            CREATE INDEX IF NOT EXISTS idx_attention_ts ON attention_events(timestamp);

            CREATE TABLE IF NOT EXISTS content_cache (
                message_id TEXT PRIMARY KEY,
                chat_id INTEGER NOT NULL,
                content_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_cache_chat ON content_cache(chat_id);
            "
        ).context("Failed to create attention tracking tables")?;

        info!("Attention tracker database initialized at {}", db_path.display());

        Ok(Self {
            db: Mutex::new(conn),
        })
    }

    /// Record an attention event (user expanded content, read it, etc.)
    pub fn record_event(&self, event: &AttentionEvent) -> Result<()> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
        db.execute(
            "INSERT INTO attention_events (message_id, chat_id, max_depth_reached, total_read_time_ms, interaction, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                event.message_id,
                event.chat_id,
                event.max_depth_reached,
                event.total_read_time_ms,
                interaction_to_str(event.interaction),
                event.timestamp.to_rfc3339(),
            ],
        ).context("Failed to insert attention event")?;
        Ok(())
    }

    /// Cache a processed message's content tree as JSON for quick retrieval.
    pub fn cache_content(&self, message_id: &str, chat_id: i64, content_json: &str) -> Result<()> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
        db.execute(
            "INSERT OR REPLACE INTO content_cache (message_id, chat_id, content_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                message_id,
                chat_id,
                content_json,
                Utc::now().to_rfc3339(),
            ],
        ).context("Failed to cache content")?;
        Ok(())
    }

    /// Get cached content JSON for a message. Returns None if not cached.
    pub fn get_cached_content(&self, message_id: &str) -> Result<Option<String>> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
        let mut stmt = db.prepare(
            "SELECT content_json FROM content_cache WHERE message_id = ?1"
        )?;
        let result = stmt.query_row(rusqlite::params![message_id], |row| {
            row.get::<_, String>(0)
        });
        match result {
            Ok(json) => Ok(Some(json)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get attention statistics for a chat: total events, avg read time, depth distribution.
    pub fn get_chat_stats(&self, chat_id: i64) -> Result<ChatAttentionStats> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;

        let total_events: i64 = db.query_row(
            "SELECT COUNT(*) FROM attention_events WHERE chat_id = ?1",
            rusqlite::params![chat_id],
            |row| row.get(0),
        )?;

        let avg_read_time: f64 = db.query_row(
            "SELECT COALESCE(AVG(total_read_time_ms), 0) FROM attention_events WHERE chat_id = ?1",
            rusqlite::params![chat_id],
            |row| row.get(0),
        )?;

        let avg_depth: f64 = db.query_row(
            "SELECT COALESCE(AVG(max_depth_reached), 0) FROM attention_events WHERE chat_id = ?1",
            rusqlite::params![chat_id],
            |row| row.get(0),
        )?;

        let deep_dive_count: i64 = db.query_row(
            "SELECT COUNT(*) FROM attention_events WHERE chat_id = ?1 AND interaction = 'deep_dive'",
            rusqlite::params![chat_id],
            |row| row.get(0),
        )?;

        Ok(ChatAttentionStats {
            chat_id,
            total_events: total_events as u64,
            avg_read_time_ms: avg_read_time as u64,
            avg_depth: avg_depth as f32,
            deep_dive_count: deep_dive_count as u64,
        })
    }

    /// Evict cached content older than the given TTL (in seconds).
    pub fn evict_stale_cache(&self, ttl_seconds: u64) -> Result<u64> {
        let db = self.db.lock().map_err(|e| anyhow::anyhow!("DB lock poisoned: {}", e))?;
        let cutoff = Utc::now() - chrono::Duration::seconds(ttl_seconds as i64);
        let deleted = db.execute(
            "DELETE FROM content_cache WHERE created_at < ?1",
            rusqlite::params![cutoff.to_rfc3339()],
        )?;
        Ok(deleted as u64)
    }
}

/// Aggregated attention statistics for a single chat.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatAttentionStats {
    pub chat_id: i64,
    pub total_events: u64,
    pub avg_read_time_ms: u64,
    pub avg_depth: f32,
    pub deep_dive_count: u64,
}

fn interaction_to_str(interaction: InteractionType) -> &'static str {
    match interaction {
        InteractionType::Skim => "skim",
        InteractionType::Read => "read",
        InteractionType::DeepDive => "deep_dive",
        InteractionType::Share => "share",
    }
}
