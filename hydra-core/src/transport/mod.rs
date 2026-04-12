pub mod vless;
pub mod wss;

use anyhow::Result;
use async_trait::async_trait;
use hydra_config::{TransportConfig, TransportMode};
use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, PartialEq)]
pub enum TransportSource {
    StaticConfig,
    DiscoveredFree,
    DiscoveredPremium,
    GossipUnstaked,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransportMetadata {
    pub profile_id: Option<String>,
    pub source: TransportSource,
    pub offer_id: Option<u64>,
    pub agent_id: Option<u64>,
    pub price_per_gb_micro_usdc: u64,
    pub stake_amount_micro_usdc: u64,
    pub bandwidth_mbps: Option<u64>,
    pub reputation_score: f64,
    pub feedback_count: u64,
    pub created_at: Option<u64>,
    pub endpoint_host: Option<String>,
    pub label: String,
}

impl TransportMetadata {
    pub fn static_config(kind: TransportKind) -> Self {
        Self {
            profile_id: None,
            source: TransportSource::StaticConfig,
            offer_id: None,
            agent_id: None,
            price_per_gb_micro_usdc: 0,
            stake_amount_micro_usdc: 0,
            bandwidth_mbps: None,
            reputation_score: 0.0,
            feedback_count: 0,
            created_at: None,
            endpoint_host: None,
            label: format!("Static {}", kind.as_str()),
        }
    }

    pub fn is_premium(&self) -> bool {
        matches!(self.source, TransportSource::DiscoveredPremium)
            || self.price_per_gb_micro_usdc > 0
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteProfileKind {
    Wss,
    Vless,
}

impl RouteProfileKind {
    pub fn as_transport_kind(&self) -> TransportKind {
        match self {
            Self::Wss => TransportKind::Wss,
            Self::Vless => TransportKind::Vless,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RouteProfileSource {
    Builtin,
    ImportedRaw,
    ImportedSubscription,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteProfile {
    pub id: String,
    pub label: String,
    pub kind: RouteProfileKind,
    pub mode: TransportMode,
    pub enabled: bool,
    pub priority: u32,
    pub source: RouteProfileSource,
    pub config: TransportConfig,
}

impl RouteProfile {
    pub fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        kind: RouteProfileKind,
        mode: TransportMode,
        enabled: bool,
        priority: u32,
        source: RouteProfileSource,
        config: TransportConfig,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            kind,
            mode,
            enabled,
            priority,
            source,
            config,
        }
    }
}

#[derive(Clone)]
pub struct ConfiguredTransport {
    pub kind: TransportKind,
    pub mode: TransportMode,
    pub transport: Arc<dyn Transport>,
    pub metadata: TransportMetadata,
}

impl ConfiguredTransport {
    pub fn mode_matches_target(&self, is_telegram: bool, proxy_mode: &str) -> bool {
        match proxy_mode {
            "telegram" => {
                is_telegram && matches!(self.mode, TransportMode::Telegram | TransportMode::All)
            }
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
                    metadata: TransportMetadata::static_config(TransportKind::Wss),
                });
            }
            TransportConfig::Vless { url, mode } => {
                transports.push(ConfiguredTransport {
                    kind: TransportKind::Vless,
                    mode: *mode,
                    transport: Arc::new(vless::VlessTransport::from_url(url)?),
                    metadata: TransportMetadata::static_config(TransportKind::Vless),
                });
            }
        }
    }

    Ok(transports)
}

pub fn build_transports_from_profiles(
    profiles: &[RouteProfile],
) -> Result<Vec<ConfiguredTransport>> {
    let mut ordered = profiles
        .iter()
        .filter(|profile| profile.enabled)
        .cloned()
        .collect::<Vec<_>>();
    ordered.sort_by_key(|profile| profile.priority);

    let mut transports = Vec::with_capacity(ordered.len());
    for profile in ordered {
        let mut built = build_transports(std::slice::from_ref(&profile.config))?;
        if let Some(mut transport) = built.pop() {
            transport.mode = profile.mode;
            transport.metadata.profile_id = Some(profile.id.clone());
            transport.metadata.label = profile.label.clone();
            transports.push(transport);
        }
    }

    Ok(transports)
}
