use anyhow::Result;
use hydra_ai::AiNegotiator;
use hydra_config::HydraConfig;
use hydra_core::{Socks5Server, discovery::RouteDiscoveryService, transport};
use hydra_econ::{EconLedger, provider::ProviderMetricsLedger};
use hydra_exchange::ExchangeConfig;
use hydra_p2p::P2PNode;
use libp2p::identity::Keypair;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Load configuration (hydra.toml in current directory, or defaults)
    let config = HydraConfig::load(&PathBuf::from("hydra.toml"))?;

    // Check if we are running as a bootstrap node
    let is_bootstrap = std::env::args().any(|arg| arg == "--bootstrap");

    // Load or create identity
    let id_path = PathBuf::from("hydra_identity.bin");
    let keypair = if id_path.exists() {
        let bytes = std::fs::read(&id_path)?;
        Keypair::from_protobuf_encoding(&bytes)?
    } else {
        let kp = Keypair::generate_ed25519();
        std::fs::write(&id_path, kp.to_protobuf_encoding()?)?;
        kp
    };

    tracing::info!("Using PeerID: {}", keypair.public().to_peer_id());

    // Initialize Economic Ledger
    let econ = Arc::new(EconLedger::new(config.econ.db_path.to_str().unwrap())?);

    // Initialize AI Negotiator
    let ai = Arc::new(AiNegotiator::new(&config.ai));

    // Load AI Model (llama.cpp — tokenizer is embedded in GGUF)
    if config.ai.model_path.exists() {
        if let Err(e) = ai.load_model(config.ai.model_path.clone()).await {
            tracing::error!(
                "Failed to load AI model: {}. Falling back to heuristic routing.",
                e
            );
        }
    } else {
        tracing::warn!(
            "AI model not found: {}. Using heuristic routing.",
            config.ai.model_path.display()
        );
    }

    // Start P2P Node
    let (p2p_node, p2p_handle) = {
        let port = if is_bootstrap {
            config.bootstrap.listen_port
        } else {
            config.network.p2p_listen_port
        };
        P2PNode::new(Some(keypair), port, &config.network).await?
    };
    tokio::spawn(async move {
        if let Err(e) = p2p_node.run().await {
            tracing::error!("P2P node error: {}", e);
        }
    });

    if is_bootstrap {
        tracing::info!("Running as Bootstrap Node. SOCKS5 disabled.");
        std::future::pending::<()>().await;
    } else {
        let transports = transport::build_transports(&config.transports)?;
        let provider_metrics = Arc::new(ProviderMetricsLedger::new(
            config.econ.db_path.join("provider_metrics"),
        )?);
        let discovery = if config.crypto.enabled {
            match ExchangeConfig::from_crypto_config(&config.crypto) {
                Ok(exchange) => Some(Arc::new(RouteDiscoveryService::new(
                    exchange,
                    config.discovery.clone(),
                    Some(provider_metrics.clone()),
                ))),
                Err(error) => {
                    tracing::warn!("Discovery disabled: {}", error);
                    None
                }
            }
        } else {
            None
        };
        let addr = SocketAddr::from(([127, 0, 0, 1], config.network.socks5_port));
        let server = Socks5Server::new(
            addr,
            ai,
            p2p_handle,
            econ,
            transports,
            config.network.proxy_mode.clone(),
            discovery,
            None,
            Some(provider_metrics),
            None,
            None,
            &config.intelligence,
        )?;
        server.run().await?;
    }

    Ok(())
}
