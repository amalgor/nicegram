use alloy::primitives::Address;
use anyhow::{Context, Result};
use hydra_config::HydraConfig;
use hydra_exchange::{
    AgentRegistrar, CreateOfferInput, ExchangeConfig, LocalWallet, ReputationClient,
    RouteExchangeClient,
};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
struct MarketplaceConfigStatus {
    state: String,
    ready: bool,
    enabled: bool,
    reputation_enabled: bool,
    chain: String,
    rpc_url: String,
    route_book_address: String,
    identity_registry_address: String,
    reputation_registry_address: String,
    usdc_address: String,
    message: String,
}

impl MarketplaceConfigStatus {
    fn disabled(config: &HydraConfig, message: impl Into<String>) -> Self {
        Self {
            state: "disabled".to_string(),
            ready: false,
            enabled: false,
            reputation_enabled: false,
            chain: config.crypto.chain.clone(),
            rpc_url: config.crypto.rpc_url.clone(),
            route_book_address: config.crypto.route_book_address.clone(),
            identity_registry_address: config.crypto.identity_registry_address.clone(),
            reputation_registry_address: config.crypto.reputation_registry_address.clone(),
            usdc_address: config.crypto.usdc_address.clone(),
            message: message.into(),
        }
    }

    fn incomplete(config: &HydraConfig, message: impl Into<String>) -> Self {
        Self {
            state: "incomplete".to_string(),
            ready: false,
            enabled: config.crypto.enabled,
            reputation_enabled: false,
            chain: config.crypto.chain.clone(),
            rpc_url: config.crypto.rpc_url.clone(),
            route_book_address: config.crypto.route_book_address.clone(),
            identity_registry_address: config.crypto.identity_registry_address.clone(),
            reputation_registry_address: config.crypto.reputation_registry_address.clone(),
            usdc_address: config.crypto.usdc_address.clone(),
            message: message.into(),
        }
    }

    fn ready(config: &HydraConfig, parsed: &ExchangeConfig) -> Self {
        Self {
            state: "ready".to_string(),
            ready: true,
            enabled: config.crypto.enabled,
            reputation_enabled: parsed.reputation_registry_address.is_some(),
            chain: config.crypto.chain.clone(),
            rpc_url: config.crypto.rpc_url.clone(),
            route_book_address: config.crypto.route_book_address.clone(),
            identity_registry_address: config.crypto.identity_registry_address.clone(),
            reputation_registry_address: config.crypto.reputation_registry_address.clone(),
            usdc_address: config.crypto.usdc_address.clone(),
            message: "Marketplace is configured for Base Sepolia.".to_string(),
        }
    }
}

fn load_exchange_config() -> Result<ExchangeConfig> {
    let base_dir = crate::api::shared_state::shared_base_dir()
        .ok_or_else(|| anyhow::anyhow!("Shared base dir not initialized"))?;
    let config_path = base_dir.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_dir)
        .with_context(|| format!("Failed to load config from {}", config_path.display()))?;
    ExchangeConfig::from_crypto_config(&config.crypto)
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

#[flutter_rust_bridge::frb(sync)]
pub fn get_marketplace_config_status() -> Result<String> {
    let Some(base_dir) = crate::api::shared_state::shared_base_dir() else {
        let status = MarketplaceConfigStatus {
            state: "incomplete".to_string(),
            ready: false,
            enabled: false,
            reputation_enabled: false,
            chain: String::new(),
            rpc_url: String::new(),
            route_book_address: String::new(),
            identity_registry_address: String::new(),
            reputation_registry_address: String::new(),
            usdc_address: String::new(),
            message: "Shared base dir is not initialized yet.".to_string(),
        };
        return to_json(&status);
    };

    let config_path = base_dir.join("hydra.toml");
    let config = match HydraConfig::load_with_base_dir(&config_path, &base_dir) {
        Ok(config) => config,
        Err(error) => {
            let status = MarketplaceConfigStatus {
                state: "incomplete".to_string(),
                ready: false,
                enabled: false,
                reputation_enabled: false,
                chain: String::new(),
                rpc_url: String::new(),
                route_book_address: String::new(),
                identity_registry_address: String::new(),
                reputation_registry_address: String::new(),
                usdc_address: String::new(),
                message: format!("Failed to load hydra.toml: {error}"),
            };
            return to_json(&status);
        }
    };

    let status = if !config.crypto.enabled {
        MarketplaceConfigStatus::disabled(
            &config,
            "Marketplace is disabled. Set [crypto].enabled = true in hydra.toml.",
        )
    } else {
        match ExchangeConfig::from_crypto_config(&config.crypto) {
            Ok(parsed) => MarketplaceConfigStatus::ready(&config, &parsed),
            Err(error) => MarketplaceConfigStatus::incomplete(&config, error.to_string()),
        }
    };

    to_json(&status)
}

#[flutter_rust_bridge::frb(sync)]
pub fn create_wallet() -> Result<String> {
    to_json(&LocalWallet::generate()?)
}

#[flutter_rust_bridge::frb(sync)]
pub fn get_wallet_preview(mnemonic: String) -> Result<String> {
    to_json(&LocalWallet::import(&mnemonic)?)
}

#[flutter_rust_bridge::frb(sync)]
pub fn import_wallet(mnemonic: String) -> Result<String> {
    to_json(&LocalWallet::import(&mnemonic)?)
}

pub async fn get_wallet_balances(address: String) -> Result<String> {
    let parsed = address
        .trim()
        .parse::<Address>()
        .with_context(|| format!("Invalid wallet address '{}'", address))?;
    let client = RouteExchangeClient::new(load_exchange_config()?);
    to_json(&client.wallet_balances(parsed).await?)
}

pub async fn list_route_offers(region: String, protocol: String) -> Result<String> {
    let client = RouteExchangeClient::new(load_exchange_config()?);
    to_json(&client.query_offers(&region, &protocol).await?)
}

pub async fn register_agent(mnemonic: String) -> Result<String> {
    let registrar = AgentRegistrar::new(load_exchange_config()?);
    to_json(&registrar.register(&mnemonic).await?)
}

pub async fn create_offer(
    mnemonic: String,
    agent_id: u64,
    endpoint_url: String,
    protocols: Vec<String>,
    region: String,
    price_per_gb_raw: String,
    stake_amount_raw: String,
    bandwidth_mbps: u64,
) -> Result<String> {
    let client = RouteExchangeClient::new(load_exchange_config()?);
    to_json(
        &client
            .create_offer(
                &mnemonic,
                CreateOfferInput {
                    agent_id,
                    endpoint_url,
                    protocols,
                    region,
                    price_per_gb_raw,
                    stake_amount_raw,
                    bandwidth_mbps,
                },
            )
            .await?,
    )
}

pub async fn deactivate_offer(mnemonic: String, offer_id: u64) -> Result<String> {
    let client = RouteExchangeClient::new(load_exchange_config()?);
    to_json(&client.deactivate_offer(&mnemonic, offer_id).await?)
}

pub async fn withdraw_stake(mnemonic: String, offer_id: u64) -> Result<String> {
    let client = RouteExchangeClient::new(load_exchange_config()?);
    to_json(&client.withdraw_stake(&mnemonic, offer_id).await?)
}

pub async fn submit_feedback(
    mnemonic: String,
    agent_id: u64,
    positive: bool,
    tag1: String,
) -> Result<String> {
    let client = ReputationClient::new(load_exchange_config()?);
    to_json(&client.give_feedback(&mnemonic, agent_id, positive, &tag1).await?)
}

// ── P2P Deal Board API ─────────────────────────────────────────────────

pub async fn list_deal_offers(currency: String) -> Result<String> {
    let client = hydra_exchange::DealBoardClient::new(load_exchange_config()?);
    to_json(&client.query_deals(&currency).await?)
}

pub async fn get_deal_offer(offer_id: u64) -> Result<String> {
    let client = hydra_exchange::DealBoardClient::new(load_exchange_config()?);
    to_json(&client.get_offer(offer_id).await?)
}

pub async fn accept_deal(mnemonic: String, offer_id: u64, usdc_amount: String) -> Result<String> {
    let client = hydra_exchange::DealBoardClient::new(load_exchange_config()?);
    to_json(&client.accept_deal(offer_id, &usdc_amount, &mnemonic).await?)
}

pub async fn mark_fiat_sent(mnemonic: String, escrow_id: u64) -> Result<String> {
    let client = hydra_exchange::DealBoardClient::new(load_exchange_config()?);
    to_json(&client.mark_sent(escrow_id, &mnemonic).await?)
}

pub async fn check_escrow_status(escrow_id: u64) -> Result<String> {
    let client = hydra_exchange::DealBoardClient::new(load_exchange_config()?);
    to_json(&client.check_status(escrow_id).await?)
}

pub async fn claim_expired_escrow(mnemonic: String, escrow_id: u64) -> Result<String> {
    let client = hydra_exchange::DealBoardClient::new(load_exchange_config()?);
    to_json(&client.claim_expired(escrow_id, &mnemonic).await?)
}

pub async fn approve_deal_board_usdc(mnemonic: String, amount: String) -> Result<String> {
    let client = hydra_exchange::DealBoardClient::new(load_exchange_config()?);
    to_json(&client.approve_usdc(&amount, &mnemonic).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn init_test_base_dir() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base_dir = std::env::temp_dir().join(format!("hydra-exchange-api-test-{}", unique));
        fs::create_dir_all(&base_dir).unwrap();
        crate::api::shared_state::init_shared_base_dir(&base_dir).unwrap();
        base_dir
    }

    #[test]
    fn create_wallet_returns_json_payload() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        let json = create_wallet().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            parsed["mnemonic"]
                .as_str()
                .unwrap()
                .split_whitespace()
                .count(),
            12
        );
        assert!(parsed["address"].as_str().unwrap().starts_with("0x"));
    }

    #[test]
    fn load_exchange_config_requires_initialized_crypto_section() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        let base_dir = init_test_base_dir();
        fs::write(base_dir.join("hydra.toml"), "[network]\nsocks5_port = 1080\n").unwrap();

        let err = load_exchange_config().expect_err("config should be incomplete");
        assert!(err.to_string().contains("Marketplace is disabled"));
    }

    #[test]
    fn marketplace_config_status_reports_disabled_and_incomplete_states() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        let base_dir = init_test_base_dir();
        fs::write(
            base_dir.join("hydra.toml"),
            r#"
[crypto]
enabled = false
chain = "BASE-SEPOLIA"
rpc_url = "https://sepolia.base.org"
"#,
        )
        .unwrap();

        let disabled: serde_json::Value =
            serde_json::from_str(&get_marketplace_config_status().unwrap()).unwrap();
        assert_eq!(disabled["state"], "disabled");
        assert_eq!(disabled["ready"], false);

        fs::write(
            base_dir.join("hydra.toml"),
            r#"
[crypto]
enabled = true
chain = "BASE-SEPOLIA"
rpc_url = "https://sepolia.base.org"
identity_registry_address = "0x8004A818BFB912233c491871b3d84c89A494BD9e"
reputation_registry_address = "0x8004B663056A597Dffe9eCcC1965A193B7388713"
usdc_address = "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
"#,
        )
        .unwrap();

        let incomplete: serde_json::Value =
            serde_json::from_str(&get_marketplace_config_status().unwrap()).unwrap();
        assert_eq!(incomplete["state"], "incomplete");
        assert_eq!(incomplete["ready"], false);
        assert!(
            incomplete["message"]
                .as_str()
                .unwrap()
                .contains("route_book_address")
        );
    }
}
