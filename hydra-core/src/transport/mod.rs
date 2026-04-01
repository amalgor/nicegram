pub mod vless;
pub mod wss;

use anyhow::Result;
use async_trait::async_trait;
use hydra_config::{TransportConfig, TransportMode};
use std::sync::Arc;

pub type TransportStream = leaf::proxy::AnyStream;

#[async_trait]
pub trait Transport: Send + Sync {
    async fn connect(&self, target: &str) -> Result<TransportStream>;
    fn name(&self) -> &str;
    fn supports_udp(&self) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Wss,
    Vless,
}

impl TransportKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Wss => "wss",
            Self::Vless => "vless",
        }
    }
}

#[derive(Clone)]
pub struct ConfiguredTransport {
    pub kind: TransportKind,
    pub mode: TransportMode,
    pub transport: Arc<dyn Transport>,
}

impl ConfiguredTransport {
    pub fn mode_matches_target(&self, is_telegram: bool, proxy_mode: &str) -> bool {
        match proxy_mode {
            "telegram" => is_telegram && matches!(self.mode, TransportMode::Telegram | TransportMode::All),
            "full" => {
                if is_telegram {
                    matches!(self.mode, TransportMode::Telegram | TransportMode::All)
                } else {
                    self.mode == TransportMode::All
                }
            }
            _ => false,
        }
    }
}

pub fn build_transports(configs: &[TransportConfig]) -> Result<Vec<ConfiguredTransport>> {
    let mut transports = Vec::with_capacity(configs.len());

    for config in configs {
        match config {
            TransportConfig::Wss {
                endpoints,
                mode,
                device_id,
            } => {
                anyhow::ensure!(
                    !endpoints.is_empty(),
                    "wss transport requires at least one endpoint"
                );
                transports.push(ConfiguredTransport {
                    kind: TransportKind::Wss,
                    mode: *mode,
                    transport: Arc::new(wss::WssTransport::new(
                        endpoints.clone(),
                        device_id.clone(),
                    )),
                });
            }
            TransportConfig::Vless { url, mode } => {
                transports.push(ConfiguredTransport {
                    kind: TransportKind::Vless,
                    mode: *mode,
                    transport: Arc::new(vless::VlessTransport::from_url(url)?),
                });
            }
        }
    }

    Ok(transports)
}
