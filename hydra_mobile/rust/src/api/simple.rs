#[flutter_rust_bridge::frb(sync)] // Synchronous execution for simple calls
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
use hydra_p2p::P2PNode;
use hydra_econ::EconLedger;
use hydra_ai::AiNegotiator;
use std::net::SocketAddr;

pub async fn start_hydra_node(base_dir: String) -> anyhow::Result<()> {
    tracing::info!("Starting real Hydra mobile node...");

    let base_path = PathBuf::from(&base_dir);
    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;

    // Initialize Economic Ledger
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

    // Initialize AI Negotiator and register for model hot-reloading
    let ai = Arc::new(AiNegotiator::new(&config.ai));
    {
        let mut shared = crate::api::model_manager::SHARED_AI.lock().await;
        *shared = Some(ai.clone());
    }

    // Load AI Model
    if config.ai.model_path.exists() && config.ai.tokenizer_path.exists() {
        if let Err(e) = ai.load_model(
            config.ai.model_path.clone(),
            config.ai.tokenizer_path.clone(),
        ).await {
            tracing::error!("Failed to load AI model: {}", e);
        } else {
            tracing::info!("AI Model loaded successfully!");
        }
    } else {
        tracing::warn!(
            "AI model or tokenizer missing: model={}, tokenizer={}. Using heuristic routing.",
            config.ai.model_path.display(),
            config.ai.tokenizer_path.display()
        );
    }

    // Start P2P Node
    tracing::info!("Initializing P2P Node & mDNS discovery...");
    let (p2p_node, p2p_handle) = P2PNode::new(None, config.network.p2p_listen_port, &config.network).await?;
    tokio::spawn(async move {
        if let Err(e) = p2p_node.run().await {
            tracing::error!("P2P node error: {}", e);
        }
    });

    // Start Socks5 Server
    let addr = SocketAddr::from(([127, 0, 0, 1], config.network.socks5_port));
    tracing::info!("Starting SOCKS5 Server on {}", addr);
    let server = Socks5Server::new(addr, ai, p2p_handle, econ);

    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("Socks5 server error: {}", e);
        }
    });

    tracing::info!("Hydra Core ready!");
    Ok(())
}
