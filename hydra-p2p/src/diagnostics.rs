use anyhow::Result;
use libp2p::PeerId;
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum DiagnosticRequest {
    PingTarget { target: String },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum DiagnosticResponse {
    PingResult {
        reachable: bool,
        latency_ms: Option<u64>,
        error: Option<String>,
    },
}

pub enum DiagnosticsCommand {
    SendRequest {
        peer_id: PeerId,
        request: DiagnosticRequest,
        resp: oneshot::Sender<Result<DiagnosticResponse>>,
    },
}
