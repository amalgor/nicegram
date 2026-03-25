use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

/// Network configuration: SOCKS5 proxy, P2P listener, bootstrap nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Port for local SOCKS5 proxy server
    pub socks5_port: u16,
    /// P2P listen port (0 = random ephemeral port)
    pub p2p_listen_port: u16,
    /// Bootstrap node addresses in libp2p multiaddr format
    pub bootstrap_nodes: Vec<String>,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            socks5_port: 1080,
            p2p_listen_port: 0,
            bootstrap_nodes: vec!["/dns4/boot.ze1.org/tcp/33097".to_string()],
        }
    }
}

/// AI model inference configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    /// Path to GGUF model weights file
    pub model_path: PathBuf,
    /// Path to tokenizer JSON file
    pub tokenizer_path: PathBuf,
    /// Maximum tokens to generate per inference call
    pub max_generation_tokens: usize,
    /// Route decision cache: time-to-live in seconds
    pub cache_ttl_seconds: u64,
    /// Route decision cache: max number of entries
    pub cache_max_items: u64,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            model_path: PathBuf::from("models/qwen2.5-1.5b-instruct-q4_k_m.gguf"),
            tokenizer_path: PathBuf::from("models/tokenizer.json"),
            max_generation_tokens: 128,
            cache_ttl_seconds: 300,
            cache_max_items: 1000,
        }
    }
}

/// Economic ledger and reputation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconConfig {
    /// Path to sled database directory
    pub db_path: PathBuf,
    /// Debt threshold (bytes) that triggers settlement
    pub settlement_threshold_bytes: i64,
}

impl Default for EconConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("hydra_db"),
            settlement_threshold_bytes: 10_000_000,
        }
    }
}

/// Telegram client configuration (grammers MTProto)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfig {
    /// Telegram API ID (obtain at https://my.telegram.org)
    pub api_id: i32,
    /// Telegram API hash
    pub api_hash: String,
    /// Path to session file (persists auth between restarts)
    pub session_path: PathBuf,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            api_id: 0,
            api_hash: String::new(),
            session_path: PathBuf::from("telegram.session"),
        }
    }
}

/// Content Intelligence configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentConfig {
    /// Path to SQLite database for attention tracking and content cache
    pub db_path: PathBuf,
    /// Max tokens for summarization prompt
    pub summarization_max_tokens: usize,
    /// Content cache TTL in seconds (how long processed summaries are cached)
    pub cache_ttl_seconds: u64,
}

impl Default for ContentConfig {
    fn default() -> Self {
        Self {
            db_path: PathBuf::from("content.db"),
            summarization_max_tokens: 512,
            cache_ttl_seconds: 3600,
        }
    }
}

/// Bootstrap node specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapConfig {
    /// Fixed port for bootstrap node
    pub listen_port: u16,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self {
            listen_port: 33097,
        }
    }
}

/// Root configuration for the entire Hydra node
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HydraConfig {
    pub network: NetworkConfig,
    pub ai: AiConfig,
    pub econ: EconConfig,
    pub telegram: TelegramConfig,
    pub content: ContentConfig,
    pub bootstrap: BootstrapConfig,
}

impl HydraConfig {
    /// Load configuration from a TOML file. Falls back to defaults if file is missing.
    pub fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let content = std::fs::read_to_string(path)?;
            let config: HydraConfig = toml::from_str(&content)?;
            info!("Configuration loaded from {}", path.display());
            Ok(config)
        } else {
            info!(
                "Config file {} not found, using defaults. \
                 Create this file to customize settings.",
                path.display()
            );
            Ok(Self::default())
        }
    }

    /// Load config from a TOML file, resolving relative paths against base_dir.
    pub fn load_with_base_dir(path: &Path, base_dir: &Path) -> Result<Self> {
        let mut config = Self::load(path)?;
        config.resolve_paths(base_dir);
        Ok(config)
    }

    /// Resolve relative paths in config against a base directory.
    pub fn resolve_paths(&mut self, base_dir: &Path) {
        if self.ai.model_path.is_relative() {
            self.ai.model_path = base_dir.join(&self.ai.model_path);
        }
        if self.ai.tokenizer_path.is_relative() {
            self.ai.tokenizer_path = base_dir.join(&self.ai.tokenizer_path);
        }
        if self.econ.db_path.is_relative() {
            self.econ.db_path = base_dir.join(&self.econ.db_path);
        }
        if self.telegram.session_path.is_relative() {
            self.telegram.session_path = base_dir.join(&self.telegram.session_path);
        }
        if self.content.db_path.is_relative() {
            self.content.db_path = base_dir.join(&self.content.db_path);
        }
    }

    /// Write default configuration to a file for the user to customize.
    pub fn write_defaults(path: &Path) -> Result<()> {
        let config = Self::default();
        let content = toml::to_string_pretty(&config)?;
        std::fs::write(path, content)?;
        info!("Default configuration written to {}", path.display());
        Ok(())
    }
}
