use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::info;

/// Network configuration: SOCKS5 proxy, P2P listener, bootstrap nodes
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkConfig {
    /// Port for local SOCKS5 proxy server
    pub socks5_port: u16,
    /// P2P listen port (0 = random ephemeral port)
    pub p2p_listen_port: u16,
    /// Bootstrap node addresses in libp2p multiaddr format
    pub bootstrap_nodes: Vec<String>,
    /// Runtime proxy mode: "off", "telegram", "full"
    pub proxy_mode: String,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            socks5_port: 1080,
            p2p_listen_port: 0,
            bootstrap_nodes: vec!["/dns4/boot.ze1.org/tcp/33097".to_string()],
            proxy_mode: "telegram".to_string(),
        }
    }
}

/// AI model inference configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    /// Path to GGUF model weights file (llama.cpp — tokenizer embedded in GGUF)
    pub model_path: PathBuf,
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
            model_path: PathBuf::from("models/qwen2.5-0.5b.gguf"),
            max_generation_tokens: 128,
            cache_ttl_seconds: 300,
            cache_max_items: 1000,
        }
    }
}

/// Economic ledger and reputation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
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
#[serde(default)]
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
#[serde(default)]
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

/// Transport routing scope.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    Telegram,
    All,
}

impl Default for TransportMode {
    fn default() -> Self {
        Self::Telegram
    }
}

impl TransportMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Telegram => "telegram",
            Self::All => "all",
        }
    }
}

/// Transport configuration entries.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TransportConfig {
    /// Cloudflare Worker / WebSocket-based relay.
    Wss {
        endpoints: Vec<String>,
        mode: TransportMode,
        #[serde(default)]
        device_id: String,
    },
    /// VLESS transport encoded as a raw vless:// URL.
    Vless {
        url: String,
        mode: TransportMode,
    },
}

impl TransportConfig {
    pub fn mode(&self) -> TransportMode {
        match self {
            Self::Wss { mode, .. } | Self::Vless { mode, .. } => *mode,
        }
    }

    pub fn wss_endpoint(&self) -> Option<&str> {
        match self {
            Self::Wss { endpoints, .. } => endpoints.first().map(String::as_str),
            Self::Vless { .. } => None,
        }
    }

    pub fn wss_device_id(&self) -> Option<&str> {
        match self {
            Self::Wss { device_id, .. } => Some(device_id.as_str()),
            Self::Vless { .. } => None,
        }
    }
}

fn default_transports() -> Vec<TransportConfig> {
    vec![TransportConfig::Wss {
        endpoints: vec!["wss://relay.hydra-net.work".to_string()],
        mode: TransportMode::Telegram,
        device_id: String::new(),
    }]
}

/// Crypto settlement configuration (Circle USDC)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CryptoConfig {
    /// Enable crypto settlement
    pub enabled: bool,
    /// Circle API key (developer-controlled wallets)
    pub circle_api_key: String,
    /// Target chain for settlement
    pub settlement_chain: String,
    /// Wallet set ID (created via Circle API)
    pub wallet_set_id: String,
    /// Entity secret for signing (hex-encoded)
    pub entity_secret: String,
}

impl Default for CryptoConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            circle_api_key: String::new(),
            settlement_chain: "ARB-SEPOLIA".to_string(),
            wallet_set_id: String::new(),
            entity_secret: String::new(),
        }
    }
}

/// Bootstrap node specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BootstrapConfig {
    /// Fixed port for bootstrap node
    pub listen_port: u16,
}

impl Default for BootstrapConfig {
    fn default() -> Self {
        Self { listen_port: 33097 }
    }
}

/// Root configuration for the entire Hydra node
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HydraConfig {
    pub network: NetworkConfig,
    pub ai: AiConfig,
    pub econ: EconConfig,
    pub telegram: TelegramConfig,
    pub content: ContentConfig,
    #[serde(default = "default_transports")]
    pub transports: Vec<TransportConfig>,
    pub crypto: CryptoConfig,
    pub bootstrap: BootstrapConfig,
}

impl Default for HydraConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig::default(),
            ai: AiConfig::default(),
            econ: EconConfig::default(),
            telegram: TelegramConfig::default(),
            content: ContentConfig::default(),
            transports: default_transports(),
            crypto: CryptoConfig::default(),
            bootstrap: BootstrapConfig::default(),
        }
    }
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

    /// Returns the first WSS endpoint and device ID for quota synchronization.
    pub fn primary_quota_transport(&self) -> Option<(String, String)> {
        self.transports.iter().find_map(|transport| match transport {
            TransportConfig::Wss {
                endpoints,
                device_id,
                ..
            } => endpoints.first().cloned().map(|endpoint| {
                let device_id = if device_id.trim().is_empty() {
                    "hydra-mobile".to_string()
                } else {
                    device_id.clone()
                };
                (endpoint, device_id)
            }),
            TransportConfig::Vless { .. } => None,
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_default_config_values() {
        let config = HydraConfig::default();
        assert_eq!(config.network.socks5_port, 1080);
        assert_eq!(config.network.p2p_listen_port, 0);
        assert_eq!(config.network.proxy_mode, "telegram");
        assert_eq!(config.ai.max_generation_tokens, 128);
        assert_eq!(config.ai.cache_ttl_seconds, 300);
        assert_eq!(config.ai.cache_max_items, 1000);
        assert_eq!(config.econ.settlement_threshold_bytes, 10_000_000);
        assert_eq!(config.transports.len(), 1);
        assert_eq!(
            config.transports[0],
            TransportConfig::Wss {
                endpoints: vec!["wss://relay.hydra-net.work".to_string()],
                mode: TransportMode::Telegram,
                device_id: String::new(),
            }
        );
        assert!(!config.crypto.enabled);
        assert_eq!(config.crypto.settlement_chain, "ARB-SEPOLIA");
        assert_eq!(config.bootstrap.listen_port, 33097);
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let config = HydraConfig::load(Path::new("/nonexistent/hydra.toml")).unwrap();
        assert_eq!(config.network.socks5_port, 1080);
        assert_eq!(config.network.proxy_mode, "telegram");
        assert_eq!(config.transports.len(), 1);
    }

    #[test]
    fn test_load_partial_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hydra.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "[network]\nsocks5_port = 9090").unwrap();

        let config = HydraConfig::load(&path).unwrap();
        assert_eq!(config.network.socks5_port, 9090);
        assert_eq!(config.network.proxy_mode, "telegram");
        assert_eq!(config.transports.len(), 1);
        assert_eq!(config.ai.max_generation_tokens, 128);
    }

    #[test]
    fn test_load_full_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hydra.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(
            f,
            r#"
[network]
socks5_port = 2080
p2p_listen_port = 5000
bootstrap_nodes = ["/ip4/1.2.3.4/tcp/1234"]
proxy_mode = "full"

[ai]
model_path = "my_model.gguf"
max_generation_tokens = 256
cache_ttl_seconds = 600
cache_max_items = 500

[econ]
db_path = "my_db"
settlement_threshold_bytes = 5000000

[telegram]
api_id = 12345
api_hash = "abc123"
session_path = "my.session"

[content]
db_path = "my_content.db"
summarization_max_tokens = 1024
cache_ttl_seconds = 7200

[[transports]]
type = "wss"
endpoints = ["wss://relay.example.com"]
mode = "telegram"
device_id = "dev-001"

[[transports]]
type = "vless"
url = "vless://uuid@example.com:443?security=reality&type=tcp&sni=github.com&fp=chrome&pbk=pubkey&sid=0123"
mode = "all"

[crypto]
enabled = true
circle_api_key = "key123"
settlement_chain = "ETH-MAINNET"
wallet_set_id = "ws-1"
entity_secret = "secret"

[bootstrap]
listen_port = 44444
"#
        )
        .unwrap();

        let config = HydraConfig::load(&path).unwrap();
        assert_eq!(config.network.socks5_port, 2080);
        assert_eq!(config.network.p2p_listen_port, 5000);
        assert_eq!(config.network.proxy_mode, "full");
        assert_eq!(config.ai.max_generation_tokens, 256);
        assert_eq!(config.transports.len(), 2);
        assert_eq!(
            config.transports[0],
            TransportConfig::Wss {
                endpoints: vec!["wss://relay.example.com".to_string()],
                mode: TransportMode::Telegram,
                device_id: "dev-001".to_string(),
            }
        );
        assert!(matches!(
            &config.transports[1],
            TransportConfig::Vless { mode, .. } if *mode == TransportMode::All
        ));
        assert!(config.crypto.enabled);
        assert_eq!(config.bootstrap.listen_port, 44444);
    }

    #[test]
    fn test_resolve_paths() {
        let mut config = HydraConfig::default();
        let base = Path::new("/data/hydra");
        config.resolve_paths(base);

        assert_eq!(
            config.ai.model_path,
            PathBuf::from("/data/hydra/models/qwen2.5-0.5b.gguf")
        );
        assert_eq!(config.econ.db_path, PathBuf::from("/data/hydra/hydra_db"));
        assert_eq!(
            config.telegram.session_path,
            PathBuf::from("/data/hydra/telegram.session")
        );
        assert_eq!(config.content.db_path, PathBuf::from("/data/hydra/content.db"));
    }

    #[test]
    fn test_resolve_paths_absolute_unchanged() {
        let mut config = HydraConfig::default();
        config.ai.model_path = PathBuf::from("/absolute/model.gguf");
        config.econ.db_path = PathBuf::from("/absolute/db");

        let base = Path::new("/data/hydra");
        config.resolve_paths(base);

        assert_eq!(config.ai.model_path, PathBuf::from("/absolute/model.gguf"));
        assert_eq!(config.econ.db_path, PathBuf::from("/absolute/db"));
    }

    #[test]
    fn test_write_and_reload_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hydra.toml");

        HydraConfig::write_defaults(&path).unwrap();
        assert!(path.exists());

        let reloaded = HydraConfig::load(&path).unwrap();
        assert_eq!(reloaded.network.socks5_port, 1080);
        assert_eq!(reloaded.network.proxy_mode, "telegram");
        assert_eq!(reloaded.transports.len(), 1);
    }

    #[test]
    fn test_invalid_toml_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hydra.toml");
        std::fs::write(&path, "this is not valid toml [[[").unwrap();

        let result = HydraConfig::load(&path);
        assert!(result.is_err());
    }

    #[test]
    fn test_load_with_base_dir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("hydra.toml");
        std::fs::write(&path, "[network]\nsocks5_port = 3000\n").unwrap();

        let base = Path::new("/app/data");
        let config = HydraConfig::load_with_base_dir(&path, base).unwrap();
        assert_eq!(config.network.socks5_port, 3000);
        assert_eq!(
            config.ai.model_path,
            PathBuf::from("/app/data/models/qwen2.5-0.5b.gguf")
        );
    }

    #[test]
    fn test_primary_quota_transport_prefers_first_wss() {
        let config = HydraConfig {
            transports: vec![
                TransportConfig::Vless {
                    url: "vless://uuid@example.com:443?security=reality&type=tcp&sni=github.com&fp=chrome&pbk=pubkey&sid=0123".to_string(),
                    mode: TransportMode::All,
                },
                TransportConfig::Wss {
                    endpoints: vec!["wss://relay.example.com".to_string()],
                    mode: TransportMode::Telegram,
                    device_id: "device-1".to_string(),
                },
            ],
            ..HydraConfig::default()
        };

        assert_eq!(
            config.primary_quota_transport(),
            Some(("wss://relay.example.com".to_string(), "device-1".to_string()))
        );
    }
}
