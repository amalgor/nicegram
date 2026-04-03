use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use hydra_config::AgentConfig;
use hydra_exchange::{DealBoardClient, DealOfferView, ExchangeConfig};

use crate::models::qwen2_infer::Qwen2Infer;

/// Result of deal scoring and selection by the agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DealDecision {
    /// Agent found a deal and auto-approved it (amount <= auto_spend_limit).
    AutoApproved {
        offer: ScoredDeal,
    },
    /// Agent found a deal but requires user confirmation (amount > auto_spend_limit).
    NeedsConfirmation {
        offer: ScoredDeal,
        reason: String,
    },
    /// No suitable deals found.
    NoDeal {
        reason: String,
    },
}

/// A deal offer with a computed score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoredDeal {
    pub offer_id: u64,
    pub dealer: String,
    pub agent_id: u64,
    pub currency: String,
    pub rate: String,
    pub payment_methods: Vec<String>,
    pub score: f64,
    pub score_breakdown: ScoreBreakdown,
}

/// Breakdown of how the deal score was computed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreBreakdown {
    pub reputation_score: f64,
    pub rate_score: f64,
    pub payment_method_score: f64,
}

/// AI-powered deal agent for P2P fiat-to-USDC negotiation.
/// Scores deals based on reputation, rate, and payment method preferences.
/// Uses LLM re-ranking when model is loaded.
pub struct DealAgent {
    config: AgentConfig,
    deal_client: DealBoardClient,
    infer: Arc<Mutex<Option<Qwen2Infer>>>,
}

impl DealAgent {
    pub fn new(
        agent_config: AgentConfig,
        exchange_config: ExchangeConfig,
        infer: Arc<Mutex<Option<Qwen2Infer>>>,
    ) -> Self {
        Self {
            config: agent_config,
            deal_client: DealBoardClient::new(exchange_config),
            infer,
        }
    }

    /// Find the best deal for a given currency and amount.
    /// Scores all active offers, applies filters, and returns a decision.
    pub async fn find_best_deal(
        &self,
        currency: &str,
        usdc_amount: f64,
    ) -> Result<DealDecision> {
        let offers = self.deal_client.query_deals(currency).await?;

        if offers.is_empty() {
            return Ok(DealDecision::NoDeal {
                reason: format!("No active deals for currency {}", currency),
            });
        }

        info!("Found {} active deals for {}", offers.len(), currency);

        // Score and filter offers
        let mut scored: Vec<ScoredDeal> = offers
            .iter()
            .filter_map(|offer| self.score_offer(offer))
            .collect();

        if scored.is_empty() {
            return Ok(DealDecision::NoDeal {
                reason: format!(
                    "All {} deals filtered out by agent config (min_reputation={}, max_premium={})",
                    offers.len(),
                    self.config.min_dealer_reputation,
                    self.config.max_rate_premium
                ),
            });
        }

        // Sort by score descending
        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        // LLM re-ranking if model is loaded
        if let Some(reranked) = self.llm_rerank(&scored).await {
            scored = reranked;
        }

        let best = scored.into_iter().next().unwrap();

        debug!(
            "Best deal: offer_id={}, score={:.2}, dealer={}",
            best.offer_id, best.score, best.dealer
        );

        // Decide: auto-approve or request confirmation
        if usdc_amount <= self.config.auto_spend_limit {
            Ok(DealDecision::AutoApproved { offer: best })
        } else {
            Ok(DealDecision::NeedsConfirmation {
                reason: format!(
                    "Amount {:.2} USDC exceeds auto_spend_limit {:.2}",
                    usdc_amount, self.config.auto_spend_limit
                ),
                offer: best,
            })
        }
    }

    /// Execute an auto-buy: find best deal and accept it if within auto_spend_limit.
    pub async fn auto_buy_route(
        &self,
        currency: &str,
        usdc_amount: f64,
        mnemonic: &str,
    ) -> Result<DealDecision> {
        let decision = self.find_best_deal(currency, usdc_amount).await?;

        match &decision {
            DealDecision::AutoApproved { offer } => {
                let amount_str = format!("{:.6}", usdc_amount);
                let result = self.deal_client
                    .accept_deal(offer.offer_id, &amount_str, mnemonic)
                    .await?;

                info!(
                    "Auto-accepted deal: offer_id={}, escrow_id={:?}, tx={}",
                    offer.offer_id, result.escrow_id, result.tx_hash
                );
            }
            DealDecision::NeedsConfirmation { reason, .. } => {
                info!("Deal requires confirmation: {}", reason);
            }
            DealDecision::NoDeal { reason } => {
                warn!("No deal available: {}", reason);
            }
        }

        Ok(decision)
    }

    // ── Scoring Logic ──────────────────────────────────────────────────

    fn score_offer(&self, offer: &DealOfferView) -> Option<ScoredDeal> {
        // Filter: minimum reputation
        let rep_value = offer
            .reputation
            .as_ref()
            .and_then(|r| r.summary_value.parse::<i64>().ok())
            .unwrap_or(0);

        if rep_value < self.config.min_dealer_reputation {
            return None;
        }

        // Reputation score: normalized to 0-1 range, capped at 100
        let reputation_score = (rep_value.max(0) as f64 / 100.0).min(1.0);

        // Rate score: lower rate (fewer fiat units per USDC) is better.
        // Without a market oracle, we score rate inversely. Range 0-1.
        let rate_raw: f64 = offer.rate.parse().unwrap_or(0.0);
        let rate_score = if rate_raw > 0.0 {
            // Normalize: assume rates are in the 1e6-1e9 range (6 decimal encoding).
            // Lower rate = better deal for buyer.
            // Use inverse: 1 / (1 + rate/1e8) to get 0-1 range.
            1.0 / (1.0 + rate_raw / 1e8)
        } else {
            0.0
        };

        // Payment method score: bonus for preferred methods
        let payment_method_score = if self.config.preferred_payment_methods.is_empty() {
            0.5 // neutral
        } else {
            let matches = offer
                .payment_methods
                .iter()
                .filter(|m| {
                    self.config
                        .preferred_payment_methods
                        .iter()
                        .any(|p| p.eq_ignore_ascii_case(m))
                })
                .count();
            if matches > 0 {
                1.0
            } else {
                0.1
            }
        };

        // Weighted composite score
        let score = reputation_score * 0.4 + rate_score * 0.4 + payment_method_score * 0.2;

        Some(ScoredDeal {
            offer_id: offer.offer_id,
            dealer: offer.dealer.clone(),
            agent_id: offer.agent_id,
            currency: offer.currency.clone(),
            rate: offer.rate.clone(),
            payment_methods: offer.payment_methods.clone(),
            score,
            score_breakdown: ScoreBreakdown {
                reputation_score,
                rate_score,
                payment_method_score,
            },
        })
    }

    /// LLM re-ranking: if model is loaded, ask LLM to re-rank top N deals.
    /// Returns None if model not loaded or inference fails.
    async fn llm_rerank(&self, scored: &[ScoredDeal]) -> Option<Vec<ScoredDeal>> {
        let mut guard = self.infer.lock().await;
        let infer = guard.as_mut()?;

        if scored.len() <= 1 {
            return None;
        }

        // Only re-rank top 5
        let top_n = scored.iter().take(5).collect::<Vec<_>>();

        let deals_json = top_n
            .iter()
            .enumerate()
            .map(|(i, d)| {
                format!(
                    "  {}: offer_id={}, rate={}, reputation={:.2}, methods={:?}",
                    i + 1,
                    d.offer_id,
                    d.rate,
                    d.score_breakdown.reputation_score,
                    d.payment_methods
                )
            })
            .collect::<Vec<_>>()
            .join("\n");

        let prompt = format!(
            "<|im_start|>system\nYou are a deal evaluation agent. \
             Rank the following P2P fiat-to-USDC deals from best to worst. \
             Consider: lower rate is better for buyer, higher reputation is safer, \
             matching payment methods reduce friction. \
             Output ONLY a JSON array of offer_ids in ranked order.\n\
             Example: [3, 1, 2]<|im_end|>\n\
             <|im_start|>user\nDeals:\n{}<|im_end|>\n\
             <|im_start|>assistant\n",
            deals_json
        );

        match infer.generate(&prompt, 64) {
            Ok(response) => {
                debug!("LLM re-rank response: {}", response);
                parse_rerank_response(&response, scored)
            }
            Err(e) => {
                warn!("LLM re-ranking failed: {}", e);
                None
            }
        }
    }
}

/// Parse LLM re-rank response: extract JSON array of offer IDs.
fn parse_rerank_response(response: &str, scored: &[ScoredDeal]) -> Option<Vec<ScoredDeal>> {
    let json_str = if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            &response[start..=end]
        } else {
            return None;
        }
    } else {
        return None;
    };

    let ids: Vec<u64> = serde_json::from_str(json_str).ok()?;

    let mut reranked = Vec::new();
    for id in &ids {
        if let Some(deal) = scored.iter().find(|d| d.offer_id == *id) {
            reranked.push(deal.clone());
        }
    }

    // Append any deals that the LLM missed
    for deal in scored {
        if !ids.contains(&deal.offer_id) {
            reranked.push(deal.clone());
        }
    }

    if reranked.is_empty() {
        None
    } else {
        Some(reranked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hydra_exchange::{DealOfferView, ReputationSummary};

    fn test_config() -> AgentConfig {
        AgentConfig {
            auto_spend_limit: 5.0,
            max_rate_premium: 0.15,
            preferred_payment_methods: vec!["sbp".to_string()],
            min_dealer_reputation: 0,
        }
    }

    fn make_offer(
        offer_id: u64,
        rate: &str,
        rep_value: &str,
        methods: Vec<&str>,
    ) -> DealOfferView {
        DealOfferView {
            offer_id,
            dealer: format!("0xdealer{}", offer_id),
            agent_id: offer_id,
            currency: "RUB".to_string(),
            rate: rate.to_string(),
            min_amount: "10".to_string(),
            max_amount: "500".to_string(),
            payment_methods: methods.into_iter().map(|s| s.to_string()).collect(),
            active: true,
            reputation: Some(ReputationSummary {
                feedback_count: 5,
                summary_value: rep_value.to_string(),
                value_decimals: 0,
                formatted_value: rep_value.to_string(),
            }),
        }
    }

    #[test]
    fn score_offer_returns_none_below_min_reputation() {
        let config = AgentConfig {
            min_dealer_reputation: 50,
            ..test_config()
        };
        let exchange_config = ExchangeConfig {
            chain: "BASE-SEPOLIA".to_string(),
            chain_id: 84532,
            rpc_url: "https://sepolia.base.org/".parse().unwrap(),
            route_book_address: Default::default(),
            deal_board_address: None,
            identity_registry_address: Default::default(),
            reputation_registry_address: None,
            usdc_address: Default::default(),
        };
        let agent = DealAgent::new(config, exchange_config, Arc::new(Mutex::new(None)));
        let offer = make_offer(1, "90000000", "10", vec!["sbp"]);
        assert!(agent.score_offer(&offer).is_none());
    }

    #[test]
    fn score_offer_prefers_higher_reputation() {
        let exchange_config = ExchangeConfig {
            chain: "BASE-SEPOLIA".to_string(),
            chain_id: 84532,
            rpc_url: "https://sepolia.base.org/".parse().unwrap(),
            route_book_address: Default::default(),
            deal_board_address: None,
            identity_registry_address: Default::default(),
            reputation_registry_address: None,
            usdc_address: Default::default(),
        };
        let agent = DealAgent::new(test_config(), exchange_config, Arc::new(Mutex::new(None)));

        let high_rep = make_offer(1, "90000000", "80", vec!["sbp"]);
        let low_rep = make_offer(2, "90000000", "10", vec!["sbp"]);

        let s1 = agent.score_offer(&high_rep).unwrap();
        let s2 = agent.score_offer(&low_rep).unwrap();

        assert!(s1.score > s2.score, "higher reputation should score higher");
    }

    #[test]
    fn score_offer_prefers_matching_payment_method() {
        let exchange_config = ExchangeConfig {
            chain: "BASE-SEPOLIA".to_string(),
            chain_id: 84532,
            rpc_url: "https://sepolia.base.org/".parse().unwrap(),
            route_book_address: Default::default(),
            deal_board_address: None,
            identity_registry_address: Default::default(),
            reputation_registry_address: None,
            usdc_address: Default::default(),
        };
        let agent = DealAgent::new(test_config(), exchange_config, Arc::new(Mutex::new(None)));

        let matching = make_offer(1, "90000000", "50", vec!["sbp"]);
        let non_matching = make_offer(2, "90000000", "50", vec!["wire"]);

        let s1 = agent.score_offer(&matching).unwrap();
        let s2 = agent.score_offer(&non_matching).unwrap();

        assert!(s1.score > s2.score, "matching payment method should score higher");
    }

    #[test]
    fn parse_rerank_response_extracts_ids() {
        let scored = vec![
            ScoredDeal {
                offer_id: 1,
                dealer: "0x1".to_string(),
                agent_id: 1,
                currency: "RUB".to_string(),
                rate: "90".to_string(),
                payment_methods: vec![],
                score: 0.5,
                score_breakdown: ScoreBreakdown {
                    reputation_score: 0.5,
                    rate_score: 0.5,
                    payment_method_score: 0.5,
                },
            },
            ScoredDeal {
                offer_id: 2,
                dealer: "0x2".to_string(),
                agent_id: 2,
                currency: "RUB".to_string(),
                rate: "85".to_string(),
                payment_methods: vec![],
                score: 0.6,
                score_breakdown: ScoreBreakdown {
                    reputation_score: 0.6,
                    rate_score: 0.6,
                    payment_method_score: 0.5,
                },
            },
        ];

        let result = parse_rerank_response("Here is the ranking: [2, 1]", &scored);
        assert!(result.is_some());
        let reranked = result.unwrap();
        assert_eq!(reranked[0].offer_id, 2);
        assert_eq!(reranked[1].offer_id, 1);
    }

    #[test]
    fn parse_rerank_response_returns_none_for_invalid() {
        let scored = vec![];
        assert!(parse_rerank_response("no json here", &scored).is_none());
    }
}
