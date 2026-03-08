use anyhow::Result;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::path::Path;
use tracing::info;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TrustBalance {
    pub peer_id: String,
    pub debt: i64,
    pub trust_score: u32,
    pub last_settlement: u64,
}

pub struct EconLedger {
    db: Db,
    settlement_threshold: i64,
}

impl EconLedger {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = sled::open(path)?;
        info!("Economic ledger initialized");
        Ok(Self {
            db,
            settlement_threshold: 10_000_000, // Example threshold (e.g., 10MB worth of debt)
        })
    }

    pub fn update_debt(&self, peer_id: &str, amount: i64) -> Result<bool> {
        let key = format!("peer:{}", peer_id);
        let mut trigger_settlement = false;

        self.db.update_and_fetch(key, |old| {
            let mut balance = old
                .map(|b| {
                    serde_json::from_slice::<TrustBalance>(b).unwrap_or(TrustBalance {
                        peer_id: peer_id.to_string(),
                        debt: 0,
                        trust_score: 100,
                        last_settlement: 0,
                    })
                })
                .unwrap_or(TrustBalance {
                    peer_id: peer_id.to_string(),
                    debt: 0,
                    trust_score: 100,
                    last_settlement: 0,
                });

            balance.debt += amount;
            if balance.debt.abs() >= self.settlement_threshold {
                trigger_settlement = true;
            }

            Some(serde_json::to_vec(&balance).unwrap())
        })?;

        Ok(trigger_settlement)
    }

    pub fn get_balance(&self, peer_id: &str) -> Result<TrustBalance> {
        let key = format!("peer:{}", peer_id);
        let b = self.db.get(key)?;
        match b {
            Some(data) => Ok(serde_json::from_slice(&data)?),
            None => Ok(TrustBalance {
                peer_id: peer_id.to_string(),
                debt: 0,
                trust_score: 100,
                last_settlement: 0,
            }),
        }
    }

    pub async fn settle(&self, peer_id: &str) -> Result<()> {
        info!(
            "Triggering crypto-clearing settlement for peer: {}",
            peer_id
        );

        // Simulation of clearing:
        // In a real system, this would involve creating a zero-knowledge proof of the debt
        // and submitting it to a clearing house or an L2 contract.
        // We simulate this by generating a "settlement receipt".

        let key = format!("peer:{}", peer_id);
        self.db.update_and_fetch(key, |old| {
            if let Some(data) = old {
                if let Ok(mut balance) = serde_json::from_slice::<TrustBalance>(data) {
                    let amount_settled = balance.debt;
                    balance.debt = 0;
                    balance.last_settlement = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();

                    info!("Settled {} units for peer {}", amount_settled, peer_id);
                    return Some(serde_json::to_vec(&balance).unwrap());
                }
            }
            old.map(|v| v.to_vec())
        })?;

        Ok(())
    }

    /// Probabilistic audit: Verifies if data was likely delivered.
    /// Updates trust score based on verification outcome.
    pub fn verify_proof_of_transfer(&self, peer_id: &str, success: bool) -> Result<()> {
        let key = format!("peer:{}", peer_id);
        self.db.update_and_fetch(key, |old| {
            let mut balance = old
                .map(|b| {
                    serde_json::from_slice::<TrustBalance>(b).unwrap_or(TrustBalance {
                        peer_id: peer_id.to_string(),
                        debt: 0,
                        trust_score: 100,
                        last_settlement: 0,
                    })
                })
                .unwrap_or(TrustBalance {
                    peer_id: peer_id.to_string(),
                    debt: 0,
                    trust_score: 100,
                    last_settlement: 0,
                });

            if success {
                // Increase trust score slowly up to 100
                balance.trust_score = (balance.trust_score + 1).min(100);
            } else {
                // Decrease trust score more aggressively on failure
                balance.trust_score = balance.trust_score.saturating_sub(10);
            }

            Some(serde_json::to_vec(&balance).unwrap())
        })?;
        Ok(())
    }
}
