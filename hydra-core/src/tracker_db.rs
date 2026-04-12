//! Tracker Database and Static Classifier
//!
//! This module provides embedded tracker classification for known advertising,
//! analytics, social tracking, telemetry, fingerprinting, and malware domains.
//!
//! # Data Sources
//! The tracker list is curated from:
//! - Disconnect Tracking Protection List
//! - DuckDuckGo Tracker Radar
//! - EasyList
//! - uBlock Origin
//!
//! # Updating the Tracker List
//! To update the tracker database:
//! 1. Edit `hydra-core/data/trackers.json`
//! 2. Add/remove domains in the appropriate category
//! 3. Rebuild the project: `cargo build -p hydra-core`
//!
//! The JSON file is embedded at compile time via `include_str!()`.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::debug;

const TRACKERS_JSON: &str = include_str!("../data/trackers.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrafficCategory {
    Advertising,
    Analytics,
    SocialTracking,
    Telemetry,
    Fingerprinting,
    Malware,
    ContentDelivery,
    Unknown,
}

impl TrafficCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            TrafficCategory::Advertising => "advertising",
            TrafficCategory::Analytics => "analytics",
            TrafficCategory::SocialTracking => "social_tracking",
            TrafficCategory::Telemetry => "telemetry",
            TrafficCategory::Fingerprinting => "fingerprinting",
            TrafficCategory::Malware => "malware",
            TrafficCategory::ContentDelivery => "content_delivery",
            TrafficCategory::Unknown => "unknown",
        }
    }

    fn from_key(key: &str) -> Self {
        match key {
            "advertising" => TrafficCategory::Advertising,
            "analytics" => TrafficCategory::Analytics,
            "social_tracking" => TrafficCategory::SocialTracking,
            "telemetry" => TrafficCategory::Telemetry,
            "fingerprinting" => TrafficCategory::Fingerprinting,
            "malware" => TrafficCategory::Malware,
            "content_delivery" => TrafficCategory::ContentDelivery,
            _ => TrafficCategory::Unknown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerEntry {
    pub company: Option<String>,
    pub category: TrafficCategory,
    pub source_db: &'static str,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackerMatch {
    pub domain_matched: String,
    pub company: Option<String>,
    pub category: TrafficCategory,
}

#[derive(Deserialize)]
struct TrackerJsonCategory {
    domains: Vec<String>,
}

#[derive(Deserialize)]
struct TrackerJsonCompany {
    name: String,
    domains: Vec<String>,
}

#[derive(Deserialize)]
struct TrackerJson {
    categories: HashMap<String, TrackerJsonCategory>,
    companies: HashMap<String, TrackerJsonCompany>,
}

pub struct TrackerDatabase {
    entries: HashMap<String, TrackerEntry>,
    domain_to_company: HashMap<String, String>,
}

impl TrackerDatabase {
    pub fn new() -> Self {
        let json: TrackerJson =
            serde_json::from_str(TRACKERS_JSON).expect("Failed to parse embedded trackers.json");

        let mut entries = HashMap::new();
        let mut domain_to_company: HashMap<String, String> = HashMap::new();

        for (company_key, company_info) in &json.companies {
            for domain in &company_info.domains {
                domain_to_company.insert(domain.to_lowercase(), company_info.name.clone());
            }
            debug!(
                "Loaded company {} with {} domains",
                company_key,
                company_info.domains.len()
            );
        }

        for (category_key, category_info) in &json.categories {
            let category = TrafficCategory::from_key(category_key);
            for domain in &category_info.domains {
                let domain_lower = domain.to_lowercase();
                let company = domain_to_company.get(&domain_lower).cloned();
                entries.insert(
                    domain_lower,
                    TrackerEntry {
                        company,
                        category,
                        source_db: "hydra-curated",
                    },
                );
            }
            debug!(
                "Loaded category {} with {} domains",
                category_key,
                category_info.domains.len()
            );
        }

        debug!("TrackerDatabase initialized with {} entries", entries.len());

        Self {
            entries,
            domain_to_company,
        }
    }

    pub fn classify_domain(&self, domain: &str) -> Option<TrackerMatch> {
        let domain_lower = domain.to_lowercase();

        if let Some(entry) = self.entries.get(&domain_lower) {
            return Some(TrackerMatch {
                domain_matched: domain_lower,
                company: entry.company.clone(),
                category: entry.category,
            });
        }

        let parts: Vec<&str> = domain_lower.split('.').collect();
        for i in 1..parts.len().saturating_sub(1) {
            let parent = parts[i..].join(".");
            if let Some(entry) = self.entries.get(&parent) {
                return Some(TrackerMatch {
                    domain_matched: parent,
                    company: entry.company.clone(),
                    category: entry.category,
                });
            }
        }

        None
    }

    pub fn classify_ip(&self, _ip: &str, reverse_dns: Option<&str>) -> Option<TrackerMatch> {
        match reverse_dns {
            Some(ptr_domain) => self.classify_domain(ptr_domain),
            None => None,
        }
    }

    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    pub fn categories_summary(&self) -> HashMap<TrafficCategory, usize> {
        let mut summary = HashMap::new();
        for entry in self.entries.values() {
            *summary.entry(entry.category).or_insert(0) += 1;
        }
        summary
    }

    pub fn company_for_domain(&self, domain: &str) -> Option<&String> {
        let domain_lower = domain.to_lowercase();

        if let Some(company) = self.domain_to_company.get(&domain_lower) {
            return Some(company);
        }

        let parts: Vec<&str> = domain_lower.split('.').collect();
        for i in 1..parts.len().saturating_sub(1) {
            let parent = parts[i..].join(".");
            if let Some(company) = self.domain_to_company.get(&parent) {
                return Some(company);
            }
        }

        None
    }
}

impl Default for TrackerDatabase {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_loads() {
        let db = TrackerDatabase::new();
        assert!(db.entry_count() > 100, "Should have at least 100 tracker entries");
    }

    #[test]
    fn test_classify_google_analytics() {
        let db = TrackerDatabase::new();
        let result = db.classify_domain("google-analytics.com");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.category, TrafficCategory::Analytics);
        assert_eq!(m.domain_matched, "google-analytics.com");
    }

    #[test]
    fn test_classify_subdomain() {
        let db = TrackerDatabase::new();
        // ssl.google-analytics.com is explicitly in the database
        let result = db.classify_domain("ssl.google-analytics.com");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.category, TrafficCategory::Analytics);
        
        // Test parent domain fallback with a subdomain not in the database
        let result2 = db.classify_domain("tracking.google-analytics.com");
        assert!(result2.is_some());
        let m2 = result2.unwrap();
        assert_eq!(m2.category, TrafficCategory::Analytics);
        assert_eq!(m2.domain_matched, "google-analytics.com");
    }

    #[test]
    fn test_classify_deep_subdomain() {
        let db = TrackerDatabase::new();
        let result = db.classify_domain("pixel.ad.doubleclick.net");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.category, TrafficCategory::Advertising);
        assert_eq!(m.domain_matched, "doubleclick.net");
    }

    #[test]
    fn test_classify_facebook_pixel() {
        let db = TrackerDatabase::new();
        let result = db.classify_domain("pixel.facebook.com");
        assert!(result.is_some());
        let m = result.unwrap();
        assert_eq!(m.category, TrafficCategory::SocialTracking);
    }

    #[test]
    fn test_classify_advertising() {
        let db = TrackerDatabase::new();
        let result = db.classify_domain("adnxs.com");
        assert!(result.is_some());
        assert_eq!(result.unwrap().category, TrafficCategory::Advertising);
    }

    #[test]
    fn test_classify_telemetry() {
        let db = TrackerDatabase::new();
        let result = db.classify_domain("telemetry.microsoft.com");
        assert!(result.is_some());
        assert_eq!(result.unwrap().category, TrafficCategory::Telemetry);
    }

    #[test]
    fn test_classify_fingerprinting() {
        let db = TrackerDatabase::new();
        let result = db.classify_domain("fingerprintjs.com");
        assert!(result.is_some());
        assert_eq!(result.unwrap().category, TrafficCategory::Fingerprinting);
    }

    #[test]
    fn test_classify_yandex_metrika() {
        let db = TrackerDatabase::new();
        let result = db.classify_domain("mc.yandex.ru");
        assert!(result.is_some());
        assert_eq!(result.unwrap().category, TrafficCategory::Analytics);
    }

    #[test]
    fn test_legitimate_domain_not_matched() {
        let db = TrackerDatabase::new();
        assert!(db.classify_domain("example.com").is_none());
        assert!(db.classify_domain("github.com").is_none());
        assert!(db.classify_domain("rust-lang.org").is_none());
        assert!(db.classify_domain("wikipedia.org").is_none());
    }

    #[test]
    fn test_classify_ip_with_reverse_dns() {
        let db = TrackerDatabase::new();
        let result = db.classify_ip("142.250.185.206", Some("pagead2.googlesyndication.com"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().category, TrafficCategory::Advertising);
    }

    #[test]
    fn test_classify_ip_without_reverse_dns() {
        let db = TrackerDatabase::new();
        let result = db.classify_ip("8.8.8.8", None);
        assert!(result.is_none());
    }

    #[test]
    fn test_categories_summary() {
        let db = TrackerDatabase::new();
        let summary = db.categories_summary();
        assert!(summary.contains_key(&TrafficCategory::Advertising));
        assert!(summary.contains_key(&TrafficCategory::Analytics));
        assert!(summary.contains_key(&TrafficCategory::SocialTracking));
        assert!(*summary.get(&TrafficCategory::Advertising).unwrap_or(&0) > 10);
    }

    #[test]
    fn test_company_lookup() {
        let db = TrackerDatabase::new();
        let company = db.company_for_domain("youtube.com");
        assert!(company.is_some());
        assert_eq!(company.unwrap(), "Google LLC");
    }

    #[test]
    fn test_company_lookup_subdomain() {
        let db = TrackerDatabase::new();
        let company = db.company_for_domain("api.facebook.com");
        assert!(company.is_some());
        assert_eq!(company.unwrap(), "Meta Platforms Inc");
    }

    #[test]
    fn test_case_insensitive() {
        let db = TrackerDatabase::new();
        let result1 = db.classify_domain("Google-Analytics.com");
        let result2 = db.classify_domain("GOOGLE-ANALYTICS.COM");
        let result3 = db.classify_domain("google-analytics.com");
        assert!(result1.is_some());
        assert!(result2.is_some());
        assert!(result3.is_some());
    }
}
