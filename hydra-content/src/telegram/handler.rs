use crate::models::{ContentNode, FoldLevel, ProcessedMessage, TrackedChat};
use crate::processing::summarizer::Summarizer;
use anyhow::Result;
use grammers_client::Client;
use grammers_client::client::UpdatesConfiguration;
use grammers_client::peer::Peer;
use grammers_client::update::Update;
use grammers_session::types::PeerRef;
use grammers_session::updates::UpdatesLike;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Handles incoming Telegram messages: fetches from chats, processes content.
pub struct MessageHandler {
    summarizer: Arc<Summarizer>,
}

fn peer_to_chat_id(peer: &Peer) -> i64 {
    peer.id().bare_id()
}

fn peer_is_private(peer: &Peer) -> bool {
    match peer {
        Peer::User(_) => true,
        Peer::Group(_) => true,
        Peer::Channel(ch) => ch.username().is_none(),
    }
}

impl MessageHandler {
    pub fn new(summarizer: Arc<Summarizer>) -> Self {
        Self { summarizer }
    }

    /// Fetch recent messages from a chat and process them into folded content.
    pub async fn fetch_and_process(
        &self,
        client: &Client,
        chat: PeerRef,
        chat_title: &str,
        limit: usize,
    ) -> Result<Vec<ProcessedMessage>> {
        let mut messages = client.iter_messages(chat);
        let mut results = Vec::new();
        let mut count = 0;

        while let Some(message) = messages.next().await? {
            if count >= limit {
                break;
            }

            let text = message.text().to_string();
            if text.is_empty() {
                count += 1;
                continue;
            }

            let sender_name = message
                .sender()
                .and_then(|s| s.name().map(|n| n.to_string()))
                .unwrap_or_else(|| "Unknown".to_string());

            let timestamp = message.date();

            let content_tree = if text.len() > 200 {
                self.summarizer.process(&text).await?
            } else {
                ContentNode::new(FoldLevel::Headline, text.clone())
            };

            let chat_id = message.peer_id().bare_id();

            results.push(ProcessedMessage {
                id: uuid::Uuid::new_v4().to_string(),
                chat_id,
                message_id: message.id(),
                chat_title: chat_title.to_string(),
                sender_name,
                timestamp,
                content_tree,
                is_private: false,
            });

            count += 1;
        }

        Ok(results)
    }

    /// Get the list of user's dialogs (chats, groups, channels) as TrackedChat entries.
    pub async fn get_dialogs(&self, client: &Client) -> Result<Vec<TrackedChat>> {
        let mut dialogs = client.iter_dialogs();
        let mut chats = Vec::new();

        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            let chat_id = peer_to_chat_id(&peer);
            let title = peer.name().unwrap_or("").trim().to_string();
            let title = if title.is_empty() {
                format!("Chat {}", chat_id)
            } else {
                title
            };

            chats.push(TrackedChat {
                chat_id,
                title,
                is_private: peer_is_private(&peer),
                processing_enabled: true,
            });
        }

        info!("Loaded {} dialogs from Telegram", chats.len());
        Ok(chats)
    }

    /// Subscribe to real-time updates and process new messages as they arrive.
    /// Runs indefinitely until the client disconnects.
    pub async fn listen_for_updates(
        &self,
        client: &Client,
        updates_rx: tokio::sync::mpsc::UnboundedReceiver<UpdatesLike>,
        on_message: impl Fn(ProcessedMessage) + Send + Sync,
    ) -> Result<()> {
        info!("Listening for new Telegram messages...");

        let mut stream = client
            .stream_updates(updates_rx, UpdatesConfiguration::default())
            .await;

        loop {
            let update = match stream.next().await {
                Ok(update) => update,
                Err(e) => {
                    error!("Error receiving update: {}", e);
                    continue;
                }
            };

            match update {
                Update::NewMessage(msg) if !msg.outgoing() => {
                    let text = msg.text().to_string();
                    if text.is_empty() {
                        continue;
                    }

                    let (chat_id, chat_title, is_private) = match msg.peer() {
                        Some(peer) => (
                            peer_to_chat_id(peer),
                            peer.name().unwrap_or("").to_string(),
                            peer_is_private(peer),
                        ),
                        None => (0, String::new(), false),
                    };

                    let sender_name = msg
                        .sender()
                        .and_then(|s| s.name().map(|n| n.to_string()))
                        .unwrap_or_else(|| "Unknown".to_string());

                    let timestamp = msg.date();

                    let content_tree = if text.len() > 200 {
                        match self.summarizer.process(&text).await {
                            Ok(tree) => tree,
                            Err(e) => {
                                warn!("Summarization failed, using raw text: {}", e);
                                ContentNode::new(FoldLevel::Headline, text.clone())
                            }
                        }
                    } else {
                        ContentNode::new(FoldLevel::Headline, text.clone())
                    };

                    let processed = ProcessedMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        chat_id,
                        message_id: msg.id(),
                        chat_title,
                        sender_name,
                        timestamp,
                        content_tree,
                        is_private,
                    };

                    debug!(
                        "Processed message from {} in {}",
                        processed.sender_name, processed.chat_title
                    );
                    on_message(processed);
                }
                _ => {}
            }
        }
    }
}
