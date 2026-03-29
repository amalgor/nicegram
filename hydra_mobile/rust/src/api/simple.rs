#[flutter_rust_bridge::frb(sync)]
pub fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

pub fn init_app() {
    use tracing_subscriber::layer::SubscriberExt;
    let subscriber = tracing_subscriber::registry()
        .with(crate::api::telemetry::FlutterLogLayer);
    let _ = tracing::subscriber::set_global_default(subscriber);
    
    tracing::info!("Hydra P2P node initialized and ready.");
}

use std::sync::Arc;
use std::path::PathBuf;
use hydra_config::HydraConfig;
use hydra_core::Socks5Server;
use hydra_core::connections::ConnectionRegistry;
use hydra_p2p::P2PNode;
use hydra_econ::EconLedger;
use hydra_ai::AiNegotiator;
use std::net::SocketAddr;

lazy_static::lazy_static! {
    static ref SHARED_REGISTRY: tokio::sync::Mutex<Option<Arc<ConnectionRegistry>>> =
        tokio::sync::Mutex::new(None);
}

pub async fn start_hydra_node(base_dir: String) -> anyhow::Result<()> {
    tracing::info!("Starting real Hydra mobile node...");

    let base_path = PathBuf::from(&base_dir);
    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;

    // Store the SOCKS5 port for tun2proxy to read
    crate::api::vpn::SOCKS5_PORT.store(config.network.socks5_port, std::sync::atomic::Ordering::Relaxed);

    let econ = match EconLedger::new(config.econ.db_path.to_str().unwrap()) {
        Ok(e) => {
            tracing::info!("Economic ledger initialized");
            Arc::new(e)
        },
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
        if let Err(e) = ai.load_model(config.ai.model_path.clone()).await {
            tracing::error!("Failed to load AI model: {}", e);
        } else {
            tracing::info!("AI Model loaded successfully!");
        }
    } else {
        tracing::warn!(
            "AI model not found: {}. Using heuristic routing.",
            config.ai.model_path.display()
        );
    }

    tracing::info!("Initializing P2P Node & mDNS discovery...");
    let (p2p_node, p2p_handle) = P2PNode::new(None, config.network.p2p_listen_port, &config.network).await?;
    tokio::spawn(async move {
        if let Err(e) = p2p_node.run().await {
            tracing::error!("P2P node error: {}", e);
        }
    });

    let addr = SocketAddr::from(([127, 0, 0, 1], config.network.socks5_port));
    tracing::info!("Starting SOCKS5 Server on {}", addr);
    let server = Socks5Server::new(addr, ai, p2p_handle, econ, &config.relay);

    // Store registry for FRB API access
    {
        let mut shared = SHARED_REGISTRY.lock().await;
        *shared = Some(server.registry());
    }

    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("Socks5 server error: {}", e);
        }
    });

    tracing::info!("Hydra Core ready!");
    Ok(())
}

/// Get active connections as JSON for Flutter UI.
pub async fn get_active_connections() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let snaps = registry.snapshot(false);

    let json_arr: Vec<serde_json::Value> = snaps.iter().map(|s| {
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
    }).collect();

    Ok(serde_json::to_string(&json_arr)?)
}

/// Get connection stats summary as JSON.
pub async fn get_connection_stats() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let stats = registry.stats();

    Ok(serde_json::json!({
        "active_count": stats.active_count,
        "total_count": stats.total_count,
        "proxied_count": stats.proxied_count,
        "total_bytes_up": stats.total_bytes_up,
        "total_bytes_down": stats.total_bytes_down,
    }).to_string())
}

/// Toggle proxy for a specific connection.
pub async fn set_connection_proxy(conn_id: u64, proxied: bool) -> anyhow::Result<()> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard.as_ref().ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    registry.set_force_proxy(conn_id, proxied);
    Ok(())
}
