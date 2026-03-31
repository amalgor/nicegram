#[flutter_rust_bridge::frb(sync)]
pub fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

pub fn init_app() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::new(
        "info,hydra_core::relay=debug,libp2p=warn,libp2p_noise=warn,libp2p_kad=warn,libp2p_gossipsub=warn,libp2p_swarm=warn,libp2p_dns=warn,libp2p_identify=warn,libp2p_mdns=warn,hickory=warn,rustls=warn,tungstenite=debug"
    );
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(crate::api::telemetry::FlutterLogLayer);
    let _ = tracing::subscriber::set_global_default(subscriber);

    tracing::info!("Hydra P2P node initialized and ready.");
}

use hydra_ai::AiNegotiator;
use hydra_config::HydraConfig;
use hydra_core::connections::ConnectionRegistry;
use hydra_core::Socks5Server;
use hydra_econ::EconLedger;
use hydra_p2p::P2PNode;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

lazy_static::lazy_static! {
    static ref SHARED_REGISTRY: tokio::sync::Mutex<Option<Arc<ConnectionRegistry>>> =
        tokio::sync::Mutex::new(None);
    static ref SHARED_RELAY_MODE: tokio::sync::Mutex<Option<Arc<std::sync::RwLock<String>>>> =
        tokio::sync::Mutex::new(None);
    static ref NODE_STARTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
}

static SNAPSHOT_TASK_STARTED: AtomicBool = AtomicBool::new(false);

pub fn init_extension_runtime(base_dir: String) -> anyhow::Result<()> {
    init_app();
    crate::api::shared_state::init_shared_base_dir(base_dir)?;
    Ok(())
}

pub async fn prepare_local_runtime(base_dir: String) -> anyhow::Result<()> {
    let base_path = PathBuf::from(&base_dir);
    crate::api::shared_state::init_shared_base_dir(&base_path)?;

    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;

    crate::api::vpn::SOCKS5_PORT.store(
        config.network.socks5_port,
        std::sync::atomic::Ordering::Relaxed,
    );

    let relay_endpoint = config
        .relay
        .endpoints
        .first()
        .cloned()
        .unwrap_or_else(|| "wss://relay.hydra-net.work".to_string());
    let device_id = if config.relay.device_id.trim().is_empty() {
        "hydra-mobile".to_string()
    } else {
        config.relay.device_id.clone()
    };
    crate::api::quota::init_quota(relay_endpoint, device_id);

    let ai = Arc::new(AiNegotiator::new(&config.ai));
    {
        let mut shared = crate::api::model_manager::SHARED_AI.lock().await;
        *shared = Some(ai);
    }

    tracing::info!("Hydra local runtime prepared at {}", base_path.display());
    Ok(())
}

pub async fn start_hydra_node(base_dir: String) -> anyhow::Result<()> {
    if NODE_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        tracing::warn!("start_hydra_node called again — node already running, skipping.");
        return Ok(());
    }
    tracing::info!("Starting real Hydra mobile node...");

    let base_path = PathBuf::from(&base_dir);
    crate::api::shared_state::init_shared_base_dir(&base_path)?;
    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;

    let relay_endpoint = config
        .relay
        .endpoints
        .first()
        .cloned()
        .unwrap_or_else(|| "wss://relay.hydra-net.work".to_string());
    let device_id = if config.relay.device_id.trim().is_empty() {
        "hydra-mobile".to_string()
    } else {
        config.relay.device_id.clone()
    };
    crate::api::quota::init_quota(relay_endpoint, device_id);

    // Store the SOCKS5 port for tun2proxy to read
    crate::api::vpn::SOCKS5_PORT.store(
        config.network.socks5_port,
        std::sync::atomic::Ordering::Relaxed,
    );

    let econ = match EconLedger::new(config.econ.db_path.to_str().unwrap()) {
        Ok(e) => {
            tracing::info!("Economic ledger initialized");
            Arc::new(e)
        }
        Err(e) => {
            tracing::error!("Failed to init EconLedger: {}. Check that no other Hydra instance is using the same db_path.", e);
            return Err(anyhow::anyhow!("EconLedger init failed: {}", e));
        }
    };

    let ai = Arc::new(AiNegotiator::new(&config.ai));
    {
        let mut shared = crate::api::model_manager::SHARED_AI.lock().await;
        *shared = Some(ai.clone());
    }

    if config.ai.model_path.exists() {
        tracing::info!(
            "AI model available at {} — will load on first use (lazy) to conserve memory.",
            config.ai.model_path.display()
        );
    } else {
        tracing::info!(
            "AI model not found: {}. LLM features disabled until model is downloaded.",
            config.ai.model_path.display()
        );
    }

    tracing::info!("Initializing P2P Node & mDNS discovery...");
    let p2p_handle = match P2PNode::new(None, config.network.p2p_listen_port, &config.network).await
    {
        Ok((p2p_node, handle)) => {
            tokio::spawn(async move {
                if let Err(e) = p2p_node.run().await {
                    tracing::error!("P2P node error: {}", e);
                }
            });
            tracing::info!("P2P node started");
            handle
        }
        Err(e) => {
            tracing::warn!(
                "P2P node init failed (expected on Android — no /etc/resolv.conf): {}. \
                 Continuing without peer discovery; relay-only mode.",
                e
            );
            P2PNode::dummy_handle()
        }
    };

    let addr = SocketAddr::from(([127, 0, 0, 1], config.network.socks5_port));
    tracing::info!("Starting SOCKS5 Server on {}", addr);
    let server = Socks5Server::new(addr, ai, p2p_handle, econ, &config.relay);

    // Store registry and relay_mode handle for FRB API access
    {
        let mut shared = SHARED_REGISTRY.lock().await;
        *shared = Some(server.registry());
    }
    {
        let mut shared = SHARED_RELAY_MODE.lock().await;
        *shared = Some(server.relay_mode_handle());
    }

    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("Socks5 server error: {}", e);
        }
    });

    ensure_snapshot_writer();

    tracing::info!("Hydra Core ready!");
    Ok(())
}

/// Get active connections as JSON for Flutter UI.
pub async fn get_active_connections() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let snaps = registry.snapshot(false);

    let json_arr: Vec<serde_json::Value> = snaps
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "target_host": s.target_host,
                "target_port": s.target_port,
                "route_type": s.route_type,
                "bytes_up": s.bytes_up,
                "bytes_down": s.bytes_down,
                "duration_ms": s.duration_ms,
                "is_telegram": s.is_telegram,
                "is_proxied": s.is_proxied,
                "status": s.status,
                "ai_reason": s.ai_reason,
            })
        })
        .collect();

    let json = serde_json::to_string(&json_arr)?;
    let _ = crate::api::shared_state::persist_active_connections(&json);
    Ok(json)
}

/// Get connection stats summary as JSON.
pub async fn get_connection_stats() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let stats = registry.stats();

    let json = serde_json::json!({
        "active_count": stats.active_count,
        "total_count": stats.total_count,
        "proxied_count": stats.proxied_count,
        "total_bytes_up": stats.total_bytes_up,
        "total_bytes_down": stats.total_bytes_down,
    })
    .to_string();
    let _ = crate::api::shared_state::persist_connection_stats(&json);
    Ok(json)
}

/// Toggle proxy for a specific connection.
pub async fn set_connection_proxy(conn_id: u64, proxied: bool) -> anyhow::Result<()> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    registry.set_force_proxy(conn_id, proxied);
    Ok(())
}

fn ensure_snapshot_writer() {
    if SNAPSHOT_TASK_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    tokio::spawn(async {
        loop {
            if let Ok(json) = get_active_connections().await {
                let _ = crate::api::shared_state::persist_active_connections(&json);
            }
            if let Ok(json) = get_connection_stats().await {
                let _ = crate::api::shared_state::persist_connection_stats(&json);
            }
            let _ = crate::api::shared_state::persist_quota_status(
                &crate::api::quota::get_quota_status(),
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

/// Set proxy mode at runtime. Values: "off", "telegram", "full".
/// Called from Flutter Settings when user changes proxy mode.
pub async fn set_proxy_mode(mode: String) -> anyhow::Result<()> {
    let guard = SHARED_RELAY_MODE.lock().await;
    let relay_mode = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    if let Ok(mut m) = relay_mode.write() {
        tracing::info!("Proxy mode changed to: {}", mode);
        *m = mode;
    }
    Ok(())
}

/// Async LLM analysis of connection security/quality.
/// Accepts a JSON description of connections, returns LLM text recommendation.
/// Returns a fallback message if AI model is not loaded (instead of erroring).
pub async fn analyze_connections(connections_json: String) -> anyhow::Result<String> {
    let ai_guard = crate::api::model_manager::SHARED_AI.lock().await;
    let ai = match ai_guard.as_ref() {
        Some(a) => a,
        None => return Ok("[OK] AI model not loaded. Download a model in Settings > AI Models for security analysis.".to_string()),
    };
    let mut infer_guard = ai.infer().lock().await;
    let infer = match infer_guard.as_mut() {
        Some(i) => i,
        None => return Ok("[OK] AI model not loaded. Download a model in Settings > AI Models for security analysis.".to_string()),
    };

    let prompt = format!(
        "<|im_start|>system\nYou are a network security analyst for a VPN/proxy app. \
        Analyze the active connections and provide a brief security assessment. \
        Focus on: suspicious destinations, unencrypted traffic (port 80), \
        high-volume transfers, Telegram routing status. \
        Be concise (2-4 sentences). Use [OK], [WARN], [ALERT] tags.\n<|im_end|>\n\
        <|im_start|>user\nActive connections:\n{}\n<|im_end|>\n<|im_start|>assistant\n",
        connections_json
    );

    match infer.generate(&prompt, 200) {
        Ok(response) => Ok(response.trim().to_string()),
        Err(e) => Err(anyhow::anyhow!("LLM analysis failed: {}", e)),
    }
}

/// Async LLM analysis for a single host — security recommendation.
/// Returns a fallback message if AI model is not loaded (instead of erroring).
pub async fn analyze_host(
    host: String,
    port: u16,
    is_proxied: bool,
    bytes_total: u64,
) -> anyhow::Result<String> {
    let ai_guard = crate::api::model_manager::SHARED_AI.lock().await;
    let ai = match ai_guard.as_ref() {
        Some(a) => a,
        None => return Ok("[OK] AI not loaded".to_string()),
    };
    let mut infer_guard = ai.infer().lock().await;
    let infer = match infer_guard.as_mut() {
        Some(i) => i,
        None => return Ok("[OK] AI not loaded".to_string()),
    };

    let proxy_status = if is_proxied {
        "proxied via relay"
    } else {
        "direct connection"
    };
    let prompt = format!(
        "<|im_start|>system\nYou are a network security advisor. \
        Give a 1-sentence security note about this connection. \
        Use [OK], [WARN] or [ALERT] prefix.\n<|im_end|>\n\
        <|im_start|>user\nHost: {}:{}, Status: {}, Transferred: {} bytes\n<|im_end|>\n\
        <|im_start|>assistant\n",
        host, port, proxy_status, bytes_total
    );

    match infer.generate(&prompt, 100) {
        Ok(response) => Ok(response.trim().to_string()),
        Err(e) => Err(anyhow::anyhow!("LLM host analysis failed: {}", e)),
    }
}
