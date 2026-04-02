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
pub struct AgentRegistrationResult {
    pub agent_id: u64,
    pub tx_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TxHashResult {
    pub tx_hash: String,
}

