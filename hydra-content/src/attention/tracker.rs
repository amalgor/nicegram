use anyhow::{Context, Result};
use crate::models::{AttentionEvent, InteractionType};
use chrono::Utc;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::info;

/// Persistent store for attention events and content cache.
/// Uses JSON file storage to avoid sqlite dependency conflicts with grammers-session's libsql.
#[derive(serde::Serialize, serde::Deserialize, Default)]
struct StoreData {
    events: Vec<AttentionEvent>,
    content_cache: HashMap<String, CachedContent>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct CachedContent {
    chat_id: i64,
    content_json: String,
    created_at: String,
}

/// Tracks user attention events: what was expanded, how long was read, etc.
/// Persists data to a JSON file on the device.
pub struct AttentionTracker {
    path: PathBuf,
    data: Mutex<StoreData>,
}

impl AttentionTracker {
    /// Open or create the attention tracking store at the given path.
    pub fn new(db_path: &Path) -> Result<Self> {
        let data = if db_path.exists() {
            let content = std::fs::read_to_string(db_path)
                .context("Failed to read attention tracker file")?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            StoreData::default()
        };

        info!("Attention tracker initialized at {}", db_path.display());

        Ok(Self {
            path: db_path.to_path_buf(),
            data: Mutex::new(data),
        })
    }

    fn save(&self, data: &StoreData) -> Result<()> {
        let json = serde_json::to_string(data)
            .context("Failed to serialize attention data")?;
        std::fs::write(&self.path, json)
            .context("Failed to write attention tracker file")?;
        Ok(())
    }

    /// Record an attention event (user expanded content, read it, etc.)
    pub fn record_event(&self, event: &AttentionEvent) -> Result<()> {
        let mut data = self.data.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        data.events.push(event.clone());
        self.save(&data)
    }

    /// Cache a processed message's content tree as JSON for quick retrieval.
    pub fn cache_content(&self, message_id: &str, chat_id: i64, content_json: &str) -> Result<()> {
        let mut data = self.data.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        data.content_cache.insert(message_id.to_string(), CachedContent {
            chat_id,
            content_json: content_json.to_string(),
            created_at: Utc::now().to_rfc3339(),
        });
        self.save(&data)
    }

    /// Get cached content JSON for a message. Returns None if not cached.
    pub fn get_cached_content(&self, message_id: &str) -> Result<Option<String>> {
        let data = self.data.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        Ok(data.content_cache.get(message_id).map(|c| c.content_json.clone()))
    }

    /// Get attention statistics for a chat.
    pub fn get_chat_stats(&self, chat_id: i64) -> Result<ChatAttentionStats> {
        let data = self.data.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let chat_events: Vec<&AttentionEvent> = data.events.iter()
            .filter(|e| e.chat_id == chat_id)
            .collect();

        let total = chat_events.len() as u64;
        let avg_read = if total > 0 {
            chat_events.iter().map(|e| e.total_read_time_ms).sum::<u64>() / total
        } else { 0 };
        let avg_depth = if total > 0 {
            chat_events.iter().map(|e| e.max_depth_reached as f32).sum::<f32>() / total as f32
        } else { 0.0 };
        let deep_dives = chat_events.iter()
            .filter(|e| matches!(e.interaction, InteractionType::DeepDive))
            .count() as u64;

        Ok(ChatAttentionStats {
            chat_id,
            total_events: total,
            avg_read_time_ms: avg_read,
            avg_depth,
            deep_dive_count: deep_dives,
        })
    }

    /// Evict cached content older than the given TTL (in seconds).
    pub fn evict_stale_cache(&self, ttl_seconds: u64) -> Result<u64> {
        let mut data = self.data.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        let cutoff = Utc::now() - chrono::Duration::seconds(ttl_seconds as i64);
        let cutoff_str = cutoff.to_rfc3339();
        let before = data.content_cache.len();
        data.content_cache.retain(|_, v| v.created_at >= cutoff_str);
        let deleted = before - data.content_cache.len();
        if deleted > 0 {
            self.save(&data)?;
        }
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
