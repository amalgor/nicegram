use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletIdentity {
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneratedWallet {
    pub mnemonic: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WalletBalances {
    pub address: String,
    pub chain: String,
    pub eth_balance_wei: String,
    pub eth_balance: String,
    pub usdc_address: String,
    pub usdc_balance_raw: String,
    pub usdc_balance: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReputationSummary {
    pub feedback_count: u64,
    pub summary_value: String,
    pub value_decimals: u8,
    pub formatted_value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteOfferView {
    pub offer_id: u64,
    pub provider: String,
    pub agent_id: u64,
    pub endpoint_ciphertext: String,
    pub protocols: Vec<String>,
    pub region: String,
    pub price_per_gb_raw: String,
    pub price_per_gb: String,
    pub stake_amount_raw: String,
    pub stake_amount: String,
    pub bandwidth_mbps: u64,
    pub created_at: u64,
    pub deactivated_at: u64,
    pub active: bool,
    pub reputation: Option<ReputationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CreateOfferInput {
    pub agent_id: u64,
    pub endpoint_url: String,
    pub protocols: Vec<String>,
    pub region: String,
    pub price_per_gb_raw: String,
    pub stake_amount_raw: String,
    pub bandwidth_mbps: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentRegistrationResult {
    pub agent_id: u64,
    pub tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OfferMutationResult {
    pub offer_id: Option<u64>,
    pub tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxHashResult {
    pub tx_hash: String,
}

// ── P2P Deal Board models ──────────────────────────────────────────────

/// Escrow status matching the on-chain EscrowStatus enum
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DealEscrowStatus {
    Funded,
    Sent,
    Completed,
    Rejected,
    Expired,
}

impl DealEscrowStatus {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Self::Funded,
            1 => Self::Sent,
            2 => Self::Completed,
            3 => Self::Rejected,
            4 => Self::Expired,
            _ => Self::Funded,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DealOfferView {
    pub offer_id: u64,
    pub dealer: String,
    pub agent_id: u64,
    pub currency: String,
    pub rate: String,
    pub min_amount: String,
    pub max_amount: String,
    pub payment_methods: Vec<String>,
    pub active: bool,
    pub reputation: Option<ReputationSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DealEscrowView {
    pub escrow_id: u64,
    pub offer_id: u64,
    pub buyer: String,
    pub dealer: String,
    pub usdc_amount: String,
    pub fiat_amount: String,
    pub status: DealEscrowStatus,
    pub created_at: u64,
    pub expires_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptDealResult {
    pub escrow_id: Option<u64>,
    pub tx_hash: String,
}
