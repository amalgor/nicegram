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

const MIN_VALID_MODEL_SIZE: u64 = 1024 * 1024;
const MODEL_CANDIDATE_FILES: &[(&str, &[&str])] = &[
    (
        "qwen2.5-0.5b",
        &[
            "qwen2.5-0.5b.gguf",
            "qwen2.5-0.5b-instruct-q4_k_m.gguf",
        ],
    ),
    (
        "qwen3.5-0.8b",
        &[
            "qwen3.5-0.8b.gguf",
            "Qwen3.5-0.8B-Q4_K_M.gguf",
        ],
    ),
    (
        "qwen2.5-1.5b",
        &[
            "qwen2.5-1.5b.gguf",
            "qwen2.5-1.5b-instruct-q4_k_m.gguf",
        ],
    ),
];

fn candidate_paths(base_dir: &PathBuf, id: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    paths.push(base_dir.join(format!("{}.gguf", id)));

    for (model_id, files) in MODEL_CANDIDATE_FILES {
        if *model_id == id {
            for file in *files {
                paths.push(base_dir.join(file));
            }
        }
    }

    paths
}

fn resolve_model_path(models_dir: &PathBuf, id: &str) -> Option<PathBuf> {
    candidate_paths(models_dir, id)
        .into_iter()
        .find(|path| match std::fs::metadata(path) {
            Ok(metadata) => metadata.len() >= MIN_VALID_MODEL_SIZE,
            Err(_) => false,
        })
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

    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .timeout(std::time::Duration::from_secs(3600)) // 1 hour for large models
        .build()
        .unwrap_or_else(|_| Client::new());
    
    let manager = ModelManager {
        client,
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
        let mut found = false;
        for path in candidate_paths(&manager.models_dir, &model.id) {
            if let Ok(metadata) = std::fs::metadata(&path) {
                let size = metadata.len();
                if size >= MIN_VALID_MODEL_SIZE {
                    model.is_downloaded = true;
                    found = true;
                    tracing::debug!("Model {} path: {} size: {} downloaded: true", model.id, path.display(), size);
                    break;
                }

                if size > 0 {
                    let _ = std::fs::remove_file(&path);
                    tracing::warn!("Removed corrupted model file: {} ({} bytes)", path.display(), size);
                }
            }
        }

        if !found {
            tracing::debug!("Model {} not found in bundled or downloaded paths", model.id);
        }
    }

    tracing::info!("Models dir: {}, downloaded: {}", manager.models_dir.display(), models.iter().filter(|m| m.is_downloaded).count());
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
    tracing::info!("Downloading model {} to {}", id, path.display());
    tracing::info!("URL: {}", model.download_url);
    
    let mut file = tokio::fs::File::create(&path).await?;

    let mut res = client.get(&model.download_url).send().await?;
    let status = res.status();
    tracing::info!("HTTP response status: {}", status);
    if !status.is_success() {
        return Err(anyhow::anyhow!("HTTP error: {}", status));
    }
    let total_size = res
        .content_length()
        .unwrap_or(model.size_mb as u64 * 1024 * 1024);

    tracing::info!("Starting download, total size: {} bytes", total_size);
    
    let mut downloaded: u64 = 0;
    while let Some(chunk) = res.chunk().await? {
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        let _ = progress.add((downloaded as f64 / total_size as f64) * 100.0);
    }
    
    file.flush().await?;
    drop(file);
    
    // Verify file was written
    let metadata = tokio::fs::metadata(&path).await?;
    tracing::info!("Download complete: {} bytes written to {}", metadata.len(), path.display());
    
    if metadata.len() == 0 {
        tokio::fs::remove_file(&path).await?;
        return Err(anyhow::anyhow!("Download failed: file is empty"));
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

    tracing::info!("set_active_model requested: id={}", id);
    let model_path = resolve_model_path(&models_dir, &id)
        .ok_or_else(|| anyhow::anyhow!("Model '{}' not downloaded. Download it first via the AI Models screen.", id))?;

    tracing::info!("Resolved active model path: {}", model_path.display());

    let ai_guard = SHARED_AI.lock().await;
    let ai = ai_guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Hydra node not started yet. Start the node before switching models.")
    })?;

    tracing::info!("Reloading AI model: {} from {}", id, model_path.display());
    ai.load_model(model_path.clone()).await?;
    tracing::info!("AI model switched to: {}", model_path.display());

    Ok(())
}

/// Check if any model is loaded and ready for inference
pub async fn is_model_loaded() -> bool {
    let ai_guard = SHARED_AI.lock().await;
    if let Some(ai) = ai_guard.as_ref() {
        let infer_guard = ai.infer().lock().await;
        infer_guard.is_some()
    } else {
        false
    }
}

/// Chat with the loaded model - send a prompt and get a response
pub async fn chat_with_model(prompt: String, max_tokens: u32) -> anyhow::Result<String> {
    let ai_guard = SHARED_AI.lock().await;
    let ai = ai_guard.as_ref().ok_or_else(|| {
        anyhow::anyhow!("Hydra node not started. Start the node first.")
    })?;

    tracing::info!(
        "chat_with_model called: prompt_len={}, max_tokens={}",
        prompt.len(),
        max_tokens
    );

    let mut infer_guard = ai.infer().lock().await;
    let infer = infer_guard.as_mut().ok_or_else(|| {
        anyhow::anyhow!("No model loaded. Download and activate a model first.")
    })?;

    let start = std::time::Instant::now();
    tracing::info!("Starting inference");
    let response = infer.generate(&prompt, max_tokens as usize)?;
    let elapsed = start.elapsed();
    
    tracing::info!(
        "LLM inference completed in {:.2}s, {} tokens generated",
        elapsed.as_secs_f64(),
        response.split_whitespace().count()
    );
    tracing::debug!("LLM response preview: {}", response.chars().take(300).collect::<String>());

    Ok(response)
}
