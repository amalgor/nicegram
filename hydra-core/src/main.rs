use anyhow::Result;
use hydra_ai::AiNegotiator;
use hydra_core::Socks5Server;
use hydra_econ::EconLedger;
use hydra_p2p::P2PNode;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    // Initialize Economic Ledger
    let econ = Arc::new(EconLedger::new("hydra_db")?);

    // Initialize AI Negotiator
    let ai = Arc::new(AiNegotiator::new());

    // Load AI Model
    let model_path = PathBuf::from("models/qwen2.5-1.5b-instruct-q4_k_m.gguf");
    let tokenizer_path = PathBuf::from("models/tokenizer.json");
    if model_path.exists() && tokenizer_path.exists() {
        if let Err(e) = ai.load_model(model_path, tokenizer_path).await {
            tracing::error!(
                "Failed to load AI model: {}. Falling back to heuristic routing.",
                e
            );
        }
    } else {
        tracing::warn!(
            "AI model files not found in 'models/' directory. Using heuristic routing. Download them with: \n\
            mkdir -p models && wget -O models/qwen2.5-1.5b-instruct-q4_k_m.gguf https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf\n\
            wget -O models/tokenizer.json https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct/resolve/main/tokenizer.json"
        );
    }

    // Start P2P Node
    let (p2p_node, p2p_handle) = P2PNode::new().await?;
    tokio::spawn(async move {
        if let Err(e) = p2p_node.run().await {
            tracing::error!("P2P node error: {}", e);
        }
    });

    // Start Socks5 Server
    let addr = SocketAddr::from(([127, 0, 0, 1], 1080));
    let server = Socks5Server::new(addr, ai, p2p_handle, econ);

    server.run().await?;

    Ok(())
}
