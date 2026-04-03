use alloy::primitives::Address;
use anyhow::{Context, Result, bail};
use hydra_config::CryptoConfig;
use url::Url;

pub const BASE_SEPOLIA_CHAIN: &str = "BASE-SEPOLIA";
pub const BASE_SEPOLIA_CHAIN_ID: u64 = 84_532;
pub const BASE_SEPOLIA_RPC_URL: &str = "https://sepolia.base.org";
pub const BASE_SEPOLIA_USDC: &str = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
pub const BASE_SEPOLIA_IDENTITY_REGISTRY: &str = "0x8004A818BFB912233c491871b3d84c89A494BD9e";
pub const BASE_SEPOLIA_REPUTATION_REGISTRY: &str = "0x8004B663056A597Dffe9eCcC1965A193B7388713";

#[derive(Debug, Clone)]
pub struct ExchangeConfig {
    pub chain: String,
    pub chain_id: u64,
    pub rpc_url: Url,
    pub route_book_address: Address,
    pub deal_board_address: Option<Address>,
    pub identity_registry_address: Address,
    pub reputation_registry_address: Option<Address>,
    pub usdc_address: Address,
}

impl ExchangeConfig {
    pub fn from_crypto_config(config: &CryptoConfig) -> Result<Self> {
        if !config.enabled {
            bail!("Marketplace is disabled. Set [crypto].enabled = true in hydra.toml.");
        }

        let chain = config.chain.trim().to_uppercase();
        if chain != BASE_SEPOLIA_CHAIN {
            bail!(
                "Unsupported chain '{}'. Phase 2 supports only {}.",
                config.chain,
                BASE_SEPOLIA_CHAIN
            );
        }

        let rpc_url = Url::parse(config.rpc_url.trim())
            .with_context(|| format!("Invalid [crypto].rpc_url '{}'", config.rpc_url))?;

        Ok(Self {
            chain,
            chain_id: BASE_SEPOLIA_CHAIN_ID,
            rpc_url,
            route_book_address: parse_required_address(
                "route_book_address",
                &config.route_book_address,
            )?,
            deal_board_address: parse_optional_address(
                "deal_board_address",
                &config.deal_board_address,
            )?,
            identity_registry_address: parse_required_address(
                "identity_registry_address",
                &config.identity_registry_address,
            )?,
            reputation_registry_address: parse_optional_address(
                "reputation_registry_address",
                &config.reputation_registry_address,
            )?,
            usdc_address: parse_required_address("usdc_address", &config.usdc_address)?,
        })
    }
}

fn parse_required_address(field: &str, value: &str) -> Result<Address> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("Missing [crypto].{} in hydra.toml.", field);
    }
    trimmed
        .parse::<Address>()
        .with_context(|| format!("Invalid [crypto].{} '{}'", field, value))
}

fn parse_optional_address(field: &str, value: &str) -> Result<Option<Address>> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    Ok(Some(
        trimmed
            .parse::<Address>()
            .with_context(|| format!("Invalid [crypto].{} '{}'", field, value))?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_exchange_config_for_base_sepolia() {
        let config = CryptoConfig {
            enabled: true,
            chain: BASE_SEPOLIA_CHAIN.to_string(),
            rpc_url: BASE_SEPOLIA_RPC_URL.to_string(),
            route_book_address: "0x1111111111111111111111111111111111111111".to_string(),
            deal_board_address: String::new(),
            identity_registry_address: BASE_SEPOLIA_IDENTITY_REGISTRY.to_string(),
            reputation_registry_address: BASE_SEPOLIA_REPUTATION_REGISTRY.to_string(),
            usdc_address: BASE_SEPOLIA_USDC.to_string(),
        };

        let parsed = ExchangeConfig::from_crypto_config(&config).unwrap();
        assert_eq!(parsed.chain, BASE_SEPOLIA_CHAIN);
        assert_eq!(parsed.chain_id, BASE_SEPOLIA_CHAIN_ID);
        assert_eq!(parsed.rpc_url.as_str(), "https://sepolia.base.org/");
        assert_eq!(
            format!("{:#x}", parsed.usdc_address),
            BASE_SEPOLIA_USDC.to_lowercase()
        );
        assert!(parsed.reputation_registry_address.is_some());
    }

    #[test]
    fn rejects_non_base_chain() {
        let config = CryptoConfig {
            enabled: true,
            chain: "UNSUPPORTED-TESTNET".to_string(),
            ..CryptoConfig::default()
        };

        let err = ExchangeConfig::from_crypto_config(&config)
            .expect_err("non-Base chain should be rejected");
        assert!(err.to_string().contains("Phase 2 supports only"));
    }
}
