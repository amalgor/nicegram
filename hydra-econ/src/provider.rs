use anyhow::Result;
use serde::{Deserialize, Serialize};
use sled::Db;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderMetrics {
    pub agent_id: u64,
    pub session_count: u64,
    pub successful_sessions: u64,
    pub bytes_relayed: u64,
    pub average_latency_ms: f64,
    pub average_throughput_mbps: f64,
    pub uptime_ratio: f64,
    pub recent_failures: u32,
    pub local_routing_score: f64,
    pub pending_reputation_delta: f64,
    pub last_onchain_sync_time: u64,
    pub last_session_time: u64,
    pub estimated_earnings_micro_usdc: i64,
    pub settled_earnings_micro_usdc: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PendingReputationSync {
    pub agent_id: u64,
    pub positive: bool,
    pub tag1: String,
    pub pending_delta: f64,
}

pub struct ProviderMetricsLedger {
    db: Db,
}

impl ProviderMetricsLedger {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            db: sled::open(path)?,
        })
    }

    pub fn get(&self, agent_id: u64) -> Result<ProviderMetrics> {
        self.load(agent_id)
    }

    pub fn record_success(
        &self,
        agent_id: u64,
        bytes_relayed: u64,
        duration: Duration,
        latency_ms: u64,
        price_per_gb_micro_usdc: u64,
    ) -> Result<ProviderMetrics> {
        let mut metrics = self.load(agent_id)?;
        metrics.session_count = metrics.session_count.saturating_add(1);
        metrics.successful_sessions = metrics.successful_sessions.saturating_add(1);
        metrics.bytes_relayed = metrics.bytes_relayed.saturating_add(bytes_relayed);
        metrics.last_session_time = now_epoch_secs();

        let sample_count = metrics.successful_sessions as f64;
        metrics.average_latency_ms =
            rolling_average(metrics.average_latency_ms, sample_count, latency_ms as f64);

        let throughput_mbps = if duration.is_zero() {
            0.0
        } else {
            (bytes_relayed as f64 * 8.0) / duration.as_secs_f64() / 1_000_000.0
        };
        metrics.average_throughput_mbps =
            rolling_average(metrics.average_throughput_mbps, sample_count, throughput_mbps);
        metrics.recent_failures = metrics.recent_failures.saturating_sub(1);
        metrics.uptime_ratio = if metrics.session_count == 0 {
            1.0
        } else {
            metrics.successful_sessions as f64 / metrics.session_count as f64
        };
        metrics.pending_reputation_delta += positive_delta_for_success(latency_ms, throughput_mbps);
        metrics.estimated_earnings_micro_usdc = metrics
            .estimated_earnings_micro_usdc
            .saturating_add(earnings_for_usage(bytes_relayed, price_per_gb_micro_usdc));
        metrics.local_routing_score = compute_local_score(&metrics);
        self.save(&metrics)?;
        Ok(metrics)
    }

    pub fn record_failure(&self, agent_id: u64) -> Result<ProviderMetrics> {
        let mut metrics = self.load(agent_id)?;
        metrics.session_count = metrics.session_count.saturating_add(1);
        metrics.recent_failures = metrics.recent_failures.saturating_add(1);
        metrics.last_session_time = now_epoch_secs();
        metrics.uptime_ratio = if metrics.session_count == 0 {
            0.0
        } else {
            metrics.successful_sessions as f64 / metrics.session_count as f64
        };
        metrics.pending_reputation_delta -= 0.35;
        metrics.local_routing_score = compute_local_score(&metrics);
        self.save(&metrics)?;
        Ok(metrics)
    }

    pub fn mark_onchain_feedback_synced(&self, agent_id: u64) -> Result<ProviderMetrics> {
        let mut metrics = self.load(agent_id)?;
        metrics.pending_reputation_delta = 0.0;
        metrics.last_onchain_sync_time = now_epoch_secs();
        self.save(&metrics)?;
        Ok(metrics)
    }

    pub fn set_settled_earnings(&self, agent_id: u64, amount_micro_usdc: i64) -> Result<ProviderMetrics> {
        let mut metrics = self.load(agent_id)?;
        metrics.settled_earnings_micro_usdc = amount_micro_usdc.max(0);
        self.save(&metrics)?;
        Ok(metrics)
    }

    pub fn pending_syncs(
        &self,
        threshold: f64,
        max_age_secs: u64,
    ) -> Result<Vec<PendingReputationSync>> {
        let now = now_epoch_secs();
        let mut pending = Vec::new();
        for entry in self.db.scan_prefix("provider:") {
            let (_, value) = entry?;
            let metrics: ProviderMetrics = serde_json::from_slice(&value)?;
            let overdue = now.saturating_sub(metrics.last_onchain_sync_time) >= max_age_secs;
            if metrics.pending_reputation_delta.abs() < threshold && !overdue {
                continue;
            }
            if metrics.pending_reputation_delta.abs() < f64::EPSILON {
                continue;
            }
            pending.push(PendingReputationSync {
                agent_id: metrics.agent_id,
                positive: metrics.pending_reputation_delta >= 0.0,
                tag1: feedback_tag(&metrics),
                pending_delta: metrics.pending_reputation_delta,
            });
        }
        Ok(pending)
    }

    fn load(&self, agent_id: u64) -> Result<ProviderMetrics> {
        let key = provider_key(agent_id);
        let Some(value) = self.db.get(key)? else {
            return Ok(ProviderMetrics {
                agent_id,
                session_count: 0,
                successful_sessions: 0,
                bytes_relayed: 0,
                average_latency_ms: 0.0,
                average_throughput_mbps: 0.0,
                uptime_ratio: 1.0,
                recent_failures: 0,
                local_routing_score: 50.0,
                pending_reputation_delta: 0.0,
                last_onchain_sync_time: 0,
                last_session_time: 0,
                estimated_earnings_micro_usdc: 0,
                settled_earnings_micro_usdc: 0,
            });
        };
        Ok(serde_json::from_slice(&value)?)
    }

    fn save(&self, metrics: &ProviderMetrics) -> Result<()> {
        self.db
            .insert(provider_key(metrics.agent_id), serde_json::to_vec(metrics)?)?;
        self.db.flush()?;
        Ok(())
    }
}

fn provider_key(agent_id: u64) -> String {
    format!("provider:{agent_id}")
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn rolling_average(current: f64, sample_count: f64, latest: f64) -> f64 {
    if sample_count <= 1.0 {
        latest
    } else {
        ((current * (sample_count - 1.0)) + latest) / sample_count
    }
}

fn compute_local_score(metrics: &ProviderMetrics) -> f64 {
    let throughput_component = (metrics.average_throughput_mbps * 0.8).min(25.0);
    let uptime_component = (metrics.uptime_ratio * 30.0).min(30.0);
    let latency_penalty = (metrics.average_latency_ms / 400.0).min(20.0);
    let failure_penalty = (metrics.recent_failures as f64 * 4.0).min(20.0);
    (45.0 + throughput_component + uptime_component - latency_penalty - failure_penalty)
        .clamp(0.0, 100.0)
}

fn positive_delta_for_success(latency_ms: u64, throughput_mbps: f64) -> f64 {
    let latency_bonus = if latency_ms <= 120 {
        0.12
    } else if latency_ms <= 250 {
        0.08
    } else {
        0.04
    };
    let throughput_bonus = if throughput_mbps >= 20.0 {
        0.10
    } else if throughput_mbps >= 5.0 {
        0.06
    } else {
        0.03
    };
    latency_bonus + throughput_bonus
}

fn earnings_for_usage(bytes_relayed: u64, price_per_gb_micro_usdc: u64) -> i64 {
    if price_per_gb_micro_usdc == 0 {
        return 0;
    }
    let earned = (bytes_relayed as u128).saturating_mul(price_per_gb_micro_usdc as u128)
        / 1_000_000_000u128;
    earned.min(i64::MAX as u128) as i64
}

fn feedback_tag(metrics: &ProviderMetrics) -> String {
    if metrics.recent_failures > 0 || metrics.uptime_ratio < 0.8 {
        "availability".to_string()
    } else if metrics.average_latency_ms > 280.0 {
        "latency".to_string()
    } else if metrics.average_throughput_mbps < 5.0 {
        "throughput".to_string()
    } else {
        "trust".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ledger() -> ProviderMetricsLedger {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("hydra-provider-metrics-{unique}"));
        ProviderMetricsLedger::new(path).unwrap()
    }

    #[test]
    fn success_updates_metrics_and_earnings() {
        let ledger = ledger();
        let metrics = ledger
            .record_success(7, 25_000_000, Duration::from_secs(5), 95, 1_000_000)
            .unwrap();
        assert_eq!(metrics.agent_id, 7);
        assert_eq!(metrics.session_count, 1);
        assert_eq!(metrics.successful_sessions, 1);
        assert!(metrics.average_throughput_mbps > 0.0);
        assert!(metrics.pending_reputation_delta > 0.0);
        assert!(metrics.estimated_earnings_micro_usdc > 0);
    }

    #[test]
    fn failure_reduces_score_and_marks_pending_feedback() {
        let ledger = ledger();
        ledger.record_failure(42).unwrap();
        let metrics = ledger.get(42).unwrap();
        assert_eq!(metrics.recent_failures, 1);
        assert!(metrics.local_routing_score < 75.0);

        let pending = ledger.pending_syncs(0.2, 0).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].agent_id, 42);
        assert!(!pending[0].positive);
        assert_eq!(pending[0].tag1, "availability");
    }

    #[test]
    fn mark_synced_resets_pending_delta() {
        let ledger = ledger();
        ledger.record_failure(99).unwrap();
        let metrics = ledger.mark_onchain_feedback_synced(99).unwrap();
        assert_eq!(metrics.pending_reputation_delta, 0.0);
        assert!(metrics.last_onchain_sync_time > 0);
    }
}
