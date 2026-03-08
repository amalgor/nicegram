use libp2p::PeerId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Default)]
pub struct PeerMetrics {
    pub rtt_ms: Option<u64>,
    pub bandwidth_bps: Option<u64>,
    pub last_seen: u64,
}

pub struct TelemetryStore {
    metrics: Arc<RwLock<HashMap<PeerId, PeerMetrics>>>,
}

impl TelemetryStore {
    pub fn new() -> Self {
        Self {
            metrics: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn update_rtt(&self, peer_id: PeerId, rtt: Duration) {
        let mut store = self.metrics.write().await;
        let entry = store.entry(peer_id).or_default();
        entry.rtt_ms = Some(rtt.as_millis() as u64);
        entry.last_seen = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
    }

    pub async fn update_bandwidth(&self, peer_id: PeerId, bytes: u64, duration: Duration) {
        let bps = if duration.as_secs_f64() > 0.0 {
            (bytes as f64 / duration.as_secs_f64()) as u64
        } else {
            return;
        };

        let mut store = self.metrics.write().await;
        let entry = store.entry(peer_id).or_default();
        // Moving average for bandwidth (simple exponential smoothing)
        if let Some(current) = entry.bandwidth_bps {
            entry.bandwidth_bps = Some((current * 3 + bps) / 4);
        } else {
            entry.bandwidth_bps = Some(bps);
        }
    }

    pub async fn get_metrics(&self, peer_id: &PeerId) -> Option<PeerMetrics> {
        self.metrics.read().await.get(peer_id).cloned()
    }
}
