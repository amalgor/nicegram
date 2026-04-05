use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use hydra_config::HydraConfig;
use hydra_core::transport::wss::connect_relay_websocket;
use hydra_econ::provider::{PendingReputationSync, ProviderMetrics};
use hydra_exchange::{AgentRegistrar, ExchangeConfig, ReputationClient, RouteExchangeClient};
use hydra_p2p::ServiceAnnouncement;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{watch, Mutex};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::{self, client::IntoClientRequest};
use zeroize::Zeroize;

const DEFAULT_PROFILE_ID: &str = "default";
const PROVIDER_STATE_PREFIX: &str = "provider_state";
const PROVIDER_RELAY_ENDPOINT: &str = "wss://relay.hydra-net.work";
const DEFAULT_PROTOCOL: &str = "vless";
const DEFAULT_REGION: &str = "US";
const DEFAULT_PRICE_PER_GB_RAW: &str = "1000000";
const DEFAULT_STAKE_AMOUNT_RAW: &str = "1000000";

lazy_static! {
    static ref SHARE_RUNTIME: Mutex<Option<ShareRuntimeHandle>> = Mutex::new(None);
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ShareSettings {
    pub price_override_raw: Option<String>,
    pub max_bandwidth_mbps: Option<u64>,
    #[serde(default)]
    pub wifi_only: bool,
    pub schedule_start_hour: Option<u8>,
    pub schedule_end_hour: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ProviderState {
    enabled: bool,
    active: bool,
    agent_id: Option<u64>,
    agent_tx_hash: String,
    endpoint_url: String,
    region: String,
    protocol: String,
    price_per_gb_raw: String,
    bandwidth_mbps: u64,
    route_book_offer_id: Option<u64>,
    onchain_active: bool,
    settings: ShareSettings,
    last_error: String,
    last_announced_at: u64,
}

impl Default for ProviderState {
    fn default() -> Self {
        Self {
            enabled: false,
            active: false,
            agent_id: None,
            agent_tx_hash: String::new(),
            endpoint_url: String::new(),
            region: DEFAULT_REGION.to_string(),
            protocol: DEFAULT_PROTOCOL.to_string(),
            price_per_gb_raw: DEFAULT_PRICE_PER_GB_RAW.to_string(),
            bandwidth_mbps: 20,
            route_book_offer_id: None,
            onchain_active: false,
            settings: ShareSettings::default(),
            last_error: String::new(),
            last_announced_at: 0,
        }
    }
}

#[derive(Debug, Serialize)]
struct ShareEarnStatusPayload {
    profile_id: String,
    runtime_profile_id: Option<String>,
    sharing_active_under_other_profile: bool,
    enabled: bool,
    active: bool,
    unlocked: bool,
    agent_id: Option<u64>,
    agent_tx_hash: String,
    endpoint_url: String,
    region: String,
    protocol: String,
    price_per_gb_raw: String,
    price_per_gb_display: String,
    bandwidth_mbps: u64,
    route_book_offer_id: Option<u64>,
    onchain_active: bool,
    last_error: String,
    last_announced_at: u64,
    estimated_earnings_display: String,
    settled_earnings_display: String,
    local_routing_score: f64,
    pending_reputation_delta: f64,
    pending_reputation_syncs: usize,
    average_latency_ms: f64,
    average_throughput_mbps: f64,
    uptime_ratio: f64,
    recent_failures: u32,
    last_onchain_sync_time: u64,
    toggle_message: String,
    settings: ShareSettings,
}

#[derive(Debug, Serialize)]
struct ProviderEarningsPayload {
    profile_id: String,
    agent_id: Option<u64>,
    agent_tx_hash: String,
    session_count: u64,
    successful_sessions: u64,
    bytes_relayed: u64,
    estimated_earnings_micro_usdc: i64,
    estimated_earnings_display: String,
    settled_earnings_micro_usdc: i64,
    settled_earnings_display: String,
    local_routing_score: f64,
    pending_reputation_delta: f64,
    pending_reputation_syncs: usize,
    average_latency_ms: f64,
    average_throughput_mbps: f64,
    uptime_ratio: f64,
    recent_failures: u32,
    last_onchain_sync_time: u64,
}

#[derive(Debug, Deserialize)]
struct ProviderControlMessage {
    #[serde(rename = "type")]
    message_type: String,
    target: Option<String>,
}

struct ShareRuntimeHandle {
    profile_id: String,
    stop_tx: watch::Sender<bool>,
    task: JoinHandle<()>,
}

pub(crate) async fn get_share_earn_status(profile_id: Option<String>) -> Result<String> {
    let profile_id = normalized_profile_id(profile_id.as_deref());
    let base_dir = shared_base_dir()?;
    let state = load_state(&base_dir, &profile_id)?;
    let metrics = provider_metrics_snapshot(state.agent_id).await?;
    let runtime_profile_id = current_runtime_profile_id().await;
    let sharing_active_under_other_profile = runtime_profile_id
        .as_ref()
        .is_some_and(|active_profile| active_profile != &profile_id);
    let payload = ShareEarnStatusPayload {
        profile_id: profile_id.clone(),
        runtime_profile_id: runtime_profile_id.clone(),
        sharing_active_under_other_profile,
        enabled: state.enabled,
        active: state.active,
        unlocked: true,
        agent_id: state.agent_id,
        agent_tx_hash: state.agent_tx_hash.clone(),
        endpoint_url: state.endpoint_url.clone(),
        region: state.region.clone(),
        protocol: state.protocol.clone(),
        price_per_gb_raw: state.price_per_gb_raw.clone(),
        price_per_gb_display: format_micro_usdc_string(&state.price_per_gb_raw),
        bandwidth_mbps: state.bandwidth_mbps,
        route_book_offer_id: state.route_book_offer_id,
        onchain_active: state.onchain_active,
        last_error: state.last_error.clone(),
        last_announced_at: state.last_announced_at,
        estimated_earnings_display: format_micro_usdc(metrics.estimated_earnings_micro_usdc),
        settled_earnings_display: format_micro_usdc(metrics.settled_earnings_micro_usdc),
        local_routing_score: metrics.local_routing_score,
        pending_reputation_delta: metrics.pending_reputation_delta,
        pending_reputation_syncs: pending_reputation_syncs().await?.len(),
        average_latency_ms: metrics.average_latency_ms,
        average_throughput_mbps: metrics.average_throughput_mbps,
        uptime_ratio: metrics.uptime_ratio,
        recent_failures: metrics.recent_failures,
        last_onchain_sync_time: metrics.last_onchain_sync_time,
        toggle_message: toggle_message(
            &state,
            &metrics,
            sharing_active_under_other_profile,
            runtime_profile_id.as_deref(),
        ),
        settings: state.settings.clone(),
    };
    Ok(serde_json::to_string(&payload)?)
}

pub(crate) async fn set_share_earn_enabled(
    enabled: bool,
    profile_id: Option<String>,
    mnemonic: Option<String>,
) -> Result<String> {
    let profile_id = normalized_profile_id(profile_id.as_deref());
    let base_dir = shared_base_dir()?;
    let mut state = load_state(&base_dir, &profile_id)?;
    let exchange = load_exchange_config()?;

    if enabled {
        let mut mnemonic = mnemonic.unwrap_or_default();
        if state.agent_id.is_none() {
            if mnemonic.trim().is_empty() {
                anyhow::bail!("Share & Earn setup needs account access the first time.");
            }
            let registration = AgentRegistrar::new(exchange.clone())
                .register(&mnemonic)
                .await?;
            state.agent_id = Some(registration.agent_id);
            state.agent_tx_hash = registration.tx_hash;
        }

        if state.price_per_gb_raw.trim().is_empty() {
            state.price_per_gb_raw = market_median_price(&exchange, &state.region).await?;
        }
        if state.bandwidth_mbps == 0 {
            state.bandwidth_mbps = default_bandwidth(&state.settings);
        }

        let agent_id = state.agent_id.expect("agent id set above");
        state.enabled = true;
        state.endpoint_url = format!("{PROVIDER_RELAY_ENDPOINT}?agent={agent_id}");
        state.last_error.clear();
        save_state(&base_dir, &profile_id, &state)?;
        ensure_runtime(base_dir.clone(), profile_id.clone(), state.clone()).await?;
        if !mnemonic.trim().is_empty() {
            let _ = sync_pending_reputation_internal(&exchange, &mut mnemonic).await;
            maybe_graduate_to_route_book(&exchange, &profile_id, &state, &mut mnemonic).await?;
        }
        mnemonic.zeroize();
    } else {
        disable_runtime(Some(profile_id.clone())).await?;
        state.enabled = false;
        state.active = false;
        save_state(&base_dir, &profile_id, &state)?;
    }

    get_share_earn_status(Some(profile_id)).await
}

pub(crate) async fn get_provider_earnings(profile_id: Option<String>) -> Result<String> {
    let profile_id = normalized_profile_id(profile_id.as_deref());
    let base_dir = shared_base_dir()?;
    let state = load_state(&base_dir, &profile_id)?;
    let metrics = provider_metrics_snapshot(state.agent_id).await?;
    let payload = ProviderEarningsPayload {
        profile_id,
        agent_id: state.agent_id,
        agent_tx_hash: state.agent_tx_hash,
        session_count: metrics.session_count,
        successful_sessions: metrics.successful_sessions,
        bytes_relayed: metrics.bytes_relayed,
        estimated_earnings_micro_usdc: metrics.estimated_earnings_micro_usdc,
        estimated_earnings_display: format_micro_usdc(metrics.estimated_earnings_micro_usdc),
        settled_earnings_micro_usdc: metrics.settled_earnings_micro_usdc,
        settled_earnings_display: format_micro_usdc(metrics.settled_earnings_micro_usdc),
        local_routing_score: metrics.local_routing_score,
        pending_reputation_delta: metrics.pending_reputation_delta,
        pending_reputation_syncs: pending_reputation_syncs().await?.len(),
        average_latency_ms: metrics.average_latency_ms,
        average_throughput_mbps: metrics.average_throughput_mbps,
        uptime_ratio: metrics.uptime_ratio,
        recent_failures: metrics.recent_failures,
        last_onchain_sync_time: metrics.last_onchain_sync_time,
    };
    Ok(serde_json::to_string(&payload)?)
}

pub(crate) async fn update_share_settings(
    profile_id: Option<String>,
    price_override_raw: Option<String>,
    max_bandwidth_mbps: Option<u64>,
    wifi_only: bool,
    schedule_start_hour: Option<u8>,
    schedule_end_hour: Option<u8>,
) -> Result<String> {
    let profile_id = normalized_profile_id(profile_id.as_deref());
    let base_dir = shared_base_dir()?;
    let mut state = load_state(&base_dir, &profile_id)?;
    state.settings = ShareSettings {
        price_override_raw: price_override_raw.filter(|value| !value.trim().is_empty()),
        max_bandwidth_mbps,
        wifi_only,
        schedule_start_hour,
        schedule_end_hour,
    };
    if let Some(price) = &state.settings.price_override_raw {
        state.price_per_gb_raw = price.trim().to_string();
    }
    state.bandwidth_mbps = default_bandwidth(&state.settings);
    save_state(&base_dir, &profile_id, &state)?;
    get_share_earn_status(Some(profile_id)).await
}

async fn ensure_runtime(base_dir: PathBuf, profile_id: String, state: ProviderState) -> Result<()> {
    let mut guard = SHARE_RUNTIME.lock().await;
    if let Some(runtime) = guard.as_ref() {
        if !runtime.task.is_finished() {
            if runtime.profile_id == profile_id {
                return Ok(());
            }
            anyhow::bail!(
                "Share & Earn is already active under profile '{}'. Disable it there before switching.",
                runtime.profile_id
            );
        }
    }

    let (stop_tx, stop_rx) = watch::channel(false);
    let runtime_profile_id = profile_id.clone();
    let task = tokio::spawn(async move {
        if let Err(error) = run_provider_runtime(
            base_dir.clone(),
            runtime_profile_id.clone(),
            state.clone(),
            stop_rx,
        )
        .await
        {
            let _ = update_state(&base_dir, &runtime_profile_id, |current| {
                current.active = false;
                current.last_error = error.to_string();
            });
        }
    });
    *guard = Some(ShareRuntimeHandle {
        profile_id,
        stop_tx,
        task,
    });
    Ok(())
}

async fn disable_runtime(profile_id: Option<String>) -> Result<()> {
    let profile_id = profile_id
        .as_deref()
        .map(|value| normalized_profile_id(Some(value)))
        .unwrap_or_else(|| DEFAULT_PROFILE_ID.to_string());
    let mut guard = SHARE_RUNTIME.lock().await;
    if let Some(runtime) = guard.as_ref() {
        if runtime.profile_id != profile_id && !runtime.task.is_finished() {
            return Ok(());
        }
    }
    if let Some(runtime) = guard.take() {
        let _ = runtime.stop_tx.send(true);
        let _ = runtime.task.await;
    }
    Ok(())
}

async fn run_provider_runtime(
    base_dir: PathBuf,
    profile_id: String,
    initial_state: ProviderState,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<()> {
    let agent_id = initial_state
        .agent_id
        .ok_or_else(|| anyhow::anyhow!("Share & Earn agent is missing"))?;

    loop {
        if *stop_rx.borrow() {
            let _ = update_state(&base_dir, &profile_id, |state| {
                state.active = false;
            });
            return Ok(());
        }

        let mut request = PROVIDER_RELAY_ENDPOINT.into_client_request()?;
        request
            .headers_mut()
            .insert("X-Hydra-Mode", "provider".parse()?);
        request
            .headers_mut()
            .insert("X-Hydra-Agent", agent_id.to_string().parse()?);

        match connect_relay_websocket(
            PROVIDER_RELAY_ENDPOINT,
            request,
            &format!("provider:{agent_id}"),
        )
        .await
        {
            Ok((mut ws, tls_report)) => {
                tracing::info!(
                    "Provider relay session for agent {} connected using {}",
                    agent_id,
                    tls_report.summary()
                );
                update_state(&base_dir, &profile_id, |state| {
                    state.active = true;
                    state.last_error.clear();
                })?;

                let mut announcement_ticker = tokio::time::interval(Duration::from_secs(60));
                publish_announcement(agent_id, &profile_id).await?;

                loop {
                    tokio::select! {
                        _ = stop_rx.changed() => {
                            if *stop_rx.borrow() {
                                let _ = ws.close(None).await;
                                update_state(&base_dir, &profile_id, |state| {
                                    state.active = false;
                                })?;
                                return Ok(());
                            }
                        }
                        _ = announcement_ticker.tick() => {
                            publish_announcement(agent_id, &profile_id).await?;
                        }
                        message = ws.next() => {
                            match message {
                                Some(Ok(tungstenite::Message::Text(text))) => {
                                    if let Some(target) = parse_connect_target(&text) {
                                        handle_provider_session(&mut ws, &profile_id, agent_id, target).await?;
                                    } else if text.contains("\"type\":\"ping\"") {
                                        ws.send(tungstenite::Message::Text("{\"type\":\"pong\"}".into())).await?;
                                    }
                                }
                                Some(Ok(tungstenite::Message::Ping(payload))) => {
                                    ws.send(tungstenite::Message::Pong(payload)).await?;
                                }
                                Some(Ok(tungstenite::Message::Close(_))) => {
                                    update_state(&base_dir, &profile_id, |state| {
                                        state.active = false;
                                        state.last_error = "Relay session closed.".to_string();
                                    })?;
                                    break;
                                }
                                Some(Err(error)) => {
                                    update_state(&base_dir, &profile_id, |state| {
                                        state.active = false;
                                        state.last_error = error.to_string();
                                    })?;
                                    break;
                                }
                                None => {
                                    update_state(&base_dir, &profile_id, |state| {
                                        state.active = false;
                                        state.last_error = "Relay session ended.".to_string();
                                    })?;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            Err(error) => {
                update_state(&base_dir, &profile_id, |state| {
                    state.active = false;
                    state.last_error = format!("Relay connection failed: {error}");
                })?;
                tokio::select! {
                    _ = stop_rx.changed() => {
                        if *stop_rx.borrow() {
                            return Ok(());
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                }
            }
        }
    }
}

async fn handle_provider_session<S>(
    ws: &mut tokio_tungstenite::WebSocketStream<S>,
    profile_id: &str,
    agent_id: u64,
    target: String,
) -> Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let connect_started = Instant::now();
    let mut outbound = match TcpStream::connect(&target).await {
        Ok(stream) => stream,
        Err(error) => {
            if let Some(metrics) = crate::credit_runtime::shared_provider_metrics().await {
                let _ = metrics.record_failure(agent_id);
            }
            ws.send(tungstenite::Message::Text(
                serde_json::json!({
                    "type": "error",
                    "message": format!("Provider could not reach target: {error}"),
                })
                .to_string()
                .into(),
            ))
            .await?;
            return Ok(());
        }
    };

    ws.send(tungstenite::Message::Text(
        serde_json::json!({ "type": "ready" }).to_string().into(),
    ))
    .await?;

    let started_at = Instant::now();
    let mut total_bytes = 0u64;
    let mut read_buf = vec![0u8; 16384];

    loop {
        tokio::select! {
            message = ws.next() => {
                match message {
                    Some(Ok(tungstenite::Message::Binary(data))) => {
                        total_bytes = total_bytes.saturating_add(data.len() as u64);
                        outbound.write_all(&data).await?;
                    }
                    Some(Ok(tungstenite::Message::Text(text))) => {
                        if text.contains("\"type\":\"close\"") {
                            break;
                        }
                    }
                    Some(Ok(tungstenite::Message::Close(_))) | Some(Err(_)) | None => {
                        break;
                    }
                    _ => {}
                }
            }
            read = outbound.read(&mut read_buf) => {
                match read {
                    Ok(0) => break,
                    Ok(n) => {
                        total_bytes = total_bytes.saturating_add(n as u64);
                        ws.send(tungstenite::Message::Binary(read_buf[..n].to_vec().into())).await?;
                    }
                    Err(error) => {
                        ws.send(tungstenite::Message::Text(
                            serde_json::json!({
                                "type": "error",
                                "message": format!("Provider upstream read failed: {error}"),
                            }).to_string().into()
                        )).await?;
                        break;
                    }
                }
            }
        }
    }

    ws.send(tungstenite::Message::Text(
        serde_json::json!({ "type": "closed" }).to_string().into(),
    ))
    .await?;

    if let Some(metrics) = crate::credit_runtime::shared_provider_metrics().await {
        let price = load_state(&shared_base_dir()?, profile_id)?
            .price_per_gb_raw
            .parse::<u64>()
            .unwrap_or_default();
        let _ = metrics.record_success(
            agent_id,
            total_bytes,
            started_at.elapsed(),
            connect_started.elapsed().as_millis() as u64,
            price,
        );
    }
    Ok(())
}

async fn publish_announcement(agent_id: u64, profile_id: &str) -> Result<()> {
    let Some(handle) = crate::api::simple::shared_p2p_handle().await else {
        return Ok(());
    };
    let base_dir = shared_base_dir()?;
    let mut state = load_state(&base_dir, profile_id)?;
    if !state.enabled {
        return Ok(());
    }
    if state.endpoint_url.is_empty() {
        state.endpoint_url = format!("{PROVIDER_RELAY_ENDPOINT}?agent={agent_id}");
    }
    let announcement = ServiceAnnouncement {
        agent_id,
        endpoint_url: state.endpoint_url.clone(),
        protocols: vec![state.protocol.clone()],
        region: state.region.clone(),
        price_per_gb_raw: state.price_per_gb_raw.clone(),
        bandwidth_mbps: state.bandwidth_mbps,
        tier: if state.onchain_active {
            "staked".to_string()
        } else {
            "unstaked".to_string()
        },
        announced_at: now_epoch_secs(),
        source_peer_id: String::new(),
    };
    handle.publish_service_announcement(announcement).await?;
    state.last_announced_at = now_epoch_secs();
    save_state(&base_dir, profile_id, &state)?;
    Ok(())
}

async fn market_median_price(exchange: &ExchangeConfig, region: &str) -> Result<String> {
    let client = RouteExchangeClient::new(exchange.clone());
    let offers = client
        .query_offers(region, DEFAULT_PROTOCOL)
        .await
        .unwrap_or_default();
    let mut prices: Vec<u64> = offers
        .iter()
        .filter_map(|offer| offer.price_per_gb_raw.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .collect();
    prices.sort_unstable();
    Ok(prices
        .get(prices.len().saturating_div(2))
        .copied()
        .unwrap_or(1_000_000)
        .to_string())
}

async fn maybe_graduate_to_route_book(
    exchange: &ExchangeConfig,
    profile_id: &str,
    state: &ProviderState,
    mnemonic: &mut String,
) -> Result<()> {
    let Some(agent_id) = state.agent_id else {
        return Ok(());
    };
    let Some(metrics) = crate::credit_runtime::shared_provider_metrics().await else {
        return Ok(());
    };
    let snapshot = metrics.get(agent_id)?;
    if snapshot.settled_earnings_micro_usdc
        < DEFAULT_STAKE_AMOUNT_RAW.parse::<i64>().unwrap_or(1_000_000)
        || state.onchain_active
    {
        return Ok(());
    }

    let client = RouteExchangeClient::new(exchange.clone());
    let result = client
        .create_offer(
            mnemonic,
            hydra_exchange::CreateOfferInput {
                agent_id,
                endpoint_url: state.endpoint_url.clone(),
                protocols: vec![state.protocol.clone()],
                region: state.region.clone(),
                price_per_gb_raw: state.price_per_gb_raw.clone(),
                stake_amount_raw: DEFAULT_STAKE_AMOUNT_RAW.to_string(),
                bandwidth_mbps: state.bandwidth_mbps,
            },
        )
        .await?;
    let base_dir = shared_base_dir()?;
    update_state(&base_dir, profile_id, |current| {
        current.route_book_offer_id = result.offer_id;
        current.onchain_active = result.offer_id.is_some();
    })?;
    Ok(())
}

pub(crate) async fn sync_provider_reputation(
    profile_id: Option<String>,
    mnemonic: String,
) -> Result<String> {
    let profile_id = normalized_profile_id(profile_id.as_deref());
    let exchange = load_exchange_config()?;
    let mut mnemonic = mnemonic;
    sync_pending_reputation_internal(&exchange, &mut mnemonic).await?;
    mnemonic.zeroize();
    get_share_earn_status(Some(profile_id)).await
}

async fn sync_pending_reputation_internal(
    exchange: &ExchangeConfig,
    mnemonic: &mut String,
) -> Result<()> {
    let Some(metrics) = crate::credit_runtime::shared_provider_metrics().await else {
        return Ok(());
    };
    let pending = metrics.pending_syncs(0.2, 3600)?;
    if pending.is_empty() {
        return Ok(());
    }

    let reputation = ReputationClient::new(exchange.clone());
    for sync in pending {
        reputation
            .give_feedback(mnemonic, sync.agent_id, sync.positive, &sync.tag1)
            .await?;
        let _ = metrics.mark_onchain_feedback_synced(sync.agent_id);
    }
    Ok(())
}

async fn provider_metrics_snapshot(agent_id: Option<u64>) -> Result<ProviderMetrics> {
    let Some(agent_id) = agent_id else {
        return Ok(empty_metrics());
    };
    let Some(metrics) = crate::credit_runtime::shared_provider_metrics().await else {
        return Ok(empty_metrics());
    };
    Ok(metrics.get(agent_id)?)
}

async fn pending_reputation_syncs() -> Result<Vec<PendingReputationSync>> {
    let Some(metrics) = crate::credit_runtime::shared_provider_metrics().await else {
        return Ok(Vec::new());
    };
    metrics.pending_syncs(0.2, 3600)
}

fn empty_metrics() -> ProviderMetrics {
    ProviderMetrics {
        agent_id: 0,
        session_count: 0,
        successful_sessions: 0,
        bytes_relayed: 0,
        average_latency_ms: 0.0,
        average_throughput_mbps: 0.0,
        uptime_ratio: 1.0,
        recent_failures: 0,
        local_routing_score: 50.0,
        pending_reputation_delta: 0.0,
        last_onchain_sync_time: 0,
        last_session_time: 0,
        estimated_earnings_micro_usdc: 0,
        settled_earnings_micro_usdc: 0,
    }
}

fn toggle_message(
    state: &ProviderState,
    metrics: &ProviderMetrics,
    sharing_active_under_other_profile: bool,
    runtime_profile_id: Option<&str>,
) -> String {
    if sharing_active_under_other_profile {
        return format!(
            "Share & Earn is active under profile '{}'. Switch back there to manage the live session.",
            runtime_profile_id.unwrap_or(DEFAULT_PROFILE_ID)
        );
    }
    if !state.enabled {
        return "Share & Earn is ready when you want to help other users.".to_string();
    }
    if !state.active {
        return if state.last_error.is_empty() {
            "Share & Earn is enabled and waiting for relay connectivity.".to_string()
        } else {
            format!("Share & Earn paused: {}", state.last_error)
        };
    }
    if state.onchain_active {
        format!(
            "Sharing is live. Estimated earnings: {}.",
            format_micro_usdc(metrics.estimated_earnings_micro_usdc)
        )
    } else {
        "Sharing is live in starter mode. Hydra will graduate this route after enough settled balance is available.".to_string()
    }
}

fn load_exchange_config() -> Result<ExchangeConfig> {
    let base_dir = shared_base_dir()?;
    let config_path = base_dir.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_dir)
        .with_context(|| format!("Failed to load config from {}", config_path.display()))?;
    ExchangeConfig::from_crypto_config(&config.crypto)
}

fn shared_base_dir() -> Result<PathBuf> {
    crate::api::shared_state::shared_base_dir()
        .ok_or_else(|| anyhow::anyhow!("Shared base dir not initialized"))
}

fn state_path(base_dir: &Path, profile_id: &str) -> PathBuf {
    base_dir.join(format!("{PROVIDER_STATE_PREFIX}_{profile_id}.json"))
}

fn load_state(base_dir: &Path, profile_id: &str) -> Result<ProviderState> {
    let path = state_path(base_dir, profile_id);
    if !path.exists() {
        let state = ProviderState::default();
        save_state(base_dir, profile_id, &state)?;
        return Ok(state);
    }
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn save_state(base_dir: &Path, profile_id: &str, state: &ProviderState) -> Result<()> {
    fs::write(
        state_path(base_dir, profile_id),
        serde_json::to_vec_pretty(state)?,
    )?;
    Ok(())
}

fn update_state(
    base_dir: &Path,
    profile_id: &str,
    update: impl FnOnce(&mut ProviderState),
) -> Result<()> {
    let mut state = load_state(base_dir, profile_id)?;
    update(&mut state);
    save_state(base_dir, profile_id, &state)
}

fn normalized_profile_id(profile_id: Option<&str>) -> String {
    let normalized = profile_id
        .unwrap_or(DEFAULT_PROFILE_ID)
        .trim()
        .to_ascii_lowercase();
    if normalized.is_empty() {
        DEFAULT_PROFILE_ID.to_string()
    } else {
        normalized
            .chars()
            .map(|item| {
                if item.is_ascii_alphanumeric() || item == '-' || item == '_' {
                    item
                } else {
                    '_'
                }
            })
            .collect()
    }
}

async fn current_runtime_profile_id() -> Option<String> {
    let guard = SHARE_RUNTIME.lock().await;
    guard
        .as_ref()
        .filter(|runtime| !runtime.task.is_finished())
        .map(|runtime| runtime.profile_id.clone())
}

fn parse_connect_target(text: &str) -> Option<String> {
    serde_json::from_str::<ProviderControlMessage>(text)
        .ok()
        .and_then(|payload| {
            if payload.message_type == "connect" {
                payload.target
            } else {
                None
            }
        })
}

fn default_bandwidth(settings: &ShareSettings) -> u64 {
    settings.max_bandwidth_mbps.unwrap_or(20).max(1)
}

fn format_micro_usdc(value: i64) -> String {
    let negative = value.is_negative();
    let value = value.abs();
    let whole = value / 1_000_000;
    let fraction = value % 1_000_000;
    if negative {
        format!("-{whole}.{fraction:06}")
    } else {
        format!("{whole}.{fraction:06}")
    }
}

fn format_micro_usdc_string(value: &str) -> String {
    value
        .parse::<i64>()
        .map(format_micro_usdc)
        .unwrap_or_else(|_| "0.000000".to_string())
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
