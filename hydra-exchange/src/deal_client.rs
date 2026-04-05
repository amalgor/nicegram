use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::sol_types::SolEvent;
use anyhow::{Context, Result, bail};

use crate::bindings::{HydraDealBoard, UsdcToken};
use crate::config::{ExchangeConfig, http_client};
use crate::models::{
    AcceptDealResult, CreateDealOfferInput, DealBoardAllowanceView, DealEscrowStatus,
    DealEscrowView, DealOfferView, OfferMutationResult, ReputationSummary, TxHashResult,
};
use crate::wallet::LocalWallet;

/// Client for interacting with the HydraDealBoard smart contract.
/// Provides methods to query deals, accept offers, and manage escrow lifecycle.
#[derive(Debug, Clone)]
pub struct DealBoardClient {
    config: ExchangeConfig,
}

impl DealBoardClient {
    pub fn new(config: ExchangeConfig) -> Self {
        Self { config }
    }

    fn deal_board_address(&self) -> Result<alloy::primitives::Address> {
        self.config
            .deal_board_address
            .ok_or_else(|| anyhow::anyhow!(
                "Missing [crypto].deal_board_address in hydra.toml. \
                 Deploy HydraDealBoard and set the address."
            ))
    }

    /// Query active deal offers for a given fiat currency (ISO 4217, e.g. "RUB").
    pub async fn query_deals(&self, currency: &str) -> Result<Vec<DealOfferView>> {
        let address = self.deal_board_address()?;
        let provider = ProviderBuilder::new()
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let total = board.totalOffers().call().await
            .context("Failed to get totalOffers")?
            .to::<u64>();

        let currency_upper = currency.trim().to_uppercase();
        let mut views = Vec::new();

        for i in 1..=total {
            let offer = board.getOffer(U256::from(i)).call().await
                .context("Failed to get offer")?;
            if !offer.active {
                continue;
            }
            if offer.currency.to_uppercase() != currency_upper {
                continue;
            }

            let reputation = self.fetch_reputation(offer.agentId.to::<u64>()).await;

            views.push(DealOfferView {
                offer_id: i,
                dealer: format!("{:#x}", offer.dealer),
                agent_id: offer.agentId.to::<u64>(),
                currency: offer.currency,
                rate: offer.rate.to_string(),
                min_amount: format_usdc(offer.minAmount),
                max_amount: format_usdc(offer.maxAmount),
                payment_methods: offer.paymentMethods,
                active: offer.active,
                reputation,
            });
        }

        Ok(views)
    }

    /// Get a single deal offer by ID.
    pub async fn get_offer(&self, offer_id: u64) -> Result<DealOfferView> {
        let address = self.deal_board_address()?;
        let provider = ProviderBuilder::new()
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let offer = board
            .getOffer(U256::from(offer_id))
            .call()
            .await
            .context("Failed to get deal offer")?;

        let reputation = self.fetch_reputation(offer.agentId.to::<u64>()).await;

        Ok(DealOfferView {
            offer_id,
            dealer: format!("{:#x}", offer.dealer),
            agent_id: offer.agentId.to::<u64>(),
            currency: offer.currency,
            rate: offer.rate.to_string(),
            min_amount: format_usdc(offer.minAmount),
            max_amount: format_usdc(offer.maxAmount),
            payment_methods: offer.paymentMethods,
            active: offer.active,
            reputation,
        })
    }

    pub async fn list_my_offers(
        &self,
        dealer: Address,
        currency: Option<&str>,
    ) -> Result<Vec<DealOfferView>> {
        let currency_filter = match currency {
            Some(value) if !value.trim().is_empty() => Some(normalize_currency(value)?),
            _ => None,
        };
        let mut offers = Vec::new();
        for offer_id in 1..=self.total_offers().await? {
            let offer = self.get_offer(offer_id).await?;
            let offer_dealer = offer
                .dealer
                .parse::<Address>()
                .with_context(|| format!("Invalid dealer address '{}'", offer.dealer))?;
            if offer_dealer != dealer {
                continue;
            }
            if let Some(currency_filter) = &currency_filter
                && offer.currency.to_uppercase() != *currency_filter
            {
                continue;
            }
            offers.push(offer);
        }
        Ok(offers)
    }

    pub async fn list_my_escrows(
        &self,
        address: Address,
        role: Option<&str>,
    ) -> Result<Vec<DealEscrowView>> {
        let normalized_role = normalize_escrow_role(role)?;
        let mut escrows = Vec::new();
        for escrow_id in 1..=self.total_escrows().await? {
            let escrow = self.check_status(escrow_id).await?;
            let buyer = escrow
                .buyer
                .parse::<Address>()
                .with_context(|| format!("Invalid buyer address '{}'", escrow.buyer))?;
            let dealer = escrow
                .dealer
                .parse::<Address>()
                .with_context(|| format!("Invalid dealer address '{}'", escrow.dealer))?;
            let matches = match normalized_role.as_deref() {
                Some("buyer") => buyer == address,
                Some("dealer") => dealer == address,
                _ => buyer == address || dealer == address,
            };
            if matches {
                escrows.push(escrow);
            }
        }
        Ok(escrows)
    }

    pub async fn create_deal_offer(
        &self,
        mnemonic: &str,
        input: CreateDealOfferInput,
    ) -> Result<OfferMutationResult> {
        let address = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let payment_methods = normalize_payment_methods(&input.payment_methods)?;
        let currency = normalize_currency(&input.currency)?;
        let rate = parse_usdc_amount(&input.rate_raw)?;
        let min_amount = parse_usdc_amount(&input.min_amount_raw)?;
        let max_amount = parse_usdc_amount(&input.max_amount_raw)?;
        if min_amount > max_amount {
            bail!("Minimum amount must be less than or equal to maximum amount.");
        }

        let pending = board
            .createDealOffer(
                U256::from(input.agent_id),
                currency,
                rate,
                min_amount,
                max_amount,
                payment_methods,
            )
            .send()
            .await
            .context("Failed to send createDealOffer transaction")?;

        let tx_hash = format!("{:#x}", pending.tx_hash());
        let receipt = pending
            .get_receipt()
            .await
            .context("Failed to get createDealOffer receipt")?;

        Ok(OfferMutationResult {
            offer_id: decode_deal_offer_created_id(&receipt),
            tx_hash,
        })
    }

    pub async fn deactivate_deal_offer(
        &self,
        offer_id: u64,
        mnemonic: &str,
    ) -> Result<OfferMutationResult> {
        let address = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let pending = board
            .deactivateDealOffer(U256::from(offer_id))
            .send()
            .await
            .context("Failed to send deactivateDealOffer transaction")?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        pending
            .get_receipt()
            .await
            .context("Failed to get deactivateDealOffer receipt")?;

        Ok(OfferMutationResult {
            offer_id: Some(offer_id),
            tx_hash,
        })
    }

    /// Accept a deal offer. Locks the dealer's USDC in escrow.
    /// The buyer calls this — the dealer must have pre-approved the DealBoard contract.
    pub async fn accept_deal(
        &self,
        offer_id: u64,
        usdc_amount: &str,
        mnemonic: &str,
    ) -> Result<AcceptDealResult> {
        let address = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let amount = parse_usdc_amount(usdc_amount)?;

        let pending = board
            .acceptDeal(U256::from(offer_id), amount)
            .send()
            .await
            .context("Failed to send acceptDeal transaction")?;

        let tx_hash = format!("{:#x}", pending.tx_hash());
        let receipt = pending.get_receipt().await
            .context("Failed to get acceptDeal receipt")?;

        let escrow_id = decode_escrow_created_id(&receipt);

        Ok(AcceptDealResult {
            escrow_id,
            tx_hash,
        })
    }

    /// Mark fiat as sent. Only the buyer can call this.
    pub async fn mark_sent(&self, escrow_id: u64, mnemonic: &str) -> Result<TxHashResult> {
        let address = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let pending = board
            .markFiatSent(U256::from(escrow_id))
            .send()
            .await
            .context("Failed to send markFiatSent transaction")?;

        let tx_hash = format!("{:#x}", pending.tx_hash());
        pending.get_receipt().await
            .context("Failed to get markFiatSent receipt")?;

        Ok(TxHashResult { tx_hash })
    }

    /// Check escrow status.
    pub async fn check_status(&self, escrow_id: u64) -> Result<DealEscrowView> {
        let address = self.deal_board_address()?;
        let provider = ProviderBuilder::new()
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let escrow = board
            .getEscrow(U256::from(escrow_id))
            .call()
            .await
            .context("Failed to get escrow status")?;

        Ok(DealEscrowView {
            escrow_id,
            offer_id: escrow.offerId.to::<u64>(),
            buyer: format!("{:#x}", escrow.buyer),
            dealer: format!("{:#x}", escrow.dealer),
            usdc_amount: format_usdc(escrow.usdcAmount),
            fiat_amount: escrow.fiatAmount.to_string(),
            status: DealEscrowStatus::from_u8(escrow.status),
            created_at: escrow.createdAt,
            expires_at: escrow.expiresAt,
        })
    }

    pub async fn confirm_receipt(&self, escrow_id: u64, mnemonic: &str) -> Result<TxHashResult> {
        let address = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let pending = board
            .confirmReceipt(U256::from(escrow_id))
            .send()
            .await
            .context("Failed to send confirmReceipt transaction")?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        pending
            .get_receipt()
            .await
            .context("Failed to get confirmReceipt receipt")?;

        Ok(TxHashResult { tx_hash })
    }

    pub async fn reject_deal(&self, escrow_id: u64, mnemonic: &str) -> Result<TxHashResult> {
        let address = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let pending = board
            .rejectDeal(U256::from(escrow_id))
            .send()
            .await
            .context("Failed to send rejectDeal transaction")?;
        let tx_hash = format!("{:#x}", pending.tx_hash());
        pending
            .get_receipt()
            .await
            .context("Failed to get rejectDeal receipt")?;

        Ok(TxHashResult { tx_hash })
    }

    /// Claim expired escrow. Anyone can call, but USDC goes to buyer.
    pub async fn claim_expired(&self, escrow_id: u64, mnemonic: &str) -> Result<TxHashResult> {
        let address = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);

        let pending = board
            .claimExpired(U256::from(escrow_id))
            .send()
            .await
            .context("Failed to send claimExpired transaction")?;

        let tx_hash = format!("{:#x}", pending.tx_hash());
        pending.get_receipt().await
            .context("Failed to get claimExpired receipt")?;

        Ok(TxHashResult { tx_hash })
    }

    /// Approve USDC spending for the DealBoard (dealer utility).
    /// Dealers must call this before their offers can be accepted.
    pub async fn approve_usdc(&self, amount: &str, mnemonic: &str) -> Result<TxHashResult> {
        let deal_board = self.deal_board_address()?;
        let signer = LocalWallet::signer_from_phrase(mnemonic)?;
        let provider = ProviderBuilder::new()
            .wallet(signer)
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let usdc = UsdcToken::new(self.config.usdc_address, &provider);

        let parsed = parse_usdc_amount(amount)?;

        let pending = usdc
            .approve(deal_board, parsed)
            .send()
            .await
            .context("Failed to send USDC approve transaction")?;

        let tx_hash = format!("{:#x}", pending.tx_hash());
        pending.get_receipt().await
            .context("Failed to get USDC approve receipt")?;

        Ok(TxHashResult { tx_hash })
    }

    pub async fn allowance(&self, owner: Address) -> Result<DealBoardAllowanceView> {
        let spender = self.deal_board_address()?;
        let provider = ProviderBuilder::new()
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let usdc = UsdcToken::new(self.config.usdc_address, &provider);
        let allowance = usdc
            .allowance(owner, spender)
            .call()
            .await
            .context("Failed to load DealBoard USDC allowance")?;
        Ok(DealBoardAllowanceView {
            owner: format!("{:#x}", owner),
            spender: format!("{:#x}", spender),
            allowance_raw: allowance.to_string(),
            allowance: format_usdc(allowance),
        })
    }

    // ── Private helpers ────────────────────────────────────────────────

    async fn fetch_reputation(&self, agent_id: u64) -> Option<ReputationSummary> {
        let registry_address = self.config.reputation_registry_address?;
        let provider = ProviderBuilder::new()
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let registry = crate::bindings::ReputationRegistry::new(registry_address, &provider);

        let clients = registry
            .getClients(U256::from(agent_id))
            .call()
            .await
            .ok()?;

        let summary = registry
            .getSummary(
                U256::from(agent_id),
                clients.clone(),
                "deal".to_string(),
                String::new(),
            )
            .call()
            .await
            .ok()?;

        Some(ReputationSummary {
            feedback_count: summary.count,
            summary_value: summary.summaryValue.to_string(),
            value_decimals: summary.summaryValueDecimals,
            formatted_value: summary.summaryValue.to_string(),
        })
    }

    async fn total_offers(&self) -> Result<u64> {
        let address = self.deal_board_address()?;
        let provider = ProviderBuilder::new()
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);
        let total = board
            .totalOffers()
            .call()
            .await
            .context("Failed to get HydraDealBoard totalOffers")?;
        Ok(total.to())
    }

    async fn total_escrows(&self) -> Result<u64> {
        let address = self.deal_board_address()?;
        let provider = ProviderBuilder::new()
            .connect_reqwest(http_client(), self.config.rpc_url.clone());
        let board = HydraDealBoard::new(address, &provider);
        let total = board
            .totalEscrows()
            .call()
            .await
            .context("Failed to get HydraDealBoard totalEscrows")?;
        Ok(total.to())
    }

}

// ── Utility functions ──────────────────────────────────────────────────

fn format_usdc(raw: U256) -> String {
    let raw_u128 = raw.to::<u128>();
    let whole = raw_u128 / 1_000_000;
    let frac = raw_u128 % 1_000_000;
    if frac == 0 {
        whole.to_string()
    } else {
        let frac_str = format!("{:06}", frac).trim_end_matches('0').to_string();
        format!("{}.{}", whole, frac_str)
    }
}

fn parse_usdc_amount(amount: &str) -> Result<U256> {
    let trimmed = amount.trim();
    if trimmed.is_empty() {
        bail!("USDC amount must not be empty.");
    }

    if let Some(dot_pos) = trimmed.find('.') {
        let whole: u128 = trimmed[..dot_pos]
            .parse()
            .context("Invalid USDC whole part")?;
        let frac_str = &trimmed[dot_pos + 1..];
        if frac_str.len() > 6 {
            bail!("USDC amount has more than 6 decimal places.");
        }
        let padded = format!("{:0<6}", frac_str);
        let frac: u128 = padded.parse().context("Invalid USDC fractional part")?;
        Ok(U256::from(whole * 1_000_000 + frac))
    } else {
        let whole: u128 = trimmed.parse().context("Invalid USDC amount")?;
        Ok(U256::from(whole * 1_000_000))
    }
}

fn decode_escrow_created_id(
    receipt: &alloy::rpc::types::TransactionReceipt,
) -> Option<u64> {
    for log in receipt.inner.logs() {
        if let Ok(event) = HydraDealBoard::EscrowCreated::decode_log(log.as_ref()) {
            return Some(event.data.escrowId.to::<u64>());
        }
    }
    None
}

fn decode_deal_offer_created_id(
    receipt: &alloy::rpc::types::TransactionReceipt,
) -> Option<u64> {
    for log in receipt.inner.logs() {
        if let Ok(event) = HydraDealBoard::DealOfferCreated::decode_log(log.as_ref()) {
            return Some(event.data.offerId.to::<u64>());
        }
    }
    None
}

fn normalize_currency(currency: &str) -> Result<String> {
    let normalized = currency.trim().to_uppercase();
    if normalized.len() != 3 || !normalized.chars().all(|item| item.is_ascii_uppercase()) {
        bail!("Currency must be a 3-letter ISO 4217 uppercase code.");
    }
    Ok(normalized)
}

fn normalize_payment_methods(methods: &[String]) -> Result<Vec<String>> {
    let normalized: Vec<String> = methods
        .iter()
        .map(|item| item.trim().to_ascii_lowercase())
        .filter(|item| !item.is_empty())
        .collect();
    if normalized.is_empty() {
        bail!("At least one payment method is required.");
    }
    Ok(normalized)
}

fn normalize_escrow_role(role: Option<&str>) -> Result<Option<String>> {
    let Some(role) = role else {
        return Ok(None);
    };
    let normalized = role.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return Ok(None);
    }
    match normalized.as_str() {
        "buyer" | "dealer" => Ok(Some(normalized)),
        _ => bail!("Unsupported escrow role '{role}'."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_usdc_renders_whole_and_fractional() {
        assert_eq!(format_usdc(U256::from(1_000_000_u64)), "1");
        assert_eq!(format_usdc(U256::from(0_u64)), "0");
        assert_eq!(format_usdc(U256::from(1_500_000_u64)), "1.5");
        assert_eq!(format_usdc(U256::from(100_u64)), "0.0001");
        assert_eq!(format_usdc(U256::from(10_000_000_u64)), "10");
    }

    #[test]
    fn parse_usdc_amount_handles_various_formats() {
        assert_eq!(parse_usdc_amount("100").unwrap(), U256::from(100_000_000_u64));
        assert_eq!(parse_usdc_amount("1.5").unwrap(), U256::from(1_500_000_u64));
        assert_eq!(parse_usdc_amount("0.000001").unwrap(), U256::from(1_u64));
        assert!(parse_usdc_amount("").is_err());
        assert!(parse_usdc_amount("1.1234567").is_err());
    }

    #[test]
    fn normalizers_validate_currency_methods_and_role() {
        assert_eq!(normalize_currency("rub").unwrap(), "RUB");
        assert!(normalize_currency("ru").is_err());
        assert_eq!(
            normalize_payment_methods(&[" Bank_Transfer ".to_string(), "SBP".to_string()])
                .unwrap(),
            vec!["bank_transfer".to_string(), "sbp".to_string()]
        );
        assert!(normalize_payment_methods(&[]).is_err());
        assert_eq!(normalize_escrow_role(Some("buyer")).unwrap(), Some("buyer".to_string()));
        assert_eq!(normalize_escrow_role(Some("")).unwrap(), None);
        assert!(normalize_escrow_role(Some("unknown")).is_err());
    }
}
