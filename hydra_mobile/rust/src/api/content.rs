use std::sync::Arc;
use tokio::sync::Mutex;
use lazy_static::lazy_static;
use hydra_content::ContentEngine;
use hydra_content::models::{AttentionEvent, InteractionType};
use hydra_content::telegram::client::AuthState;

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
            anyhow::anyhow!("Hydra node not started yet. Start the node before initializing content engine.")
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
    let engine = guard.as_mut().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;

    engine.tg_client.send_phone(&phone).await?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Send login code for Telegram auth. Returns auth state.
pub async fn telegram_send_code(code: String) -> anyhow::Result<String> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard.as_mut().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;

    engine.tg_client.send_code(&code).await?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Send 2FA password. Returns auth state.
pub async fn telegram_send_password(password: String) -> anyhow::Result<String> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard.as_mut().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;

    engine.tg_client.send_password(&password).await?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Get Telegram auth state as a string.
pub async fn telegram_auth_state() -> anyhow::Result<String> {
    let guard = CONTENT_ENGINE.lock().await;
    let engine = guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;
    Ok(auth_state_to_string(engine.tg_client.state()))
}

/// Get list of Telegram dialogs (chats, groups, channels).
/// Returns JSON array of TrackedChat objects.
pub async fn telegram_get_dialogs() -> anyhow::Result<String> {
    let guard = CONTENT_ENGINE.lock().await;
    let engine = guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;

    let client = engine.tg_client.client().ok_or_else(|| {
        anyhow::anyhow!("Telegram not connected.")
    })?;

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
    let engine = guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;

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
    let engine = guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;

    let stats = engine.tracker.get_chat_stats(chat_id)?;
    Ok(serde_json::to_string(&stats)?)
}

/// Disconnect from Telegram.
pub async fn telegram_disconnect() -> anyhow::Result<()> {
    let mut guard = CONTENT_ENGINE.lock().await;
    let engine = guard.as_mut().ok_or_else(|| {
        anyhow::anyhow!("Content engine not initialized.")
    })?;

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
