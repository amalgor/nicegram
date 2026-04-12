use alloy::primitives::{Address, FixedBytes, U256};
use alloy::providers::{Provider, ProviderBuilder, WalletProvider};
use alloy::rpc::types::TransactionReceipt;
use anyhow::{Result, bail};
use std::str::FromStr;

use crate::bindings::{HydraRouteBook, IdentityRegistry, ReputationRegistry, UsdcToken};
use crate::config::{BASE_SEPOLIA_CHAIN, ExchangeConfig, http_client};
use crate::models::{
    AgentRegistrationResult, CreateOfferInput, OfferMutationResult, ReputationSummary,
    RouteBookLifecycleView, RouteOfferView, TxHashResult, WalletBalances, WalletSignature,
};
use crate::wallet::LocalWallet;

#[derive(Debug, Clone)]
pub struct RouteExchangeClient {
    config: ExchangeConfig,
}

#[derive(Debug, Clone)]
pub struct AgentRegistrar {
    config: ExchangeConfig,
}

#[derive(Debug, Clone)]
pub struct ReputationClient {
    config: ExchangeConfig,
}

impl RouteExchangeClient {
    pub fn new(config: ExchangeConfig) -> Self {
        Self { config }
    }

    pub async fn list_active_offers(&self, max_offers: usize) -> Result<Vec<RouteOfferView>> {
        let total = self.total_offers().await?;
        let reputation_client = self
            .config
            .reputation_registry_address
            .map(|_| ReputationClient::new(self.config.clone()));

        let mut offers = Vec::new();
        for offer_id in 1..=total {
            if offers.len() >= max_offers {
                break;
            }

            let view = self.get_offer(offer_id).await?;
            if !view.active {
                continue;
            }

            let enriched = if let Some(client) = &reputation_client {
                let reputation = client.summary_for_agent(view.agent_id).await?;
                RouteOfferView { reputation, ..view }
            } else {
                view
            };

            offers.push(enriched);
        }

        Ok(offers)
    }

    pub async fn query_offers(&self, region: &str, protocol: &str) -> Result<Vec<RouteOfferView>> {
        let region = normalize_optional_region(region)?;
        let protocol = normalize_protocol(protocol)?;
        let mut offers = Vec::new();
        for view in self.list_active_offers(usize::MAX).await? {
            if region.as_ref().is_some_and(|region| view.region != *region) {
                continue;
            }
            if !view
                .protocols
                .iter()
                .any(|item| item.eq_ignore_ascii_case(&protocol))
            {
                continue;
            }

            offers.push(view);
        }

        Ok(offers)
    }

    pub async fn get_offer(&self, offer_id: u64) -> Result<RouteOfferView> {
        let provider =
            ProviderBuilder::new().connect_reqwest(http_client(), self.config.rpc_url.clone());
        let route_book = HydraRouteBook::new(self.config.route_book_address, provider);
        let offer = route_book.getOffer(U256::from(offer_id)).call().await?;

        Ok(RouteOfferView {
            offer_id,
            provider: format!("{:#x}", offer.provider),
            agent_id: offer.agentId.to(),
            endpoint_ciphertext: offer.endpointCiphertext,
            protocols: offer.protocols,
            region: offer.region,
            price_per_gb_raw: offer.pricePerGB.to_string(),
            price_per_gb: format_unsigned_fixed(&offer.pricePerGB, 6),
            stake_amount_raw: offer.stakeAmount.to_string(),
            stake_amount: format_unsigned_fixed(&offer.stakeAmount, 6),
            bandwidth_mbps: offer.bandwidthMbps.to(),
            created_at: offer.createdAt,
            deactivated_at: offer.deactivatedAt,
            active: offer.active,
            reputation: None,
        })
    }

    pub async fn list_my_offers(
        &self,
        provider_address: Option<Address>,
        agent_id: Option<u64>,
    ) -> Result<Vec<RouteOfferView>> {
        let mut offers = Vec::new();
        for offer_id in 1..=self.total_offers().await? {
            let offer = self.get_offer(offer_id).await?;
            if let Some(provider_address) = provider_address {
                let offer_provider = offer.provider.parse::<Address>().map_err(|error| {
                    anyhow::anyhow!("Invalid provider address '{}': {error}", offer.provider)
                })?;
                if offer_provider != provider_address {
                    continue;
                }
            }
            if let Some(agent_id) = agent_id
                && offer.agent_id != agent_id
            {
                continue;
            }
            offers.push(offer);
        }
        Ok(offers)
    }

    pub async fn lifecycle(&self) -> Result<RouteBookLifecycleView> {
        let provider =
            ProviderBuilder::new().connect_reqwest(http_client(), self.config.rpc_url.clone());
        let route_book = HydraRouteBook::new(self.config.route_book_address, provider);
        let withdrawal_delay = route_book.withdrawalDelay().call().await?;
        Ok(RouteBookLifecycleView {
            withdrawal_delay_secs: withdrawal_delay.to(),
        })
    }

    pub async fn wallet_balances(&self, address: Address) -> Result<WalletBalances> {
        let provider =
            ProviderBuilder::new().connect_reqwest(http_client(), self.config.rpc_url.clone());
        let eth_balance: U256 = provider.get_balance(address).await?;
        let usdc = UsdcToken::new(self.config.usdc_address, provider);
        let usdc_balance = usdc.balanceOf(address).call().await?;

        Ok(WalletBalances {
            address: format!("{:#x}", address),
            chain: BASE_SEPOLIA_CHAIN.to_string(),
            eth_balance_wei: eth_balance.to_string(),
            eth_balance: format_unsigned_fixed(&eth_balance, 18),
            usdc_address: format!("{:#x}", self.config.usdc_address),
            usdc_balance_raw: usdc_balance.to_string(),
            usdc_balance: format_unsigned_fixed(&usdc_balance, 6),
        })
    }

    async fn total_offers(&self) -> Result<u64> {
        let provider =
            ProviderBuilder::new().connect_reqwest(http_client(), self.config.rpc_url.clone());
        let route_book = HydraRouteBook::new(self.config.route_book_address, provider);
        let total = route_book.totalOffers().call().await?;
        Ok(total.to())
    }

    pub async fn create_offer(
        &self,
        mnemonic: &str,
        input: CreateOfferInput,
    ) -> Result<OfferMutationResult> {
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let signer_address = provider.wallet().default_signer().address();
        let route_book = HydraRouteBook::new(self.config.route_book_address, provider.clone());
        let usdc = UsdcToken::new(self.config.usdc_address, provider.clone());

        let stake_amount = parse_u256(&input.stake_amount_raw, "stake amount")?;
        let price_per_gb = parse_u256(&input.price_per_gb_raw, "price per GB")?;
        let region = normalize_region(&input.region)?;
        let protocols = normalize_protocols(&input.protocols)?;
        let endpoint_url = input.endpoint_url.trim();
        if endpoint_url.is_empty() {
            bail!("Endpoint URL must not be empty.");
        }

        let allowance = usdc
            .allowance(signer_address, self.config.route_book_address)
            .call()
            .await?;
        if allowance < stake_amount {
            usdc.approve(self.config.route_book_address, stake_amount)
                .send()
                .await?
                .get_receipt()
                .await?;
        }

        let pending = route_book
            .createOffer(
                U256::from(input.agent_id),
                endpoint_url.to_string(),
                protocols,
                region,
                price_per_gb,
                stake_amount,
                U256::from(input.bandwidth_mbps),
            )
            .send()
            .await?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        let receipt = pending.get_receipt().await?;
        let offer_id = decode_created_offer_id(&receipt);

        Ok(OfferMutationResult { offer_id, tx_hash })
    }

    pub async fn deactivate_offer(
        &self,
        mnemonic: &str,
        offer_id: u64,
    ) -> Result<OfferMutationResult> {
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let route_book = HydraRouteBook::new(self.config.route_book_address, provider);

        let pending = route_book
            .deactivateOffer(U256::from(offer_id))
            .send()
            .await?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        let _receipt = pending.get_receipt().await?;

        Ok(OfferMutationResult {
            offer_id: Some(offer_id),
            tx_hash,
        })
    }

    pub async fn withdraw_stake(
        &self,
        mnemonic: &str,
        offer_id: u64,
    ) -> Result<OfferMutationResult> {
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let route_book = HydraRouteBook::new(self.config.route_book_address, provider);

        let pending = route_book
            .withdrawStake(U256::from(offer_id))
            .send()
            .await?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        let _receipt = pending.get_receipt().await?;

        Ok(OfferMutationResult {
            offer_id: Some(offer_id),
            tx_hash,
        })
    }

    pub fn sign_message(&self, mnemonic: &str, message: &str) -> Result<WalletSignature> {
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        Ok(WalletSignature {
            address: format!("{:#x}", signer.address()),
            message: message.to_string(),
            signature: LocalWallet::sign_message(mnemonic, message.as_bytes())?,
        })
    }
}

impl AgentRegistrar {
    pub fn new(config: ExchangeConfig) -> Self {
        Self { config }
    }

    pub async fn register(&self, mnemonic: &str) -> Result<AgentRegistrationResult> {
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let identity = IdentityRegistry::new(self.config.identity_registry_address, provider);

        let preview = identity.register_0().call().await?;
        let pending = identity.register_0().send().await?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        let receipt = pending.get_receipt().await?;
        let agent_id = decode_registered_agent_id(&receipt).unwrap_or_else(|| preview.to());

        Ok(AgentRegistrationResult { agent_id, tx_hash })
    }
}

impl ReputationClient {
    pub fn new(config: ExchangeConfig) -> Self {
        Self { config }
    }

    pub async fn give_feedback(
        &self,
        mnemonic: &str,
        agent_id: u64,
        positive: bool,
        tag1: &str,
    ) -> Result<TxHashResult> {
        let registry_address = self
            .config
            .reputation_registry_address
            .ok_or_else(|| anyhow::anyhow!("Missing [crypto].reputation_registry_address."))?;
        let normalized_tag = normalize_tag(tag1)?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let reputation = ReputationRegistry::new(registry_address, provider);

        let value: i128 = if positive { 1 } else { -1 };
        let pending = reputation
            .giveFeedback(
                U256::from(agent_id),
                value,
                0,
                normalized_tag,
                String::new(),
                String::new(),
                String::new(),
                FixedBytes::<32>::ZERO,
            )
            .send()
            .await?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        let _receipt = pending.get_receipt().await?;

        Ok(TxHashResult { tx_hash })
    }

    pub async fn summary_for_agent(&self, agent_id: u64) -> Result<Option<ReputationSummary>> {
        let Some(registry_address) = self.config.reputation_registry_address else {
            return Ok(None);
        };
        let provider =
            ProviderBuilder::new().connect_reqwest(http_client(), self.config.rpc_url.clone());
        let reputation = ReputationRegistry::new(registry_address, provider);
        let clients = reputation.getClients(U256::from(agent_id)).call().await?;

        if clients.is_empty() {
            return Ok(Some(ReputationSummary {
                feedback_count: 0,
                summary_value: "0".to_string(),
                value_decimals: 0,
                formatted_value: "0".to_string(),
            }));
        }

        let summary = reputation
            .getSummary(U256::from(agent_id), clients, String::new(), String::new())
            .call()
            .await?;

        Ok(Some(ReputationSummary {
            feedback_count: summary.count,
            summary_value: summary.summaryValue.to_string(),
            value_decimals: summary.summaryValueDecimals,
            formatted_value: format_signed_fixed(
                summary.summaryValue,
                summary.summaryValueDecimals,
            ),
        }))
    }
}

fn decode_registered_agent_id(receipt: &TransactionReceipt) -> Option<u64> {
    receipt
        .decoded_log::<IdentityRegistry::Registered>()
        .map(|event| event.data.agentId.to())
}

fn decode_created_offer_id(receipt: &TransactionReceipt) -> Option<u64> {
    receipt
        .decoded_log::<HydraRouteBook::OfferCreated>()
        .map(|event| event.data.offerId.to())
}

fn normalize_region(region: &str) -> Result<String> {
    let normalized = region.trim().to_uppercase();
    if normalized.len() != 2 || !normalized.chars().all(|c| c.is_ascii_uppercase()) {
        bail!("Region must be an uppercase ISO-3166 alpha-2 code.");
    }
    Ok(normalized)
}

fn normalize_optional_region(region: &str) -> Result<Option<String>> {
    let normalized = region.trim();
    if normalized.is_empty() {
        return Ok(None);
    }

    normalize_region(normalized).map(Some)
}

fn normalize_protocol(protocol: &str) -> Result<String> {
    let normalized = protocol.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        bail!("Protocol filter must not be empty.");
    }
    Ok(normalized)
}

fn normalize_protocols(protocols: &[String]) -> Result<Vec<String>> {
    let normalized: Vec<String> = protocols
        .iter()
        .map(|item| normalize_protocol(item))
        .collect::<Result<Vec<_>>>()?;
    if normalized.is_empty() {
        bail!("Offer must contain at least one protocol.");
    }
    Ok(normalized)
}

fn parse_u256(raw: &str, field: &str) -> Result<U256> {
    let value = raw.trim();
    if value.is_empty() {
        bail!("{field} must not be empty.");
    }
    U256::from_str(value).map_err(|error| anyhow::anyhow!("Invalid {field} '{value}': {error}"))
}

fn normalize_tag(tag1: &str) -> Result<String> {
    let normalized = tag1.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "availability" | "latency" | "throughput" | "trust" => Ok(normalized),
        _ => bail!("Unsupported feedback tag '{}'.", tag1),
    }
}

fn format_unsigned_fixed(value: &U256, decimals: usize) -> String {
    format_fixed_string(&value.to_string(), decimals, false)
}

fn format_signed_fixed(value: i128, decimals: u8) -> String {
    let abs = value.unsigned_abs().to_string();
    format_fixed_string(&abs, decimals as usize, value.is_negative())
}

fn format_fixed_string(raw: &str, decimals: usize, negative: bool) -> String {
    if decimals == 0 {
        return if negative {
            format!("-{}", raw)
        } else {
            raw.to_string()
        };
    }

    let mut digits = raw.to_string();
    if digits.len() <= decimals {
        digits = format!("{}{}", "0".repeat(decimals + 1 - digits.len()), digits);
    }

    let split = digits.len() - decimals;
    let integer = &digits[..split];
    let fraction = digits[split..].trim_end_matches('0');
    let number = if fraction.is_empty() {
        integer.to_string()
    } else {
        format!("{}.{}", integer, fraction)
    };

    if negative {
        format!("-{}", number)
    } else {
        number
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatters_render_expected_values() {
        assert_eq!(format_fixed_string("1234500", 6, false), "1.2345");
        assert_eq!(format_fixed_string("1000000", 6, false), "1");
        assert_eq!(format_fixed_string("42", 0, false), "42");
        assert_eq!(format_signed_fixed(-32, 1), "-3.2");
    }

    #[test]
    fn tag_validation_is_fixed_enum() {
        assert!(normalize_tag("availability").is_ok());
        assert!(normalize_tag("latency").is_ok());
        assert!(normalize_tag("speed").is_err());
    }

    #[test]
    fn empty_region_filter_is_allowed_for_queries() {
        assert_eq!(normalize_optional_region("").unwrap(), None);
        assert_eq!(
            normalize_optional_region(" us ").unwrap(),
            Some("US".to_string())
        );
    }
}
