use crate::classifier::{ClassificationRequest, VerdictCache, VerdictKey};
use crate::connections::{
    ClassificationSource, ConnectionClassification, ConnectionRegistry, TrafficCategory,
};
use hydra_ai::models::qwen2_infer::Qwen2Infer;
use serde::Deserialize;
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

// -- Configuration constants --------------------------------------------------

const BATCH_SIZE: usize = 5;
const MIN_INTERVAL: Duration = Duration::from_secs(5);
const MAX_QUEUE_SIZE: usize = 100;
const MAX_GENERATION_TOKENS: usize = 300;
const MODEL_CHECK_INTERVAL: Duration = Duration::from_secs(30);

// System prompt kept concise to stay within 512-token total budget for 0.6B models.
const SYSTEM_PROMPT: &str = "\
You are a network traffic classifier. For each connection output a JSON array.\
Each element: {\"id\":N,\"category\":\"<cat>\",\"confidence\":0.0-1.0,\"explanation\":\"<one sentence Russian>\"}.\
Categories: legitimate, advertising, analytics, telemetry, social_tracking, malware, unknown.\
Patterns: doubleclick/adsense=advertising, google-analytics/firebase=analytics, \
facebook pixel/graph=social_tracking, crash/telemetry endpoints=telemetry.";

// -- LLM response item --------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LlmClassificationItem {
    id: usize,
    category: String,
    #[serde(default = "default_confidence")]
    confidence: f64,
    explanation: Option<String>,
}

fn default_confidence() -> f64 {
    0.5
}

// -- LlmClassifier ------------------------------------------------------------

pub struct LlmClassifier {
    infer: Arc<Mutex<Option<Qwen2Infer>>>,
    pending_rx: mpsc::Receiver<ClassificationRequest>,
    buffer: VecDeque<ClassificationRequest>,
}

impl LlmClassifier {
    pub fn new(
        infer: Arc<Mutex<Option<Qwen2Infer>>>,
        pending_rx: mpsc::Receiver<ClassificationRequest>,
    ) -> Self {
        Self {
            infer,
            pending_rx,
            buffer: VecDeque::new(),
        }
    }

    pub async fn run(
        mut self,
        registry: Arc<ConnectionRegistry>,
        verdict_cache: Arc<VerdictCache>,
    ) {
        info!("LLM classifier task started");

        loop {
            // Drain channel into internal buffer
            while let Ok(req) = self.pending_rx.try_recv() {
                self.buffer.push_back(req);
            }

            // Overflow: drop oldest items beyond MAX_QUEUE_SIZE, mark them Unknown
            while self.buffer.len() > MAX_QUEUE_SIZE {
                if let Some(req) = self.buffer.pop_front() {
                    let classification = ConnectionClassification {
                        category: TrafficCategory::Unknown,
                        confidence: 0.0,
                        source: ClassificationSource::LlmAnalysis,
                        explanation: Some("LLM queue overflow".to_string()),
                    };
                    registry.update_classification(req.conn_id, classification.clone());
                    verdict_cache.put(
                        VerdictKey {
                            host_or_domain: req.host.to_lowercase(),
                            app_uid: req.app_uid,
                        },
                        classification,
                    );
                    info!(
                        conn_id = req.conn_id,
                        host = req.host.as_str(),
                        "llm_classifier: queue overflow, marked Unknown"
                    );
                }
            }

            // If buffer is empty, block-wait for the next item
            if self.buffer.is_empty() {
                match self.pending_rx.recv().await {
                    Some(req) => self.buffer.push_back(req),
                    None => {
                        info!("LLM classifier: channel closed, exiting");
                        return;
                    }
                }
            }

            // Check if model is loaded
            {
                let guard = self.infer.lock().await;
                if guard.is_none() {
                    debug!("LLM classifier: model not loaded, draining buffer and waiting");
                    self.buffer.clear();
                    drop(guard);
                    tokio::time::sleep(MODEL_CHECK_INTERVAL).await;
                    continue;
                }
            }

            // Collect batch from front of buffer (up to BATCH_SIZE)
            let batch = self.collect_batch();
            if batch.is_empty() {
                continue;
            }

            // Run inference
            let prompt = build_classification_prompt(&batch);

            let mut guard = self.infer.lock().await;
            if guard.is_none() {
                warn!("LLM classifier: model disappeared mid-iteration");
                apply_fallback(
                    &batch,
                    "Model unloaded during classification",
                    &registry,
                    &verdict_cache,
                );
                drop(guard);
                tokio::time::sleep(MODEL_CHECK_INTERVAL).await;
                continue;
            }

            info!(
                batch_size = batch.len(),
                "llm_classifier: running inference"
            );

            // Clone Arc for spawn_blocking
            let infer_arc = self.infer.clone();
            let prompt_clone = prompt.clone();

            // Run LLM inference in blocking thread to avoid SIGABRT in llama.cpp
            let inference_result = tokio::task::spawn_blocking(move || {
                let mut guard = infer_arc.blocking_lock();
                if let Some(infer) = guard.as_mut() {
                    infer.generate(&prompt_clone, MAX_GENERATION_TOKENS)
                } else {
                    Err(anyhow::anyhow!("Model unloaded"))
                }
            })
            .await;

            // Release the async lock we held earlier
            drop(guard);

            match inference_result {
                Ok(Ok(response)) => {
                    let items = parse_classification_response(&response);
                    let parse_success = !items.is_empty();

                    info!(
                        input = %prompt,
                        output = %response,
                        parse_success = parse_success,
                        "llm_classification"
                    );

                    apply_results(&batch, &items, &registry, &verdict_cache);
                }
                Ok(Err(e)) => {
                    error!("LLM classification inference failed: {}", e);
                    info!(
                        input = %prompt,
                        output = "",
                        parse_success = false,
                        "llm_classification"
                    );
                    apply_fallback(&batch, "LLM inference failed", &registry, &verdict_cache);
                }
                Err(e) => {
                    error!("LLM spawn_blocking task panicked: {}", e);
                    apply_fallback(&batch, "LLM task panicked", &registry, &verdict_cache);
                }
            }

            // Rate limiting: minimum 5 seconds between LLM calls
            tokio::time::sleep(MIN_INTERVAL).await;
        }
    }

    fn collect_batch(&mut self) -> Vec<ClassificationRequest> {
        let count = BATCH_SIZE.min(self.buffer.len());
        self.buffer.drain(..count).collect()
    }
}

// -- Prompt construction ------------------------------------------------------

fn build_classification_prompt(batch: &[ClassificationRequest]) -> String {
    let mut user_lines = String::from("Classify these connections:\n");
    for (i, req) in batch.iter().enumerate() {
        user_lines.push_str(&format!(
            "{}. host={} port={}",
            i + 1,
            req.host,
            req.port,
        ));
        if let Some(ref app) = req.app_label {
            user_lines.push_str(&format!(" app={}", app));
        } else if let Some(ref pkg) = req.package_name {
            user_lines.push_str(&format!(" app={}", pkg));
        }
        if let Some(ref rdns) = req.reverse_dns {
            user_lines.push_str(&format!(" reverse_dns={}", rdns));
        }
        if let Some(ref org) = req.whois_org {
            user_lines.push_str(&format!(" whois_org={}", org));
        }
        if let Some(asn) = req.whois_asn {
            user_lines.push_str(&format!(" whois_asn={}", asn));
        }
        user_lines.push('\n');
    }
    user_lines.push_str("Output JSON array:");

    format!(
        "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
        SYSTEM_PROMPT, user_lines
    )
}

// -- Response parsing ---------------------------------------------------------

fn parse_classification_response(response: &str) -> Vec<LlmClassificationItem> {
    // Strategy 1: parse entire response as JSON array
    if let Some(items) = try_parse_json_array(response) {
        return items;
    }

    // Strategy 2: extract JSON from markdown code blocks (```json ... ```)
    if let Some(items) = try_extract_code_block(response) {
        return items;
    }

    // Strategy 3: find [ ... ] substring
    if let Some(start) = response.find('[') {
        if let Some(end) = response.rfind(']') {
            if let Some(items) = try_parse_json_array(&response[start..=end]) {
                return items;
            }
        }
    }

    // Strategy 4: try line-by-line JSON objects
    let mut items = Vec::new();
    for line in response.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('{') {
            let json_str = trimmed.trim_end_matches(',');
            if let Ok(item) = serde_json::from_str::<LlmClassificationItem>(json_str) {
                items.push(item);
            }
        }
    }
    if !items.is_empty() {
        return items;
    }

    warn!(
        "LLM classifier: failed to parse response: {}",
        &response[..response.len().min(200)]
    );
    Vec::new()
}

fn try_parse_json_array(text: &str) -> Option<Vec<LlmClassificationItem>> {
    serde_json::from_str::<Vec<LlmClassificationItem>>(text.trim()).ok()
}

fn try_extract_code_block(text: &str) -> Option<Vec<LlmClassificationItem>> {
    let markers = ["```json", "```"];
    for marker in markers {
        if let Some(start_idx) = text.find(marker) {
            let content_start = start_idx + marker.len();
            if let Some(end_idx) = text[content_start..].find("```") {
                let block = text[content_start..content_start + end_idx].trim();
                if let Some(items) = try_parse_json_array(block) {
                    return Some(items);
                }
            }
        }
    }
    None
}

// -- Result application -------------------------------------------------------

fn parse_category(s: &str) -> TrafficCategory {
    match s.trim().to_lowercase().as_str() {
        "legitimate" => TrafficCategory::Legitimate,
        "advertising" => TrafficCategory::Advertising,
        "analytics" => TrafficCategory::Analytics,
        "telemetry" => TrafficCategory::Telemetry,
        "social_tracking" => TrafficCategory::SocialTracking,
        "malware" => TrafficCategory::Malware,
        _ => TrafficCategory::Unknown,
    }
}

fn apply_results(
    batch: &[ClassificationRequest],
    items: &[LlmClassificationItem],
    registry: &ConnectionRegistry,
    verdict_cache: &VerdictCache,
) {
    for (i, req) in batch.iter().enumerate() {
        let llm_item = items.iter().find(|item| item.id == i + 1);

        let classification = if let Some(item) = llm_item {
            ConnectionClassification {
                category: parse_category(&item.category),
                confidence: (item.confidence as f32).clamp(0.0, 1.0),
                source: ClassificationSource::LlmAnalysis,
                explanation: item.explanation.clone(),
            }
        } else {
            ConnectionClassification {
                category: TrafficCategory::Unknown,
                confidence: 0.0,
                source: ClassificationSource::LlmAnalysis,
                explanation: Some("LLM output parsing failed for this item".to_string()),
            }
        };

        registry.update_classification(req.conn_id, classification.clone());
        verdict_cache.put(
            VerdictKey {
                host_or_domain: req.host.to_lowercase(),
                app_uid: req.app_uid,
            },
            classification.clone(),
        );

        info!(
            conn_id = req.conn_id,
            host = req.host.as_str(),
            category = classification.category.as_str(),
            confidence = classification.confidence,
            source = "llm_analysis",
            "llm_classification_result"
        );
    }
}

fn apply_fallback(
    batch: &[ClassificationRequest],
    reason: &str,
    registry: &ConnectionRegistry,
    verdict_cache: &VerdictCache,
) {
    for req in batch {
        let classification = ConnectionClassification {
            category: TrafficCategory::Unknown,
            confidence: 0.0,
            source: ClassificationSource::LlmAnalysis,
            explanation: Some(reason.to_string()),
        };
        registry.update_classification(req.conn_id, classification.clone());
        verdict_cache.put(
            VerdictKey {
                host_or_domain: req.host.to_lowercase(),
                app_uid: req.app_uid,
            },
            classification,
        );
        info!(
            conn_id = req.conn_id,
            host = req.host.as_str(),
            category = "unknown",
            confidence = 0.0,
            source = "llm_analysis",
            reason = reason,
            "llm_classification_fallback"
        );
    }
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_json_array_clean() {
        let input = r#"[{"id":1,"category":"advertising","confidence":0.95,"explanation":"Рекламный трекер"},{"id":2,"category":"legitimate","confidence":0.88,"explanation":"API основного сервиса"}]"#;
        let items = parse_classification_response(input);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 1);
        assert_eq!(items[0].category, "advertising");
        assert!((items[0].confidence - 0.95).abs() < 0.01);
        assert_eq!(items[1].id, 2);
        assert_eq!(items[1].category, "legitimate");
    }

    #[test]
    fn test_parse_json_with_surrounding_text() {
        let input = "Here are the results:\n[{\"id\":1,\"category\":\"analytics\",\"confidence\":0.9,\"explanation\":\"Google Analytics\"}]\nDone.";
        let items = parse_classification_response(input);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].category, "analytics");
    }

    #[test]
    fn test_parse_json_from_code_block() {
        let input = "```json\n[{\"id\":1,\"category\":\"telemetry\",\"confidence\":0.7,\"explanation\":\"Телеметрия\"}]\n```";
        let items = parse_classification_response(input);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].category, "telemetry");
    }

    #[test]
    fn test_parse_line_by_line_json() {
        let input = "{\"id\":1,\"category\":\"advertising\",\"confidence\":0.9,\"explanation\":\"Ad\"}\n{\"id\":2,\"category\":\"legitimate\",\"confidence\":0.8,\"explanation\":\"OK\"}";
        let items = parse_classification_response(input);
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn test_parse_invalid_returns_empty() {
        let input = "I cannot classify these connections.";
        let items = parse_classification_response(input);
        assert!(items.is_empty());
    }

    #[test]
    fn test_parse_category_mapping() {
        assert_eq!(parse_category("legitimate"), TrafficCategory::Legitimate);
        assert_eq!(parse_category("advertising"), TrafficCategory::Advertising);
        assert_eq!(parse_category("analytics"), TrafficCategory::Analytics);
        assert_eq!(parse_category("telemetry"), TrafficCategory::Telemetry);
        assert_eq!(
            parse_category("social_tracking"),
            TrafficCategory::SocialTracking
        );
        assert_eq!(parse_category("malware"), TrafficCategory::Malware);
        assert_eq!(parse_category("unknown"), TrafficCategory::Unknown);
        assert_eq!(parse_category("garbage"), TrafficCategory::Unknown);
        assert_eq!(parse_category("ADVERTISING"), TrafficCategory::Advertising);
    }

    #[test]
    fn test_build_classification_prompt_single() {
        let batch = vec![ClassificationRequest {
            conn_id: 1,
            host: "pixel.facebook.com".to_string(),
            port: 443,
            app_uid: None,
            app_label: Some("Instagram".to_string()),
            package_name: Some("com.instagram.android".to_string()),
            reverse_dns: Some("edge-star-shv-01-nrt1.facebook.com".to_string()),
            whois_org: Some("Facebook".to_string()),
            whois_asn: Some(32934),
            whois_country: Some("US".to_string()),
        }];
        let prompt = build_classification_prompt(&batch);
        assert!(prompt.contains("pixel.facebook.com"));
        assert!(prompt.contains("app=Instagram"));
        assert!(prompt.contains("reverse_dns=edge-star-shv-01-nrt1.facebook.com"));
        assert!(prompt.contains("whois_org=Facebook"));
        assert!(prompt.contains("whois_asn=32934"));
        assert!(prompt.contains("<|im_start|>system"));
        assert!(prompt.contains("<|im_start|>assistant"));
    }

    #[test]
    fn test_build_classification_prompt_batch() {
        let batch = vec![
            ClassificationRequest {
                conn_id: 1,
                host: "a.com".to_string(),
                port: 443,
                app_uid: None,
                app_label: None,
                package_name: None,
                reverse_dns: None,
                whois_org: None,
                whois_asn: None,
                whois_country: None,
            },
            ClassificationRequest {
                conn_id: 2,
                host: "b.com".to_string(),
                port: 80,
                app_uid: Some(10001),
                app_label: Some("Chrome".to_string()),
                package_name: Some("com.android.chrome".to_string()),
                reverse_dns: None,
                whois_org: Some("Google".to_string()),
                whois_asn: Some(15169),
                whois_country: Some("US".to_string()),
            },
        ];
        let prompt = build_classification_prompt(&batch);
        assert!(prompt.contains("1. host=a.com port=443"));
        assert!(prompt.contains("2. host=b.com port=80"));
        assert!(prompt.contains("app=Chrome"));
    }

    #[test]
    fn test_apply_results_matches_by_id() {
        let registry = ConnectionRegistry::new();
        let config = hydra_config::IntelligenceConfig {
            auto_block_trackers: false,
            block_confidence_threshold: 0.8,
            verdict_cache_ttl_seconds: 3600,
            verdict_cache_max_entries: 1000,
        };
        let verdict_cache = Arc::new(VerdictCache::new(&config));

        let resolved = registry.resolve_policy("test.example.com:443", None);
        let conn_id = registry.register(
            "test.example.com:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        let batch = vec![ClassificationRequest {
            conn_id,
            host: "test.example.com".to_string(),
            port: 443,
            app_uid: None,
            app_label: None,
            package_name: None,
            reverse_dns: None,
            whois_org: None,
            whois_asn: None,
            whois_country: None,
        }];

        let items = vec![LlmClassificationItem {
            id: 1,
            category: "advertising".to_string(),
            confidence: 0.92,
            explanation: Some("Test ad tracker".to_string()),
        }];

        apply_results(&batch, &items, &registry, &verdict_cache);

        let snaps = registry.snapshot(false);
        let snap = snaps.iter().find(|s| s.id == conn_id).unwrap();
        assert_eq!(
            snap.classification_category.as_deref(),
            Some("advertising")
        );
        assert_eq!(snap.classification_confidence, Some(0.92));
        assert_eq!(
            snap.classification_source.as_deref(),
            Some("llm_analysis")
        );

        let cached = verdict_cache.get(&VerdictKey {
            host_or_domain: "test.example.com".to_string(),
            app_uid: None,
        });
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().category, TrafficCategory::Advertising);
    }

    #[test]
    fn test_apply_fallback_marks_unknown() {
        let registry = ConnectionRegistry::new();
        let config = hydra_config::IntelligenceConfig {
            auto_block_trackers: false,
            block_confidence_threshold: 0.8,
            verdict_cache_ttl_seconds: 3600,
            verdict_cache_max_entries: 1000,
        };
        let verdict_cache = Arc::new(VerdictCache::new(&config));

        let resolved = registry.resolve_policy("api.openai.com:443", None);
        let conn_id = registry.register(
            "api.openai.com:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        let batch = vec![ClassificationRequest {
            conn_id,
            host: "api.openai.com".to_string(),
            port: 443,
            app_uid: None,
            app_label: None,
            package_name: None,
            reverse_dns: None,
            whois_org: None,
            whois_asn: None,
            whois_country: None,
        }];

        apply_fallback(&batch, "Test failure", &registry, &verdict_cache);

        let snaps = registry.snapshot(false);
        let snap = snaps.iter().find(|s| s.id == conn_id).unwrap();
        assert_eq!(snap.classification_category.as_deref(), Some("unknown"));
        assert_eq!(snap.classification_confidence, Some(0.0));
    }

    #[test]
    fn test_confidence_clamped() {
        let registry = ConnectionRegistry::new();
        let config = hydra_config::IntelligenceConfig {
            auto_block_trackers: false,
            block_confidence_threshold: 0.8,
            verdict_cache_ttl_seconds: 3600,
            verdict_cache_max_entries: 1000,
        };
        let verdict_cache = Arc::new(VerdictCache::new(&config));

        let resolved = registry.resolve_policy("x.com:443", None);
        let conn_id = registry.register(
            "x.com:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        let batch = vec![ClassificationRequest {
            conn_id,
            host: "x.com".to_string(),
            port: 443,
            app_uid: None,
            app_label: None,
            package_name: None,
            reverse_dns: None,
            whois_org: None,
            whois_asn: None,
            whois_country: None,
        }];

        let items = vec![LlmClassificationItem {
            id: 1,
            category: "legitimate".to_string(),
            confidence: 1.5, // out of range
            explanation: None,
        }];

        apply_results(&batch, &items, &registry, &verdict_cache);

        let snaps = registry.snapshot(false);
        let snap = snaps.iter().find(|s| s.id == conn_id).unwrap();
        assert_eq!(snap.classification_confidence, Some(1.0));
    }

    #[test]
    fn test_parse_partial_batch_result() {
        // LLM only classified 1 of 2 items
        let input = r#"[{"id":1,"category":"advertising","confidence":0.9,"explanation":"Ad"}]"#;
        let items = parse_classification_response(input);
        assert_eq!(items.len(), 1);

        let registry = ConnectionRegistry::new();
        let config = hydra_config::IntelligenceConfig {
            auto_block_trackers: false,
            block_confidence_threshold: 0.8,
            verdict_cache_ttl_seconds: 3600,
            verdict_cache_max_entries: 1000,
        };
        let verdict_cache = Arc::new(VerdictCache::new(&config));

        let resolved1 = registry.resolve_policy("ad.com:443", None);
        let id1 = registry.register(
            "ad.com:443",
            resolved1.group,
            resolved1.action,
            None,
            None,
        );
        let resolved2 = registry.resolve_policy("ok.com:443", None);
        let id2 = registry.register(
            "ok.com:443",
            resolved2.group,
            resolved2.action,
            None,
            None,
        );

        let batch = vec![
            ClassificationRequest {
                conn_id: id1,
                host: "ad.com".to_string(),
                port: 443,
                app_uid: None,
                app_label: None,
                package_name: None,
                reverse_dns: None,
                whois_org: None,
                whois_asn: None,
                whois_country: None,
            },
            ClassificationRequest {
                conn_id: id2,
                host: "ok.com".to_string(),
                port: 443,
                app_uid: None,
                app_label: None,
                package_name: None,
                reverse_dns: None,
                whois_org: None,
                whois_asn: None,
                whois_country: None,
            },
        ];

        apply_results(&batch, &items, &registry, &verdict_cache);

        let snaps = registry.snapshot(false);
        let snap1 = snaps.iter().find(|s| s.id == id1).unwrap();
        assert_eq!(
            snap1.classification_category.as_deref(),
            Some("advertising")
        );

        let snap2 = snaps.iter().find(|s| s.id == id2).unwrap();
        assert_eq!(snap2.classification_category.as_deref(), Some("unknown"));
    }
}
