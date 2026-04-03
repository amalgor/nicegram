use hydra_content::models::{AttentionEvent, InteractionType};
use hydra_content::telegram::client::AuthState;
use hydra_content::ContentEngine;
use lazy_static::lazy_static;
use std::sync::Arc;
use tokio::sync::Mutex;

lazy_static! {
    static ref CONTENT_ENGINE: Arc<Mutex<Option<ContentEngine>>> = Arc::new(Mutex::new(None));
}

/// Initialize the Content Engine. Called after start_hydra_node when config is ready.
pub async fn init_content_engine(base_dir: String) -> anyhow::Result<()> {
    let base_path = std::path::PathBuf::from(&base_dir);
    let config_path = base_path.join("hydra.toml");
    let config = hydra_config::HydraConfig::load_with_base_dir(&config_path, &base_path)?;

    let ai = {
        let guard = crate::api::model_manager::SHARED_AI.lock().await;
        guard.as_ref().cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "Hydra node not started yet. Start the node before initializing content engine."
            )
        })?
    };

    let engine = ContentEngine::new(&config, ai)?;
    let mut guard = CONTENT_ENGINE.lock().await;
    *guard = Some(engine);

    tracing::info!("Content engine initialized");
    Ok(())
}

/// Connect to Telegram. Returns the current auth state as a string.
pub async fn telegram_connect() -> anyhow::Result<String> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard.as_mut().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized. Call init_content_engine first.")
    })?;

    engine.tg_client.connect().await?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Send phone number for Telegram login. Returns auth state.
pub async fn telegram_send_phone(phone: String) -> anyhow::Result<String> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    engine.tg_client.send_phone(&phone).await?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Send login code for Telegram auth. Returns auth state.
pub async fn telegram_send_code(code: String) -> anyhow::Result<String> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    engine.tg_client.send_code(&code).await?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Send 2FA password. Returns auth state.
pub async fn telegram_send_password(password: String) -> anyhow::Result<String> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    engine.tg_client.send_password(&password).await?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Get Telegram auth state as a string.
pub async fn telegram_auth_state() -> anyhow::Result<String> {
    let guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Get list of Telegram dialogs (chats, groups, channels).
/// Returns JSON array of TrackedChat objects.
pub async fn telegram_get_dialogs() -> anyhow::Result<String> {
    let guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    let client = engine
        .tg_client
        .client()
        .ok_or_else(|| anyhow::anyhow!("Telegram not connected."))?;

    let dialogs = engine.handler.get_dialogs(client).await?;
    Ok(serde_json::to_string(&dialogs)?)
}

/// Record an attention event from the Flutter UI.
/// Called when user expands/reads content.
pub async fn record_attention(
    message_id: String,
    chat_id: i64,
    max_depth: u8,
    read_time_ms: u64,
    interaction: String,
) -> anyhow::Result<()> {
    let guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    let interaction_type = match interaction.as_str() {
        "skim" => InteractionType::Skim,
        "read" => InteractionType::Read,
        "deep_dive" => InteractionType::DeepDive,
        "share" => InteractionType::Share,
        _ => InteractionType::Skim,
    };

    engine.tracker.record_event(&AttentionEvent {
        message_id,
        chat_id,
        max_depth_reached: max_depth,
        total_read_time_ms: read_time_ms,
        interaction: interaction_type,
        timestamp: chrono::Utc::now(),
    })?;

    Ok(())
}

/// Get attention statistics for a chat. Returns JSON.
pub async fn get_chat_attention_stats(chat_id: i64) -> anyhow::Result<String> {
    let guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    let stats = engine.tracker.get_chat_stats(chat_id)?;
    Ok(serde_json::to_string(&stats)?)
}

/// Fetch messages from a specific chat and process them with summarization.
/// Returns JSON array of ProcessedMessage with fold levels.
pub async fn fetch_channel_messages(chat_id: i64, limit: i32) -> anyhow::Result<String> {
    let guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    let client = engine
        .tg_client
        .client()
        .ok_or_else(|| anyhow::anyhow!("Telegram not connected."))?;

    // Resolve chat_id to a PeerRef by iterating dialogs
    let mut dialogs = client.iter_dialogs();
    let mut target_peer = None;
    let mut target_title = String::new();

    while let Some(dialog) = dialogs.next().await? {
        let peer = dialog.peer();
        if peer.id().bare_id() == chat_id {
            target_title = peer.name().unwrap_or("").to_string();
            if target_title.is_empty() {
                target_title = format!("Chat {}", chat_id);
            }
            if let Some(pr) = peer.to_ref().await {
                target_peer = Some(pr);
            }
            break;
        }
    }

    let peer_ref = target_peer
        .ok_or_else(|| anyhow::anyhow!("Chat with id {} not found in dialogs", chat_id))?;

    let messages = engine
        .handler
        .fetch_and_process(client, peer_ref, &target_title, limit.max(1) as usize)
        .await?;

    Ok(serde_json::to_string(&messages)?)
}

/// Get a specific fold level content for a message.
/// level: 0=headline, 1=summary, 2=key_points, 3=full_text
/// Returns the content string for that level from the content tree.
pub fn get_message_fold_content(content_tree_json: String, level: u8) -> anyhow::Result<String> {
    use hydra_content::models::{ContentNode, FoldLevel};

    let root: ContentNode = serde_json::from_str(&content_tree_json)?;
    let target_level = FoldLevel::from_u8(level);

    fn find_level(node: &ContentNode, target: FoldLevel) -> Option<String> {
        if node.level == target {
            return Some(node.content.clone());
        }
        for child in &node.children {
            if let Some(found) = find_level(child, target) {
                return Some(found);
            }
        }
        None
    }

    find_level(&root, target_level)
        .ok_or_else(|| anyhow::anyhow!("Fold level {} not found in content tree", level))
}

/// Disconnect from Telegram.
pub async fn telegram_disconnect() -> anyhow::Result<()> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard
        .as_mut()
        .ok_or_else(|| anyhow::anyhow!("Content engine not initialized."))?;

    engine.tg_client.disconnect();
    Ok(())
}

fn auth_state_to_string(state: &AuthState) -> String {
    match state {
        AuthState::Disconnected => "disconnected".to_string(),
        AuthState::NeedPhone => "need_phone".to_string(),
        AuthState::NeedCode => "need_code".to_string(),
        AuthState::NeedPassword { hint } => format!("need_password:{}", hint),
        AuthState::Authorized { user_name } => format!("authorized:{}", user_name),
        AuthState::Error(e) => format!("error:{}", e),
    }
}

pub(crate) async fn telegram_anchor_info() -> anyhow::Result<Option<(i64, String)>> {
    let guard = CONTENT_ENGINE.lock().await;
    let Some(engine) = guard.as_ref() else {
        return Ok(None);
    };

    Ok(engine
        .tg_client
        .current_user_identity()
        .await?
        .map(|identity| (identity.user_id, identity.user_name)))
}
