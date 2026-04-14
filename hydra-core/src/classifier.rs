use crate::connections::{
    ClassificationSource, ConnectionClassification, ConnectionRegistry, TrafficCategory,
};
use crate::classification_log::{make_event, ClassificationEventLog};
use crate::enrichment::EnrichmentResult;
use crate::tracker_db::TrackerDatabase;
use hydra_config::IntelligenceConfig;
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::info;

const WHOIS_AD_KEYWORDS: &[&str] = &[
    "advertising",
    "analytics",
    "marketing",
    "tracker",
    "adtech",
    "admob",
    "adsense",
    "adserver",
    "doubleclick",
    "taboola",
    "outbrain",
    "criteo",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VerdictKey {
    pub host_or_domain: String,
    pub app_uid: Option<u32>,
}

pub struct VerdictCache {
    inner: Cache<VerdictKey, ConnectionClassification>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl VerdictCache {
    pub fn new(config: &IntelligenceConfig) -> Self {
        let ttl = Duration::from_secs(config.verdict_cache_ttl_seconds);
        let cache = Cache::builder()
            .time_to_live(ttl)
            .max_capacity(config.verdict_cache_max_entries)
            .build();
        Self {
            inner: cache,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    pub fn get(&self, key: &VerdictKey) -> Option<ConnectionClassification> {
        match self.inner.get(key) {
            Some(v) => {
                self.hits.fetch_add(1, Ordering::Relaxed);
                Some(v)
            }
            None => {
                self.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    pub fn put(&self, key: VerdictKey, verdict: ConnectionClassification) {
        self.inner.insert(key, verdict);
    }

    pub fn sync(&self) {
        self.inner.run_pending_tasks();
    }

    pub fn size(&self) -> u64 {
        self.inner.run_pending_tasks();
        self.inner.entry_count()
    }

    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            hits as f64 / total as f64
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassificationRequest {
    pub conn_id: u64,
    pub host: String,
    pub port: u16,
    pub app_uid: Option<u32>,
    pub app_label: Option<String>,
    pub package_name: Option<String>,
    pub reverse_dns: Option<String>,
    pub whois_org: Option<String>,
    pub whois_asn: Option<u32>,
    pub whois_country: Option<String>,
}

pub enum ClassificationResult {
    Verdict(ConnectionClassification),
    Pending,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifierStats {
    pub cache_size: u64,
    pub cache_hit_rate: f64,
    pub tracker_hits: u64,
    pub rules_applied: u64,
    pub llm_pending: u64,
}

pub struct ConnectionClassifier {
    tracker_db: Arc<TrackerDatabase>,
    verdict_cache: Arc<VerdictCache>,
    llm_tx: mpsc::Sender<ClassificationRequest>,
    auto_block: AtomicBool,
    block_confidence_threshold: f32,
    tracker_hits: AtomicU64,
    rules_applied: AtomicU64,
}

impl ConnectionClassifier {
    pub fn new(
        tracker_db: Arc<TrackerDatabase>,
        config: &IntelligenceConfig,
    ) -> (Self, mpsc::Receiver<ClassificationRequest>) {
        let (llm_tx, llm_rx) = mpsc::channel::<ClassificationRequest>(256);
        let verdict_cache = Arc::new(VerdictCache::new(config));

        let classifier = Self {
            tracker_db,
            verdict_cache,
            llm_tx,
            auto_block: AtomicBool::new(config.auto_block_trackers),
            block_confidence_threshold: config.block_confidence_threshold,
            tracker_hits: AtomicU64::new(0),
            rules_applied: AtomicU64::new(0),
        };

        (classifier, llm_rx)
    }

    pub fn classify_fast(
        &self,
        host: &str,
        _port: u16,
        app_uid: Option<u32>,
    ) -> ClassificationResult {
        let key = VerdictKey {
            host_or_domain: host.to_lowercase(),
            app_uid,
        };

        // Tier 1: verdict cache
        if let Some(cached) = self.verdict_cache.get(&key) {
            info!(
                host = host,
                category = cached.category.as_str(),
                confidence = cached.confidence,
                source = "rule_cache",
                "classification_fast: cache hit"
            );
            return ClassificationResult::Verdict(cached);
        }

        // Tier 2a: tracker database
        if let Some(tracker_match) = self.tracker_db.classify_domain(host) {
            let classification = ConnectionClassification {
                category: map_tracker_category(tracker_match.category),
                confidence: 0.95,
                source: ClassificationSource::TrackerDb,
                explanation: Some(format!(
                    "Matched tracker list: {} ({})",
                    tracker_match.domain_matched,
                    tracker_match
                        .company
                        .as_deref()
                        .unwrap_or("unknown company")
                )),
            };
            self.tracker_hits.fetch_add(1, Ordering::Relaxed);
            self.verdict_cache.put(key, classification.clone());

            info!(
                host = host,
                category = classification.category.as_str(),
                confidence = classification.confidence,
                source = "tracker_db",
                matched_domain = tracker_match.domain_matched.as_str(),
                "classification_fast: tracker db hit"
            );

            return ClassificationResult::Verdict(classification);
        }

        ClassificationResult::Pending
    }

    pub async fn classify_enriched(
        &self,
        conn_id: u64,
        host: &str,
        port: u16,
        app_uid: Option<u32>,
        app_label: Option<&str>,
        package_name: Option<&str>,
        enrichment: &EnrichmentResult,
        registry: &ConnectionRegistry,
        classification_log: &ClassificationEventLog,
    ) {
        let key = VerdictKey {
            host_or_domain: host.to_lowercase(),
            app_uid,
        };

        // Already classified (fast path already cached a verdict)
        if self.verdict_cache.get(&key).is_some() {
            return;
        }

        // Re-check tracker_db with reverse DNS (may match where raw IP didn't)
        if let Some(ref reverse_dns) = enrichment.reverse_dns {
            if let Some(tracker_match) = self.tracker_db.classify_domain(reverse_dns) {
                let classification = ConnectionClassification {
                    category: map_tracker_category(tracker_match.category),
                    confidence: 0.90,
                    source: ClassificationSource::TrackerDb,
                    explanation: Some(format!(
                        "PTR {} matched tracker list: {} ({})",
                        reverse_dns,
                        tracker_match.domain_matched,
                        tracker_match
                            .company
                            .as_deref()
                            .unwrap_or("unknown company")
                    )),
                };
                self.tracker_hits.fetch_add(1, Ordering::Relaxed);
                self.verdict_cache.put(key, classification.clone());
                registry.update_classification(conn_id, classification.clone());
                classification_log.log_event(make_event(
                    host.to_string(),
                    port,
                    app_uid,
                    package_name.map(String::from),
                    enrichment.reverse_dns.clone(),
                    enrichment.whois_org.clone(),
                    enrichment.asn,
                    enrichment.country.clone(),
                    classification.category,
                    classification.confidence,
                    classification.source,
                    classification.explanation.clone(),
                    0,
                    0,
                    None,
                ));

                info!(
                    conn_id = conn_id,
                    host = host,
                    reverse_dns = reverse_dns.as_str(),
                    category = classification.category.as_str(),
                    confidence = classification.confidence,
                    source = "tracker_db",
                    "classification_enriched: tracker db hit via PTR"
                );
                return;
            }
        }

        // WHOIS-based heuristic rules
        if let Some(classification) = self.apply_whois_rules(host, enrichment) {
            self.rules_applied.fetch_add(1, Ordering::Relaxed);
            self.verdict_cache.put(key, classification.clone());
            registry.update_classification(conn_id, classification.clone());
            classification_log.log_event(make_event(
                host.to_string(),
                port,
                app_uid,
                package_name.map(String::from),
                enrichment.reverse_dns.clone(),
                enrichment.whois_org.clone(),
                enrichment.asn,
                enrichment.country.clone(),
                classification.category,
                classification.confidence,
                classification.source,
                classification.explanation.clone(),
                0,
                0,
                None,
            ));

            info!(
                conn_id = conn_id,
                host = host,
                category = classification.category.as_str(),
                confidence = classification.confidence,
                source = "whois_rules",
                whois_org = enrichment.whois_org.as_deref().unwrap_or(""),
                whois_asn = enrichment.asn.unwrap_or(0),
                "classification_enriched: whois rule hit"
            );
            return;
        }

        // Tier 3: send to LLM queue
        let request = ClassificationRequest {
            conn_id,
            host: host.to_string(),
            port,
            app_uid,
            app_label: app_label.map(String::from),
            package_name: package_name.map(String::from),
            reverse_dns: enrichment.reverse_dns.clone(),
            whois_org: enrichment.whois_org.clone(),
            whois_asn: enrichment.asn,
            whois_country: enrichment.country.clone(),
        };

        if self.llm_tx.try_send(request).is_err() {
            tracing::debug!(
                conn_id = conn_id,
                host = host,
                "LLM classification queue full, skipping"
            );
        } else {
            classification_log.log_event(make_event(
                host.to_string(),
                port,
                app_uid,
                package_name.map(String::from),
                enrichment.reverse_dns.clone(),
                enrichment.whois_org.clone(),
                enrichment.asn,
                enrichment.country.clone(),
                TrafficCategory::Unknown,
                0.0,
                ClassificationSource::LlmAnalysis,
                Some("Classification queued for LLM analysis".to_string()),
                0,
                0,
                None,
            ));
        }
    }

    fn apply_whois_rules(
        &self,
        _host: &str,
        enrichment: &EnrichmentResult,
    ) -> Option<ConnectionClassification> {
        if let Some(ref org) = enrichment.whois_org {
            let org_lower = org.to_lowercase();
            for keyword in WHOIS_AD_KEYWORDS {
                if org_lower.contains(keyword) {
                    return Some(ConnectionClassification {
                        category: TrafficCategory::Advertising,
                        confidence: 0.70,
                        source: ClassificationSource::RuleCache,
                        explanation: Some(format!(
                            "WHOIS org '{}' contains ad/tracker keyword '{}'",
                            org, keyword
                        )),
                    });
                }
            }
        }

        None
    }

    pub fn should_block(&self, classification: &ConnectionClassification) -> bool {
        self.auto_block.load(Ordering::Relaxed)
            && classification.category.is_tracker()
            && classification.confidence >= self.block_confidence_threshold
    }

    pub fn set_auto_block(&self, enabled: bool) {
        self.auto_block.store(enabled, Ordering::Relaxed);
        info!(enabled = enabled, "intelligence auto_block_trackers updated");
    }

    pub fn auto_block_enabled(&self) -> bool {
        self.auto_block.load(Ordering::Relaxed)
    }

    pub fn verdict_cache(&self) -> &Arc<VerdictCache> {
        &self.verdict_cache
    }

    pub fn stats(&self) -> ClassifierStats {
        ClassifierStats {
            cache_size: self.verdict_cache.size(),
            cache_hit_rate: self.verdict_cache.hit_rate(),
            tracker_hits: self.tracker_hits.load(Ordering::Relaxed),
            rules_applied: self.rules_applied.load(Ordering::Relaxed),
            llm_pending: self.llm_tx.max_capacity() as u64
                - self.llm_tx.capacity() as u64,
        }
    }
}

fn map_tracker_category(
    tc: crate::tracker_db::TrafficCategory,
) -> TrafficCategory {
    match tc {
        crate::tracker_db::TrafficCategory::Advertising => TrafficCategory::Advertising,
        crate::tracker_db::TrafficCategory::Analytics => TrafficCategory::Analytics,
        crate::tracker_db::TrafficCategory::SocialTracking => TrafficCategory::SocialTracking,
        crate::tracker_db::TrafficCategory::Telemetry => TrafficCategory::Telemetry,
        crate::tracker_db::TrafficCategory::Fingerprinting => TrafficCategory::Analytics,
        crate::tracker_db::TrafficCategory::Malware => TrafficCategory::Malware,
        crate::tracker_db::TrafficCategory::ContentDelivery => TrafficCategory::Legitimate,
        crate::tracker_db::TrafficCategory::Unknown => TrafficCategory::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> IntelligenceConfig {
        IntelligenceConfig {
            auto_block_trackers: false,
            block_confidence_threshold: 0.8,
            verdict_cache_ttl_seconds: 3600,
            verdict_cache_max_entries: 1000,
        }
    }

    fn test_classification_log() -> ClassificationEventLog {
        let dir = tempfile::tempdir().unwrap();
        ClassificationEventLog::new(dir.into_path()).unwrap()
    }

    #[test]
    fn test_classify_fast_known_tracker() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        match classifier.classify_fast("doubleclick.net", 443, None) {
            ClassificationResult::Verdict(v) => {
                assert_eq!(v.category, TrafficCategory::Advertising);
                assert_eq!(v.source, ClassificationSource::TrackerDb);
                assert!(v.confidence > 0.9);
            }
            ClassificationResult::Pending => panic!("Expected Verdict for known tracker"),
        }
    }

    #[test]
    fn test_classify_fast_unknown_domain_returns_pending() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        match classifier.classify_fast("github.com", 443, None) {
            ClassificationResult::Pending => {}
            ClassificationResult::Verdict(_) => {
                panic!("Expected Pending for unknown domain")
            }
        }
    }

    #[test]
    fn test_classify_fast_cache_hit_on_second_call() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        // First call populates cache
        let _ = classifier.classify_fast("google-analytics.com", 443, None);

        // Second call should hit cache
        match classifier.classify_fast("google-analytics.com", 443, None) {
            ClassificationResult::Verdict(v) => {
                assert_eq!(v.category, TrafficCategory::Analytics);
            }
            ClassificationResult::Pending => panic!("Expected cache hit"),
        }

        assert!(classifier.verdict_cache.hit_rate() > 0.0);
    }

    #[test]
    fn test_classify_fast_subdomain_tracker() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        match classifier.classify_fast("pixel.ad.doubleclick.net", 443, None) {
            ClassificationResult::Verdict(v) => {
                assert_eq!(v.category, TrafficCategory::Advertising);
            }
            ClassificationResult::Pending => panic!("Expected Verdict for tracker subdomain"),
        }
    }

    #[test]
    fn test_should_block_respects_auto_block_flag() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        let verdict = ConnectionClassification {
            category: TrafficCategory::Advertising,
            confidence: 0.95,
            source: ClassificationSource::TrackerDb,
            explanation: None,
        };

        // auto_block is false by default in test_config
        assert!(!classifier.should_block(&verdict));

        classifier.set_auto_block(true);
        assert!(classifier.should_block(&verdict));
    }

    #[test]
    fn test_should_block_respects_confidence_threshold() {
        let db = Arc::new(TrackerDatabase::new());
        let config = IntelligenceConfig {
            auto_block_trackers: true,
            block_confidence_threshold: 0.8,
            ..test_config()
        };
        let (classifier, _rx) = ConnectionClassifier::new(db, &config);

        let low_confidence = ConnectionClassification {
            category: TrafficCategory::Advertising,
            confidence: 0.5,
            source: ClassificationSource::RuleCache,
            explanation: None,
        };
        assert!(!classifier.should_block(&low_confidence));

        let high_confidence = ConnectionClassification {
            category: TrafficCategory::Advertising,
            confidence: 0.95,
            source: ClassificationSource::TrackerDb,
            explanation: None,
        };
        assert!(classifier.should_block(&high_confidence));
    }

    #[test]
    fn test_should_block_does_not_block_legitimate() {
        let db = Arc::new(TrackerDatabase::new());
        let config = IntelligenceConfig {
            auto_block_trackers: true,
            ..test_config()
        };
        let (classifier, _rx) = ConnectionClassifier::new(db, &config);

        let legitimate = ConnectionClassification {
            category: TrafficCategory::Legitimate,
            confidence: 0.99,
            source: ClassificationSource::TrackerDb,
            explanation: None,
        };
        assert!(!classifier.should_block(&legitimate));
    }

    #[test]
    fn test_whois_rules_detect_ad_org() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        let enrichment = EnrichmentResult {
            reverse_dns: None,
            whois_org: Some("DoubleClick Advertising Network".to_string()),
            asn: Some(15169),
            country: Some("US".to_string()),
        };

        let result = classifier.apply_whois_rules("ad.example.com", &enrichment);
        assert!(result.is_some());
        let classification = result.unwrap();
        assert_eq!(classification.category, TrafficCategory::Advertising);
        assert_eq!(classification.source, ClassificationSource::RuleCache);
    }

    #[test]
    fn test_whois_rules_no_match_for_normal_org() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        let enrichment = EnrichmentResult {
            reverse_dns: None,
            whois_org: Some("GitHub Inc.".to_string()),
            asn: Some(36459),
            country: Some("US".to_string()),
        };

        let result = classifier.apply_whois_rules("github.com", &enrichment);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_classify_enriched_with_ptr_tracker() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());
        let registry = ConnectionRegistry::new();

        let resolved = registry.resolve_policy("142.250.185.100:443", None);
        let conn_id = registry.register(
            "142.250.185.100:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        let enrichment = EnrichmentResult {
            reverse_dns: Some("pagead2.googlesyndication.com".to_string()),
            whois_org: Some("Google LLC".to_string()),
            asn: Some(15169),
            country: Some("US".to_string()),
        };
        let classification_log = test_classification_log();

        classifier
            .classify_enriched(
                conn_id,
                "142.250.185.100",
                443,
                None,
                None,
                None,
                &enrichment,
                &registry,
                &classification_log,
            )
            .await;

        let snaps = registry.snapshot(false);
        let snap = snaps.iter().find(|s| s.id == conn_id).unwrap();
        assert_eq!(
            snap.classification_category.as_deref(),
            Some("advertising")
        );
    }

    #[tokio::test]
    async fn test_classify_enriched_sends_to_llm_queue() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, mut rx) = ConnectionClassifier::new(db, &test_config());
        let registry = ConnectionRegistry::new();

        let resolved = registry.resolve_policy("api.openai.com:443", None);
        let conn_id = registry.register(
            "api.openai.com:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        let enrichment = EnrichmentResult {
            reverse_dns: Some("api.openai.com".to_string()),
            whois_org: Some("Microsoft Corporation".to_string()),
            asn: Some(8075),
            country: Some("US".to_string()),
        };
        let classification_log = test_classification_log();

        classifier
            .classify_enriched(
                conn_id,
                "api.openai.com",
                443,
                None,
                None,
                None,
                &enrichment,
                &registry,
                &classification_log,
            )
            .await;

        // Should have sent to LLM queue
        let request = rx.try_recv();
        assert!(request.is_ok());
        let req = request.unwrap();
        assert_eq!(req.conn_id, conn_id);
        assert_eq!(req.host, "api.openai.com");
    }

    #[test]
    fn test_verdict_cache_basic() {
        let config = test_config();
        let cache = VerdictCache::new(&config);

        let key = VerdictKey {
            host_or_domain: "test.com".to_string(),
            app_uid: None,
        };

        assert!(cache.get(&key).is_none());
        assert_eq!(cache.size(), 0);

        cache.put(
            key.clone(),
            ConnectionClassification {
                category: TrafficCategory::Advertising,
                confidence: 0.9,
                source: ClassificationSource::TrackerDb,
                explanation: None,
            },
        );

        assert!(cache.get(&key).is_some());
        assert_eq!(cache.size(), 1);
    }

    #[test]
    fn test_verdict_cache_app_uid_differentiation() {
        let config = test_config();
        let cache = VerdictCache::new(&config);

        let key_no_uid = VerdictKey {
            host_or_domain: "example.com".to_string(),
            app_uid: None,
        };
        let key_with_uid = VerdictKey {
            host_or_domain: "example.com".to_string(),
            app_uid: Some(1001),
        };

        cache.put(
            key_no_uid.clone(),
            ConnectionClassification {
                category: TrafficCategory::Advertising,
                confidence: 0.9,
                source: ClassificationSource::TrackerDb,
                explanation: None,
            },
        );

        assert!(cache.get(&key_no_uid).is_some());
        assert!(cache.get(&key_with_uid).is_none());
    }

    #[test]
    fn test_classifier_stats() {
        let db = Arc::new(TrackerDatabase::new());
        let (classifier, _rx) = ConnectionClassifier::new(db, &test_config());

        let stats = classifier.stats();
        assert_eq!(stats.cache_size, 0);
        assert_eq!(stats.tracker_hits, 0);
        assert_eq!(stats.rules_applied, 0);

        // Classify a known tracker to bump stats
        let _ = classifier.classify_fast("doubleclick.net", 443, None);

        let stats = classifier.stats();
        assert_eq!(stats.cache_size, 1);
        assert_eq!(stats.tracker_hits, 1);
    }

    #[test]
    fn test_map_tracker_category() {
        use crate::tracker_db::TrafficCategory as TC;

        assert_eq!(map_tracker_category(TC::Advertising), TrafficCategory::Advertising);
        assert_eq!(map_tracker_category(TC::Analytics), TrafficCategory::Analytics);
        assert_eq!(map_tracker_category(TC::SocialTracking), TrafficCategory::SocialTracking);
        assert_eq!(map_tracker_category(TC::Telemetry), TrafficCategory::Telemetry);
        assert_eq!(map_tracker_category(TC::Fingerprinting), TrafficCategory::Analytics);
        assert_eq!(map_tracker_category(TC::Malware), TrafficCategory::Malware);
        assert_eq!(map_tracker_category(TC::ContentDelivery), TrafficCategory::Legitimate);
        assert_eq!(map_tracker_category(TC::Unknown), TrafficCategory::Unknown);
    }
}
