use anyhow::Result;
use hydra_config::HydraConfig;
use hydra_content::telegram::client::TelegramClient;
use std::io::{self, Write};
use std::path::PathBuf;

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

    println!("=== Hydra Telegram Connection Test ===");
    println!("api_id: {}", config.telegram.api_id);
    println!("session: {}", config.telegram.session_path.display());
    println!();

    let mut client = TelegramClient::new(config.telegram);

    println!("[...] Connecting to Telegram...");
    client.connect().await?;
    println!("[OK] Connected. State: {:?}", client.state());

    match client.state() {
        hydra_content::telegram::client::AuthState::Authorized { user_name } => {
            println!("[OK] Already authorized as: {}", user_name);
        }
        hydra_content::telegram::client::AuthState::NeedPhone => {
            let phone = prompt("Enter phone number (e.g. +79001234567): ")?;
            client.send_phone(&phone).await?;
            println!("[OK] Code sent. State: {:?}", client.state());

            let code = prompt("Enter login code: ")?;
            client.send_code(&code).await?;

            match client.state() {
                hydra_content::telegram::client::AuthState::NeedPassword { hint } => {
                    println!("[...] 2FA required. Hint: {}", hint);
                    let password = prompt("Enter 2FA password: ")?;
                    client.send_password(&password).await?;
                }
                _ => {}
            }

            println!("[OK] Auth state: {:?}", client.state());
        }
        other => {
            println!("[WARN] Unexpected state: {:?}", other);
        }
    }

    if let hydra_content::telegram::client::AuthState::Authorized { user_name } = client.state() {
        println!();
        println!("=== Authorized as: {} ===", user_name);
        println!("[...] Loading dialogs...");

        let tg = client.client().expect("Client must be available after auth");
        let mut dialogs = tg.iter_dialogs();
        let mut count = 0;

        while let Some(dialog) = dialogs.next().await? {
            let peer = dialog.peer();
            let kind = match &peer {
                grammers_client::peer::Peer::User(_) => "USER",
                grammers_client::peer::Peer::Group(_) => "GROUP",
                grammers_client::peer::Peer::Channel(ch) => {
                    if ch.username().is_some() { "PUBLIC_CH" } else { "PRIVATE_CH" }
                }
            };
            println!("  [{}] {} (id: {})", kind, peer.name().unwrap_or("?"), peer.id().bare_id());
            count += 1;
            if count >= 20 {
                println!("  ... (showing first 20)");
                break;
            }
        }
        println!();
        println!("[OK] Test complete. {} dialogs shown.", count);
    }

    client.disconnect();
    Ok(())
}

fn prompt(msg: &str) -> Result<String> {
    print!("{}", msg);
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().to_string())
}
