mod bindings;
mod client;
mod config;
mod models;
mod wallet;

pub use client::{AgentRegistrar, ReputationClient, RouteExchangeClient};
pub use config::{
    BASE_SEPOLIA_CHAIN, BASE_SEPOLIA_CHAIN_ID, BASE_SEPOLIA_IDENTITY_REGISTRY,
    BASE_SEPOLIA_REPUTATION_REGISTRY, BASE_SEPOLIA_RPC_URL, BASE_SEPOLIA_USDC, ExchangeConfig,
};
pub use models::{
    AgentRegistrationResult, GeneratedWallet, ReputationSummary, RouteOfferView, TxHashResult,
    WalletBalances, WalletIdentity,
};
pub use wallet::{LocalWallet, normalize_mnemonic};
