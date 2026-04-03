use anyhow::Result;
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use hydra_config::AiConfig;

pub mod deal_agent;
pub mod models;
pub use deal_agent::DealAgent;
pub use models::qwen2_infer::Qwen2Infer;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct PeerInfo {
    pub peer_id: String,
    pub trust_score: u32,
    pub current_debt: i64,
    pub rtt_ms: Option<u64>,
    pub bandwidth_bps: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct RouteRequest {
    pub target: String,
    pub protocol: String,
    pub peers: Vec<PeerInfo>,
    pub diagnostic_context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingInstruction {
    pub path: Vec<String>,
    pub transport: String,
    pub max_price: f64,
}

pub struct AiNegotiator {
    infer: Arc<Mutex<Option<Qwen2Infer>>>,
    cache: Cache<RouteRequest, RoutingInstruction>,
    max_generation_tokens: usize,
}

impl AiNegotiator {
    pub fn new(config: &AiConfig) -> Self {
        let cache = Cache::builder()
            .max_capacity(config.cache_max_items)
            .time_to_live(Duration::from_secs(config.cache_ttl_seconds))
            .build();

        Self {
            infer: Arc::new(Mutex::new(None)),
            cache,
            max_generation_tokens: config.max_generation_tokens,
        }
    }

    /// Access the underlying inference engine for direct generation (used by hydra-content summarizer)
    pub fn infer(&self) -> &Arc<Mutex<Option<Qwen2Infer>>> {
        &self.infer
    }

    pub async fn load_model(&self, model_path: PathBuf) -> Result<()> {
        info!("Loading AI model from {:?}", model_path);
        let infer = Qwen2Infer::load(&model_path, None)?;
        *self.infer.lock().await = Some(infer);
        info!("AI model loaded successfully");
        Ok(())
    }

    pub async fn decide_route(&self, request: RouteRequest) -> Result<RoutingInstruction> {
        // 1. Check cache
        if let Some(instruction) = self.cache.get(&request).await {
            info!("Cache hit for route to {}", request.target);
            return Ok(instruction);
        }

        info!("AI Negotiator deciding route for: {}", request.target);

        // 2. Direct fast-path for localhost or no peers
        if request.peers.is_empty() || request.target.contains("localhost") {
            let decision = RoutingInstruction {
                path: vec![],
                transport: "raw".to_string(),
                max_price: 0.0,
            };
            self.cache.insert(request, decision.clone()).await;
            return Ok(decision);
        }

        // 3. Construct prompt
        let diag_section = match &request.diagnostic_context {
            Some(ctx) => format!("\nDiagnostic context (previous attempt failed): {}", ctx),
            None => String::new(),
        };

        let prompt = format!(
            "<|im_start|>system\nYou are a network routing agent. Output ONLY a valid JSON object. No explanation.\n\
            Given the request and available peers (trust_score 0-100, current_debt in bytes, rtt_ms for latency, bandwidth_bps for throughput), \
            select the best routing path as an ordered list of peer_ids.\n\
            If no peer is reliable (trust < 50), output an empty path for direct connection.\n\
            Format: {{\"path\": [\"<peer_id>\", ...], \"transport\": \"vless\" | \"raw\", \"max_price\": <float>}}\n\
            Rules:\n\
            - path: array of peer_id strings forming the route. Empty array [] means direct connection.\n\
            - transport: \"vless\" for tunneled, \"raw\" for direct.\n\
            - max_price: max acceptable price in credits per MB (use 0.0 for direct).\n\
            - Prefer peers with high trust_score, low rtt_ms, high bandwidth_bps, low current_debt.<|im_end|>\n\
            <|im_start|>user\nRequest: Target={}, Protocol={}\nPeers: {}{}<|im_end|>\n<|im_start|>assistant\n",
            request.target,
            request.protocol,
            serde_json::to_string(&request.peers)?,
            diag_section
        );

        debug!("Prompt for AI: {}", prompt);

        // 4. Run inference if model is loaded, otherwise fallback to simple logic
        let mut infer_guard = self.infer.lock().await;
        if let Some(infer) = infer_guard.as_mut() {
            match infer.generate(&prompt, self.max_generation_tokens) {
                Ok(response) => {
                    debug!("Raw AI Response: {}", response);

                    // Simple JSON extraction regex or find bounds
                    let json_str = if let Some(start) = response.find('{') {
                        if let Some(end) = response.rfind('}') {
                            &response[start..=end]
                        } else {
                            &response[start..]
                        }
                    } else {
                        &response
                    };

                    match serde_json::from_str::<RoutingInstruction>(json_str) {
                        Ok(decision) => {
                            self.cache.insert(request, decision.clone()).await;
                            return Ok(decision);
                        }
                        Err(e) => {
                            error!(
                                "Failed to parse AI response as JSON: {}. Response: {}",
                                e, response
                            );
                            // Fallback on error
                        }
                    }
                }
                Err(e) => {
                    error!("Model inference failed: {}", e);
                }
            }
        } else {
            debug!("Model not loaded, using fallback heuristic");
        }

        // 5. Fallback logic
        let mut sorted_peers = request.peers.clone();
        sorted_peers.sort_by(|a, b| {
            b.trust_score
                .cmp(&a.trust_score)
                .then_with(|| a.current_debt.cmp(&b.current_debt))
        });

        let decision = if sorted_peers[0].trust_score < 50 {
            RoutingInstruction {
                path: vec![],
                transport: "raw".to_string(),
                max_price: 0.0,
            }
        } else {
            RoutingInstruction {
                path: vec![sorted_peers[0].peer_id.clone()],
                transport: "vless".to_string(),
                max_price: 0.001,
            }
        };

        self.cache.insert(request, decision.clone()).await;
        Ok(decision)
    }
}

/// Parse a JSON string into a RoutingInstruction, extracting JSON from surrounding text.
pub fn parse_routing_json(response: &str) -> Result<RoutingInstruction> {
    let json_str = if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            &response[start..=end]
        } else {
            &response[start..]
        }
    } else {
        response
    };
    serde_json::from_str::<RoutingInstruction>(json_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse routing JSON: {}", e))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AiConfig {
        AiConfig {
            model_path: std::path::PathBuf::from("nonexistent.gguf"),
            max_generation_tokens: 128,
            cache_ttl_seconds: 60,
            cache_max_items: 100,
        }
    }

    fn make_peer(id: &str, trust: u32, debt: i64) -> PeerInfo {
        PeerInfo {
            peer_id: id.to_string(),
            trust_score: trust,
            current_debt: debt,
            rtt_ms: Some(50),
            bandwidth_bps: Some(1_000_000),
        }
    }

    #[test]
    fn test_parse_routing_json_clean() {
        let json = r#"{"path": ["peer1"], "transport": "vless", "max_price": 0.001}"#;
        let inst = parse_routing_json(json).unwrap();
        assert_eq!(inst.path, vec!["peer1"]);
        assert_eq!(inst.transport, "vless");
        assert!((inst.max_price - 0.001).abs() < 1e-9);
    }

    #[test]
    fn test_parse_routing_json_with_surrounding_text() {
        let response = "Here is the routing: {\"path\": [], \"transport\": \"raw\", \"max_price\": 0.0} done.";
        let inst = parse_routing_json(response).unwrap();
        assert!(inst.path.is_empty());
        assert_eq!(inst.transport, "raw");
    }

    #[test]
    fn test_parse_routing_json_empty_path() {
        let json = r#"{"path": [], "transport": "raw", "max_price": 0.0}"#;
        let inst = parse_routing_json(json).unwrap();
        assert!(inst.path.is_empty());
    }

    #[test]
    fn test_parse_routing_json_invalid() {
        assert!(parse_routing_json("not json at all").is_err());
        assert!(parse_routing_json("{broken").is_err());
        assert!(parse_routing_json(r#"{"path": "wrong_type"}"#).is_err());
    }

    #[test]
    fn test_parse_routing_json_multi_hop() {
        let json = r#"{"path": ["peer1", "peer2", "peer3"], "transport": "vless", "max_price": 0.01}"#;
        let inst = parse_routing_json(json).unwrap();
        assert_eq!(inst.path.len(), 3);
    }

    #[tokio::test]
    async fn test_fallback_no_model_no_peers() {
        let ai = AiNegotiator::new(&test_config());
        let req = RouteRequest {
            target: "example.com:443".to_string(),
            protocol: "tcp".to_string(),
            peers: vec![],
            diagnostic_context: None,
        };
        let result = ai.decide_route(req).await.unwrap();
        assert!(result.path.is_empty());
        assert_eq!(result.transport, "raw");
    }

    #[tokio::test]
    async fn test_fallback_no_model_with_trusted_peer() {
        let ai = AiNegotiator::new(&test_config());
        let req = RouteRequest {
            target: "example.com:443".to_string(),
            protocol: "tcp".to_string(),
            peers: vec![make_peer("peer-A", 80, 1000)],
            diagnostic_context: None,
        };
        let result = ai.decide_route(req).await.unwrap();
        assert_eq!(result.path, vec!["peer-A"]);
        assert_eq!(result.transport, "vless");
    }

    #[tokio::test]
    async fn test_fallback_no_model_low_trust_peer() {
        let ai = AiNegotiator::new(&test_config());
        let req = RouteRequest {
            target: "example.com:443".to_string(),
            protocol: "tcp".to_string(),
            peers: vec![make_peer("peer-B", 30, 0)],
            diagnostic_context: None,
        };
        let result = ai.decide_route(req).await.unwrap();
        assert!(result.path.is_empty(), "Low trust peer should result in direct route");
    }

    #[tokio::test]
    async fn test_fallback_sorts_by_trust_then_debt() {
        let ai = AiNegotiator::new(&test_config());
        let req = RouteRequest {
            target: "example.com:443".to_string(),
            protocol: "tcp".to_string(),
            peers: vec![
                make_peer("low-trust", 60, 100),
                make_peer("high-trust", 90, 500),
                make_peer("high-trust-low-debt", 90, 50),
            ],
            diagnostic_context: None,
        };
        let result = ai.decide_route(req).await.unwrap();
        assert_eq!(result.path, vec!["high-trust-low-debt"]);
    }

    #[tokio::test]
    async fn test_localhost_fast_path() {
        let ai = AiNegotiator::new(&test_config());
        let req = RouteRequest {
            target: "localhost:8080".to_string(),
            protocol: "tcp".to_string(),
            peers: vec![make_peer("peer-X", 100, 0)],
            diagnostic_context: None,
        };
        let result = ai.decide_route(req).await.unwrap();
        assert!(result.path.is_empty(), "localhost should always be direct");
    }

    #[tokio::test]
    async fn test_cache_hit() {
        let ai = AiNegotiator::new(&test_config());
        let req = RouteRequest {
            target: "cached.example.com:443".to_string(),
            protocol: "tcp".to_string(),
            peers: vec![],
            diagnostic_context: None,
        };

        let r1 = ai.decide_route(req.clone()).await.unwrap();
        let r2 = ai.decide_route(req).await.unwrap();
        assert_eq!(r1.path, r2.path);
        assert_eq!(r1.transport, r2.transport);
    }
}
