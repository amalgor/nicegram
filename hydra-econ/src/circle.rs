use anyhow::{Context, Result};
use hydra_config::CryptoConfig;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::info;

const CIRCLE_API_BASE: &str = "https://api.circle.com/v1/w3s";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub id: String,
    pub address: String,
    pub blockchain: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBalance {
    pub token_id: String,
    pub amount: String,
    pub blockchain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionResult {
    pub id: String,
    pub state: String,
}

pub struct CircleSettlement {
    client: Client,
    api_key: String,
    entity_secret: String,
    chain: String,
    wallet_set_id: String,
}

impl CircleSettlement {
    pub fn new(config: &CryptoConfig) -> Result<Self> {
        if config.circle_api_key.is_empty() {
            return Err(anyhow::anyhow!(
                "Circle API key not configured. Set [crypto].circle_api_key in hydra.toml."
            ));
        }

        Ok(Self {
            client: Client::new(),
            api_key: config.circle_api_key.clone(),
            entity_secret: config.entity_secret.clone(),
            chain: config.settlement_chain.clone(),
            wallet_set_id: config.wallet_set_id.clone(),
        })
    }

    /// Create a wallet set if none exists. Returns wallet set ID.
    pub async fn ensure_wallet_set(&mut self) -> Result<String> {
        if !self.wallet_set_id.is_empty() {
            return Ok(self.wallet_set_id.clone());
        }

        let body = serde_json::json!({
            "idempotencyKey": uuid::Uuid::new_v4().to_string(),
            "name": "hydra-node-wallets",
            "entitySecretCiphertext": self.entity_secret,
        });

        let resp = self
            .client
            .post(format!("{}/developer/walletSets", CIRCLE_API_BASE))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to create wallet set")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Circle wallet set creation failed ({}): {}",
                status,
                text
            ));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        let id = parsed["data"]["walletSet"]["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing walletSet.id in response: {}", text))?
            .to_string();

        info!("Created Circle wallet set: {}", id);
        self.wallet_set_id = id.clone();
        Ok(id)
    }

    /// Create a new wallet in the wallet set for the given blockchain.
    pub async fn create_wallet(&self) -> Result<Wallet> {
        let body = serde_json::json!({
            "idempotencyKey": uuid::Uuid::new_v4().to_string(),
            "walletSetId": self.wallet_set_id,
            "blockchains": [self.chain],
            "count": 1,
            "accountType": "EOA",
            "entitySecretCiphertext": self.entity_secret,
        });

        let resp = self
            .client
            .post(format!("{}/developer/wallets", CIRCLE_API_BASE))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to create wallet")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Circle wallet creation failed ({}): {}",
                status,
                text
            ));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        let wallet_data = &parsed["data"]["wallets"][0];

        Ok(Wallet {
            id: wallet_data["id"].as_str().unwrap_or("").to_string(),
            address: wallet_data["address"].as_str().unwrap_or("").to_string(),
            blockchain: wallet_data["blockchain"].as_str().unwrap_or("").to_string(),
            state: wallet_data["state"].as_str().unwrap_or("").to_string(),
        })
    }

    /// Get USDC balance for a wallet.
    pub async fn get_balance(&self, wallet_id: &str) -> Result<Vec<TokenBalance>> {
        let resp = self
            .client
            .get(format!(
                "{}/developer/wallets/{}/balances",
                CIRCLE_API_BASE, wallet_id
            ))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("Failed to get wallet balance")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Circle balance check failed ({}): {}",
                status,
                text
            ));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        let balances = parsed["data"]["tokenBalances"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|b| TokenBalance {
                        token_id: b["token"]["id"].as_str().unwrap_or("").to_string(),
                        amount: b["amount"].as_str().unwrap_or("0").to_string(),
                        blockchain: b["token"]["blockchain"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(balances)
    }

    /// Transfer USDC from one wallet to another.
    /// amount_usdc is in human-readable form (e.g., "1.50" for 1.50 USDC).
    pub async fn transfer_usdc(
        &self,
        from_wallet_id: &str,
        to_address: &str,
        amount_usdc: &str,
    ) -> Result<TransactionResult> {
        // USDC token ID for testnet — this varies by chain
        // For Arbitrum Sepolia testnet USDC
        let body = serde_json::json!({
            "idempotencyKey": uuid::Uuid::new_v4().to_string(),
            "entitySecretCiphertext": self.entity_secret,
            "walletId": from_wallet_id,
            "tokenId": self.usdc_token_id(),
            "destinationAddress": to_address,
            "amounts": [amount_usdc],
            "blockchain": self.chain,
        });

        let resp = self
            .client
            .post(format!(
                "{}/developer/transactions/transfer",
                CIRCLE_API_BASE
            ))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("Failed to initiate USDC transfer")?;

        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "Circle transfer failed ({}): {}",
                status,
                text
            ));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text)?;
        let tx = &parsed["data"];

        Ok(TransactionResult {
            id: tx["id"].as_str().unwrap_or("").to_string(),
            state: tx["state"].as_str().unwrap_or("INITIATED").to_string(),
        })
    }

    /// Poll transaction until terminal state.
    pub async fn wait_for_transaction(&self, tx_id: &str) -> Result<String> {
        for _ in 0..30 {
            let resp = self
                .client
                .get(format!(
                    "{}/developer/transactions/{}",
                    CIRCLE_API_BASE, tx_id
                ))
                .bearer_auth(&self.api_key)
                .send()
                .await?;

            let text = resp.text().await?;
            let parsed: serde_json::Value = serde_json::from_str(&text)?;
            let state = parsed["data"]["transaction"]["state"]
                .as_str()
                .unwrap_or("UNKNOWN");

            match state {
                "COMPLETE" | "FAILED" | "DENIED" | "CANCELLED" => {
                    info!("Transaction {} reached terminal state: {}", tx_id, state);
                    return Ok(state.to_string());
                }
                _ => {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                }
            }
        }
        Err(anyhow::anyhow!(
            "Transaction {} did not reach terminal state within timeout",
            tx_id
        ))
    }

    fn usdc_token_id(&self) -> &str {
        match self.chain.as_str() {
            "ARB-SEPOLIA" => "0x75faf114eafb1BDbe2F0316DF893fd58CE46AA4d",
            "ETH-SEPOLIA" => "0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238",
            "MATIC-AMOY" => "0x41E94Eb71Ef8C9fAB0052b2eC2C9C6015A1E6B0B",
            _ => "",
        }
    }
}
