pub mod attention;
pub mod models;
pub mod processing;
pub mod telegram;

use crate::attention::tracker::AttentionTracker;
use crate::processing::summarizer::Summarizer;
use crate::telegram::client::TelegramClient;
use crate::telegram::handler::MessageHandler;
use anyhow::Result;
use hydra_ai::AiNegotiator;
use hydra_config::HydraConfig;
use std::sync::Arc;
use tracing::info;

/// Central coordinator for the Content Intelligence subsystem.
/// Owns the Telegram client, summarizer, and attention tracker.
pub struct ContentEngine {
    pub tg_client: TelegramClient,
    pub handler: MessageHandler,
    pub tracker: AttentionTracker,
    pub summarizer: Arc<Summarizer>,
}

impl ContentEngine {
    /// Create a new ContentEngine from config and a shared AI negotiator.
    pub fn new(config: &HydraConfig, ai: Arc<AiNegotiator>) -> Result<Self> {
        let summarizer = Arc::new(Summarizer::new(ai, config.content.summarization_max_tokens));

        let tg_client = TelegramClient::new(config.telegram.clone());
        let handler = MessageHandler::new(summarizer.clone());
        let tracker = AttentionTracker::new(&config.content.db_path)?;

        info!("ContentEngine initialized");
        Ok(Self {
            tg_client,
            handler,
            tracker,
            summarizer,
        })
    }
}
