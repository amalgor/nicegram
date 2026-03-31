use flutter_rust_bridge::frb;
use hydra_ai::AiNegotiator;
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub description: String,
    pub size_mb: u32,
    pub download_url: String,
    pub is_downloaded: bool,
}

pub struct ModelManager {
    client: Client,
    models_dir: PathBuf,
}

lazy_static::lazy_static! {
    static ref MANAGER: Arc<Mutex<Option<ModelManager>>> = Arc::new(Mutex::new(None));
    /// Shared AI negotiator reference for model hot-reloading from mobile UI
    pub static ref SHARED_AI: Arc<Mutex<Option<Arc<AiNegotiator>>>> = Arc::new(Mutex::new(None));
}

#[frb(sync)]
pub fn init_model_manager(base_dir: String) -> anyhow::Result<()> {
    crate::api::shared_state::init_shared_base_dir(&base_dir)?;
    let models_dir = PathBuf::from(&base_dir).join("models");
    std::fs::create_dir_all(&models_dir)?;

    let manager = ModelManager {
        client: Client::new(),
        models_dir,
    };

    let mut m = MANAGER.blocking_lock();
    *m = Some(manager);
    Ok(())
}

pub async fn get_available_models() -> anyhow::Result<Vec<ModelInfo>> {
    let m = MANAGER.lock().await;
    let manager = m.as_ref().expect("ModelManager not initialized");

    let mut models = vec![
        ModelInfo {
            id: "qwen2.5-0.5b".to_string(),
            name: "Qwen 2.5 (0.5B)".to_string(),
            description: "Сверхлегкая модель для максимальной экономии батареи.".to_string(),
            size_mb: 468,
            download_url: "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf".to_string(),
            is_downloaded: false,
        },
        ModelInfo {
            id: "qwen3.5-0.8b".to_string(),
            name: "Qwen 3.5 (0.8B)".to_string(),
            description: "Современная модель. Оптимальный баланс между скоростью маршрутизации и качеством решений.".to_string(),
            size_mb: 507,
            download_url: "https://huggingface.co/unsloth/Qwen3.5-0.8B-GGUF/resolve/main/Qwen3.5-0.8B-Q4_K_M.gguf".to_string(),
            is_downloaded: false,
        },
        ModelInfo {
            id: "qwen2.5-1.5b".to_string(),
            name: "Qwen 2.5 (1.5B)".to_string(),
            description: "Продвинутая модель. Рекомендуется для устройств с большим объемом оперативной памяти (>8 ГБ).".to_string(),
            size_mb: 1120,
            download_url: "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf".to_string(),
            is_downloaded: false,
        }
    ];

    for model in models.iter_mut() {
        let path = manager.models_dir.join(format!("{}.gguf", model.id));
        if path.exists() {
            model.is_downloaded = true;
        }
    }

    Ok(models)
}

use crate::frb_generated::StreamSink;

pub async fn download_model(id: String, progress: StreamSink<f64>) -> anyhow::Result<()> {
    let (client, models_dir) = {
        let m = MANAGER.lock().await;
        let manager = m.as_ref().expect("ModelManager not initialized");
        (manager.client.clone(), manager.models_dir.clone())
    };

    let models = get_available_models().await?;
    let model = models
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow::anyhow!("Model not found"))?;

    let path = models_dir.join(format!("{}.gguf", model.id));
    let mut file = tokio::fs::File::create(&path).await?;

    let mut res = client.get(&model.download_url).send().await?;
    let total_size = res
        .content_length()
        .unwrap_or(model.size_mb as u64 * 1024 * 1024);

    let mut downloaded: u64 = 0;
    while let Some(chunk) = res.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let _ = progress.add((downloaded as f64 / total_size as f64) * 100.0);
    }

    Ok(())
}

pub async fn set_active_model(id: String) -> anyhow::Result<()> {
    let models_dir = {
        let m = MANAGER.lock().await;
        m.as_ref()
            .expect("ModelManager not initialized. Call init_model_manager first.")
            .models_dir
            .clone()
    };

    let model_path = models_dir.join(format!("{}.gguf", id));
    if !model_path.exists() {
        return Err(anyhow::anyhow!(
            "Model '{}' not downloaded. Download it first via the AI Models screen.",
            id
        ));
    }

    let ai_guard = SHARED_AI.lock().await;
    let ai = ai_guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Hydra node not started yet. Start the node before switching models.")
    })?;

    tracing::info!("Reloading AI model: {} from {}", id, model_path.display());
    ai.load_model(model_path.clone()).await?;
    tracing::info!("AI model switched to: {}", model_path.display());

    Ok(())
}
