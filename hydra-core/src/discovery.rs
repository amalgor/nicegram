use crate::transport::{
    ConfiguredTransport, TransportKind, TransportMetadata, TransportSource, build_transports,
};
use anyhow::Result;
use hydra_config::{DiscoveryConfig, TransportConfig, TransportMode};
use hydra_econ::provider::ProviderMetricsLedger;
use hydra_exchange::{ExchangeConfig, ReputationSummary, RouteExchangeClient, RouteOfferView};
use hydra_p2p::{P2PHandle, ServiceAnnouncement};
use moka::future::Cache;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tracing::warn;
use url::Url;

const CACHE_KEY: &str = "all";

#[derive(Debug, Clone)]
pub struct DiscoveredRoute {
    pub offer: RouteOfferView,
    pub transport_config: TransportConfig,
    pub reputation_score: f64,
    pub route_score: f64,
    pub is_premium: bool,
    pub source: TransportSource,
}

impl DiscoveredRoute {
    pub fn to_configured_transport(&self) -> Result<ConfiguredTransport> {
        let kind = match self.transport_config {
            TransportConfig::Wss { .. } => TransportKind::Wss,
            TransportConfig::Vless { .. } => TransportKind::Vless,
        };
        let mut built = build_transports(std::slice::from_ref(&self.transport_config))?;
        let mut configured = built.pop().expect("single config should build");
        configured.metadata = TransportMetadata {
            source: self.source.clone(),
            offer_id: (self.offer.offer_id > 0).then_some(self.offer.offer_id),
            agent_id: Some(self.offer.agent_id),
            price_per_gb_micro_usdc: self
                .offer
                .price_per_gb_raw
                .parse::<u64>()
                .unwrap_or_default(),
            stake_amount_micro_usdc: self
                .offer
                .stake_amount_raw
                .parse::<u64>()
                .unwrap_or_default(),
            bandwidth_mbps: Some(self.offer.bandwidth_mbps),
            reputation_score: self.reputation_score,
            feedback_count: self
                .offer
                .reputation
                .as_ref()
                .map(|item| item.feedback_count)
                .unwrap_or_default(),
            created_at: Some(self.offer.created_at),
            endpoint_host: endpoint_host(&self.offer.endpoint_ciphertext),
            label: route_label(self.offer.offer_id, self.offer.agent_id, kind, &self.source),
        };
        Ok(configured)
    }
}

pub struct RouteDiscoveryService {
    client: RouteExchangeClient,
    config: DiscoveryConfig,
    cache: Cache<String, Arc<Vec<DiscoveredRoute>>>,
    last_refresh_error: Arc<RwLock<Option<String>>>,
    polling_started: AtomicBool,
    p2p_handle: Arc<RwLock<Option<P2PHandle>>>,
    provider_metrics: Option<Arc<ProviderMetricsLedger>>,
}

impl RouteDiscoveryService {
    pub fn new(
        exchange: ExchangeConfig,
        config: DiscoveryConfig,
        provider_metrics: Option<Arc<ProviderMetricsLedger>>,
    ) -> Self {
        let ttl = Duration::from_secs(config.poll_interval_secs.max(1));
        Self {
            client: RouteExchangeClient::new(exchange),
            config,
            cache: Cache::builder().time_to_live(ttl).max_capacity(4).build(),
            last_refresh_error: Arc::new(RwLock::new(None)),
            polling_started: AtomicBool::new(false),
            p2p_handle: Arc::new(RwLock::new(None)),
            provider_metrics,
        }
    }

    pub async fn attach_p2p_handle(&self, handle: P2PHandle) {
        *self.p2p_handle.write().await = Some(handle);
    }

    pub fn start_polling(self: &Arc<Self>) {
        if self.polling_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let this = self.clone();
        tokio::spawn(async move {
            let _ = this.refresh().await;
            let mut ticker =
                tokio::time::interval(Duration::from_secs(this.config.poll_interval_secs.max(1)));
            loop {
                ticker.tick().await;
                let _ = this.refresh().await;
            }
        });
    }

    pub async fn refresh(&self) -> Result<Vec<DiscoveredRoute>> {
        let future = self
            .client
            .list_active_offers(self.config.max_offers as usize);
        let offers = tokio::time::timeout(
            Duration::from_secs(self.config.rpc_timeout_secs.max(1)),
            future,
        )
        .await
        .map_err(|_| anyhow::anyhow!("Discovery RPC timed out"))??;

        let mut routes = Vec::new();
        for offer in offers {
            if let Some(route) = self.parse_offer(offer) {
                routes.push(route);
            }
        }

        self.cache
            .insert(CACHE_KEY.to_string(), Arc::new(routes.clone()))
            .await;
        *self.last_refresh_error.write().await = None;
        Ok(routes)
    }

    pub async fn cached_routes(&self) -> Vec<DiscoveredRoute> {
        self.cache
            .get(CACHE_KEY)
            .await
            .map(|items| items.as_ref().clone())
            .unwrap_or_default()
    }

    pub async fn current_routes(&self) -> Vec<DiscoveredRoute> {
        let mut routes = self.cached_routes().await;
        if let Some(handle) = self.p2p_handle.read().await.clone() {
            let gossip = handle.get_service_announcements().await;
            routes.extend(self.parse_service_announcements(gossip));
        }

        apply_risk_penalties(&mut routes, self.provider_metrics.as_ref());
        sort_routes(&mut routes, self.config.prefer_free);
        routes
    }

    pub async fn premium_route_available(&self) -> bool {
        self.current_routes()
            .await
            .iter()
            .any(|route| route.is_premium)
    }

    pub async fn premium_is_materially_better(&self) -> bool {
        let routes = self.current_routes().await;
        let best_free = routes
            .iter()
            .filter(|route| !route.is_premium)
            .map(|route| route.route_score)
            .fold(0.0f64, f64::max);
        let best_premium = routes
            .iter()
            .filter(|route| route.is_premium)
            .map(|route| route.route_score)
            .fold(0.0f64, f64::max);
        best_premium > (best_free * 1.15) && best_premium > 0.0
    }

    pub async fn get_transports(
        &self,
        proxy_mode: &str,
        is_telegram: bool,
        premium_allowed: bool,
    ) -> Vec<ConfiguredTransport> {
        if proxy_mode == "off" {
            return Vec::new();
        }

        let mut routes = self.current_routes().await;
        routes.retain(|route| {
            let mode_matches = match proxy_mode {
                "telegram" => is_telegram,
                "full" => true,
                _ => false,
            };
            mode_matches && (!route.is_premium || premium_allowed)
        });

        let mut configured = Vec::new();
        for route in routes {
            match route.to_configured_transport() {
                Ok(item) => configured.push(item),
                Err(error) => warn!(
                    "Skipping discovered offer #{} during transport build: {}",
                    route.offer.offer_id, error
                ),
            }
        }
        configured
    }

    pub async fn last_refresh_error(&self) -> Option<String> {
        self.last_refresh_error.read().await.clone()
    }

    fn parse_offer(&self, offer: RouteOfferView) -> Option<DiscoveredRoute> {
        let transport_config = parse_transport_config(&offer.endpoint_ciphertext)?;
        let price_per_gb_micro_usdc = offer.price_per_gb_raw.parse::<u64>().unwrap_or_default();
        let reputation_score = reputation_score(offer.reputation.as_ref());
        let route_score = base_route_score(
            offer.bandwidth_mbps,
            reputation_score,
            price_per_gb_micro_usdc,
        );

        Some(DiscoveredRoute {
            offer,
            transport_config,
            reputation_score,
            route_score,
            is_premium: price_per_gb_micro_usdc > 0,
            source: if price_per_gb_micro_usdc > 0 {
                TransportSource::DiscoveredPremium
            } else {
                TransportSource::DiscoveredFree
            },
        })
    }

    fn parse_service_announcements(
        &self,
        announcements: Vec<ServiceAnnouncement>,
    ) -> Vec<DiscoveredRoute> {
        let mut routes = Vec::new();
        for announcement in announcements {
            let Some(transport_config) = parse_transport_config(&announcement.endpoint_url) else {
                warn!(
                    "Skipping service announcement for agent {} because endpoint is not transport-compatible",
                    announcement.agent_id
                );
                continue;
            };

            let price_per_gb_raw = announcement.price_per_gb_raw.clone();
            let price_per_gb_micro_usdc = price_per_gb_raw.parse::<u64>().unwrap_or_default();
            let offer = RouteOfferView {
                offer_id: 0,
                provider: announcement.source_peer_id.clone(),
                agent_id: announcement.agent_id,
                endpoint_ciphertext: announcement.endpoint_url.clone(),
                protocols: announcement.protocols.clone(),
                region: announcement.region.clone(),
                price_per_gb_raw: price_per_gb_raw.clone(),
                price_per_gb: price_per_gb_raw.clone(),
                stake_amount_raw: "0".to_string(),
                stake_amount: "0".to_string(),
                bandwidth_mbps: announcement.bandwidth_mbps,
                created_at: announcement.announced_at,
                deactivated_at: 0,
                active: true,
                reputation: None,
            };
            routes.push(DiscoveredRoute {
                offer,
                transport_config,
                reputation_score: 0.0,
                route_score: base_route_score(
                    announcement.bandwidth_mbps,
                    0.0,
                    price_per_gb_micro_usdc,
                ),
                is_premium: price_per_gb_micro_usdc > 0,
                source: TransportSource::GossipUnstaked,
            });
        }
        routes
    }
}

fn parse_transport_config(endpoint: &str) -> Option<TransportConfig> {
    let endpoint = endpoint.trim();
    if endpoint.starts_with("vless://") {
        Some(TransportConfig::Vless {
            url: endpoint.to_string(),
            mode: TransportMode::All,
        })
    } else if endpoint.starts_with("wss://") {
        Some(TransportConfig::Wss {
            endpoints: vec![endpoint.to_string()],
            mode: TransportMode::All,
            device_id: String::new(),
        })
    } else {
        warn!(
            "Skipping discovered endpoint because it is not a usable transport URL: {}",
            endpoint
        );
        None
    }
}

fn sort_routes(routes: &mut [DiscoveredRoute], prefer_free: bool) {
    routes.sort_by(|a, b| {
        let price_order = if prefer_free {
            a.is_premium.cmp(&b.is_premium)
        } else {
            std::cmp::Ordering::Equal
        };
        price_order.then_with(|| {
            b.route_score
                .partial_cmp(&a.route_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    });
}

fn apply_risk_penalties(
    routes: &mut [DiscoveredRoute],
    provider_metrics: Option<&Arc<ProviderMetricsLedger>>,
) {
    let now = now_epoch_secs();
    let positive_prices: Vec<u64> = routes
        .iter()
        .filter_map(|route| route.offer.price_per_gb_raw.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .collect();
    let median_price = median(positive_prices);
    let mut host_counts: HashMap<String, usize> = HashMap::new();
    for route in routes.iter() {
        if let Some(host) = endpoint_host(&route.offer.endpoint_ciphertext) {
            *host_counts.entry(host).or_default() += 1;
        }
    }

    for route in routes.iter_mut() {
        let price = route
            .offer
            .price_per_gb_raw
            .parse::<u64>()
            .unwrap_or_default();
        let feedback_count = route
            .offer
            .reputation
            .as_ref()
            .map(|item| item.feedback_count)
            .unwrap_or_default();
        let mut penalty = 0.0;
        let mut bonus = 0.0;

        if let Some(reference_price) = median_price {
            if price > 0 && price < (reference_price as f64 * 0.35) as u64 {
                penalty += 18.0;
            }
        }

        if now.saturating_sub(route.offer.created_at) < 86_400 {
            penalty += 4.0;
        }

        if matches!(route.source, TransportSource::GossipUnstaked)
            || route.offer.stake_amount_raw == "0"
        {
            penalty += 12.0;
        }

        if feedback_count < 3 {
            penalty += 4.0;
        }

        if route.reputation_score < 0.0 {
            penalty += route.reputation_score.abs() * 18.0;
        } else if feedback_count > 0 {
            bonus += route.reputation_score * 12.0;
        }

        if let Some(host) = endpoint_host(&route.offer.endpoint_ciphertext) {
            if let Some(count) = host_counts.get(&host) {
                if *count > 1 {
                    penalty += (*count as f64 - 1.0) * 3.0;
                }
            }
        }

        if !valid_region(&route.offer.region) {
            penalty += 6.0;
        }

        if let Some(metrics) = provider_metrics {
            if let Ok(snapshot) = metrics.get(route.offer.agent_id) {
                bonus += snapshot.local_routing_score * 0.35;
                penalty += (snapshot.recent_failures as f64).min(5.0) * 2.0;
            }
        }

        route.route_score = (route.route_score + bonus - penalty).max(0.0);
    }
}

fn reputation_score(summary: Option<&ReputationSummary>) -> f64 {
    summary
        .and_then(|item| item.formatted_value.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn base_route_score(
    bandwidth_mbps: u64,
    reputation_score: f64,
    price_per_gb_micro_usdc: u64,
) -> f64 {
    let price_penalty = if price_per_gb_micro_usdc == 0 {
        0.0
    } else {
        (price_per_gb_micro_usdc as f64 / 1_000_000.0) * 15.0
    };
    let free_bonus = if price_per_gb_micro_usdc == 0 {
        10.0
    } else {
        0.0
    };
    bandwidth_mbps as f64 + (reputation_score * 20.0) + free_bonus - price_penalty
}

fn route_label(
    offer_id: u64,
    agent_id: u64,
    kind: TransportKind,
    source: &TransportSource,
) -> String {
    match source {
        TransportSource::GossipUnstaked => format!("Unstaked agent {} {}", agent_id, kind.as_str()),
        _ if offer_id > 0 => format!("Offer #{} {}", offer_id, kind.as_str()),
        _ => format!("Agent {} {}", agent_id, kind.as_str()),
    }
}

fn endpoint_host(endpoint: &str) -> Option<String> {
    Url::parse(endpoint)
        .ok()
        .and_then(|parsed| parsed.host_str().map(ToString::to_string))
}

fn valid_region(region: &str) -> bool {
    region.len() == 2 && region.chars().all(|ch| ch.is_ascii_uppercase())
}

fn median(mut values: Vec<u64>) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable();
    Some(values[values.len() / 2])
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hydra_exchange::BASE_SEPOLIA_CHAIN_ID;

    fn offer(endpoint: &str, price_per_gb_raw: &str) -> RouteOfferView {
        RouteOfferView {
            offer_id: 1,
            provider: "0xprovider".to_string(),
            agent_id: 1,
            endpoint_ciphertext: endpoint.to_string(),
            protocols: vec!["vless".to_string()],
            region: "US".to_string(),
            price_per_gb_raw: price_per_gb_raw.to_string(),
            price_per_gb: "0".to_string(),
            stake_amount_raw: "1".to_string(),
            stake_amount: "1".to_string(),
            bandwidth_mbps: 100,
            created_at: 1_700_000_000,
            deactivated_at: 0,
            active: true,
            reputation: Some(ReputationSummary {
                feedback_count: 5,
                summary_value: "1".to_string(),
                value_decimals: 0,
                formatted_value: "1".to_string(),
            }),
        }
    }

    #[test]
    fn parse_offer_accepts_wss_and_vless_endpoints() {
        let service = RouteDiscoveryService::new(
            ExchangeConfig {
                chain: "BASE-SEPOLIA".to_string(),
                chain_id: BASE_SEPOLIA_CHAIN_ID,
                rpc_url: Url::parse("https://rpc.invalid").unwrap(),
                route_book_address: "0x70594C7C33544fc0F22592005004B219dfbb012E"
                    .parse()
                    .unwrap(),
                deal_board_address: None,
                identity_registry_address: "0x8004A818BFB912233c491871b3d84c89A494BD9e"
                    .parse()
                    .unwrap(),
                reputation_registry_address: Some(
                    "0x8004B663056A597Dffe9eCcC1965A193B7388713"
                        .parse()
                        .unwrap(),
                ),
                usdc_address: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
                    .parse()
                    .unwrap(),
            },
            DiscoveryConfig::default(),
            None,
        );
        assert!(
            service
                .parse_offer(offer("wss://relay.hydra-net.work?agent=4", "0"))
                .is_some()
        );
        assert!(
            service
                .parse_offer(offer("vless://user@example.com:443", "0"))
                .is_some()
        );
        assert!(
            service
                .parse_offer(offer("https://not-supported", "0"))
                .is_none()
        );
    }

    #[test]
    fn apply_risk_penalties_demotes_unstaked_and_too_cheap_routes() {
        let mut routes = vec![
            DiscoveredRoute {
                offer: offer("wss://relay.hydra-net.work?agent=1", "1000000"),
                transport_config: parse_transport_config("wss://relay.hydra-net.work?agent=1")
                    .unwrap(),
                reputation_score: 1.0,
                route_score: 100.0,
                is_premium: true,
                source: TransportSource::DiscoveredPremium,
            },
            DiscoveredRoute {
                offer: RouteOfferView {
                    offer_id: 0,
                    provider: "peer".to_string(),
                    agent_id: 2,
                    endpoint_ciphertext: "wss://relay.hydra-net.work?agent=2".to_string(),
                    protocols: vec!["vless".to_string()],
                    region: "US".to_string(),
                    price_per_gb_raw: "1".to_string(),
                    price_per_gb: "0".to_string(),
                    stake_amount_raw: "0".to_string(),
                    stake_amount: "0".to_string(),
                    bandwidth_mbps: 120,
                    created_at: now_epoch_secs(),
                    deactivated_at: 0,
                    active: true,
                    reputation: None,
                },
                transport_config: parse_transport_config("wss://relay.hydra-net.work?agent=2")
                    .unwrap(),
                reputation_score: 0.0,
                route_score: 120.0,
                is_premium: true,
                source: TransportSource::GossipUnstaked,
            },
        ];

        apply_risk_penalties(&mut routes, None);
        assert!(routes[0].route_score > routes[1].route_score);
    }
}
