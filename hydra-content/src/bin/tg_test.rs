use anyhow::Result;
use grammers_client::peer::Peer;
use grammers_session::types::PeerRef;
use hydra_ai::AiNegotiator;
use hydra_config::HydraConfig;
use hydra_content::models::FoldLevel;
use hydra_content::processing::summarizer::Summarizer;
use hydra_content::telegram::client::{AuthState, TelegramClient};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let config_path = PathBuf::from("hydra.toml");
    let config = HydraConfig::load(&config_path)?;

    if config.telegram.api_id == 0 {
        eprintln!("[ERR] Telegram API credentials not set in hydra.toml");
        eprintln!("      Get yours at https://my.telegram.org/auth");
        std::process::exit(1);
    }

    println!("=== Hydra Content Intelligence Test ===");
    println!();

    // --- Phase 1: Connect & Auth ---
    let mut client = TelegramClient::new(config.telegram);
    println!("[...] Connecting to Telegram...");
    client.connect().await?;

    match client.state() {
        AuthState::Authorized { user_name } => {
            println!("[OK] Authorized as: {}", user_name);
        }
        AuthState::NeedPhone => {
            let phone = prompt("Enter phone number (e.g. +79001234567): ")?;
            client.send_phone(&phone).await?;
            let code = prompt("Enter login code: ")?;
            client.send_code(&code).await?;
            if let AuthState::NeedPassword { hint } = client.state() {
                println!("[...] 2FA required. Hint: {}", hint);
                let password = prompt("Enter 2FA password: ")?;
                client.send_password(&password).await?;
            }
            println!("[OK] Auth state: {:?}", client.state());
        }
        other => {
            eprintln!("[ERR] Unexpected state: {:?}", other);
            std::process::exit(1);
        }
    }

    let tg = client
        .client()
        .expect("Client must be available after auth");

    // --- Phase 2: Find news channels ---
    println!();
    println!("=== Loading dialogs ===");
    let mut dialogs = tg.iter_dialogs();
    let mut news_channels: Vec<(String, PeerRef)> = Vec::new();

    while let Some(dialog) = dialogs.next().await? {
        let peer = dialog.peer();
        let kind = match &peer {
            Peer::User(_) => "USER",
            Peer::Group(_) => "GROUP",
            Peer::Channel(ch) => {
                if ch.username().is_some() {
                    "PUBLIC_CH"
                } else {
                    "PRIVATE_CH"
                }
            }
        };
        let name = peer.name().unwrap_or("?").to_string();
        println!("  [{}] {}", kind, name);

        if let Peer::Channel(ch) = &peer {
            if ch.username().is_some() {
                if let Some(pr) = peer.to_ref().await {
                    news_channels.push((name, pr));
                }
            }
        }
    }

    if news_channels.is_empty() {
        println!("[WARN] No public channels found. Subscribe to some news channels.");
        client.disconnect();
        return Ok(());
    }

    // --- Phase 3: Read messages & summarize ---
    println!();
    println!("=== Content Intelligence: TLDR Folding ===");
    println!(
        "Found {} public channels. Reading last 3 messages from each (max 3 channels).",
        news_channels.len()
    );
    println!();

    let ai = Arc::new(AiNegotiator::new(&config.ai));

    // Try loading LLM model for real summarization (llama.cpp — tokenizer embedded in GGUF)
    if config.ai.model_path.exists() {
        println!(
            "[...] Loading AI model for summarization: {}",
            config.ai.model_path.display()
        );
        match ai.load_model(config.ai.model_path.clone()).await {
            Ok(_) => println!("[OK] AI model loaded. Summarization will use LLM."),
            Err(e) => println!(
                "[WARN] Failed to load model: {}. Using heuristic fallback.",
                e
            ),
        }
    } else {
        println!("[INFO] AI model not found. Using heuristic fallback for summarization.");
    }

    let summarizer = Summarizer::new(ai, config.content.summarization_max_tokens);

    for (name, peer_ref) in news_channels.iter().take(3) {
        println!("--- {} ---", name);

        let mut messages = tg.iter_messages(*peer_ref);
        let mut msg_count = 0;

        while let Some(message) = messages.next().await? {
            if msg_count >= 3 {
                break;
            }

            let text = message.text().to_string();
            if text.is_empty() || text.len() < 50 {
                continue;
            }

            println!();
            println!(
                "  [MSG #{}] {} ({} chars)",
                message.id(),
                message.date().format("%H:%M"),
                text.len()
            );

            if text.len() > 200 {
                let tree = summarizer.process(&text).await?;
                print_tree(&tree, 2);
            } else {
                println!("    [L0 HEADLINE] {}", text);
            }

            msg_count += 1;
        }

        if msg_count == 0 {
            println!("  (no text messages found)");
        }
        println!();
    }

    println!("=== Test complete ===");
    client.disconnect();
    Ok(())
}

fn print_tree(node: &hydra_content::models::ContentNode, indent: usize) {
    let prefix = " ".repeat(indent);
    let level_tag = match node.level {
        FoldLevel::Headline => "[L0 HEADLINE]",
        FoldLevel::Summary => "[L1 SUMMARY]",
        FoldLevel::KeyPoints => "[L2 KEY_POINTS]",
        FoldLevel::FullText => "[L3 FULL_TEXT]",
    };

    let display = if node.content.len() > 120 {
        let boundary = node
            .content
            .char_indices()
            .take_while(|(i, _)| *i <= 120)
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        format!("{}...", &node.content[..boundary])
    } else {
        node.content.clone()
    };

    println!("{}{} {}", prefix, level_tag, display);

    for child in &node.children {
        print_tree(child, indent + 2);
    }
}

fn prompt(msg: &str) -> Result<String> {
    print!("{}", msg);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}
