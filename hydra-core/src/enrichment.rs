use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::TokioResolver;
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use tracing::{debug, warn};
use whois_rust::{WhoIs, WhoIsLookupOptions};

const LOOKUP_TIMEOUT: Duration = Duration::from_secs(3);
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours
const CACHE_MAX_ENTRIES: u64 = 10_000;

const WHOIS_SERVERS_JSON: &str = r#"{
    "": "whois.ripe.net",
    "_": {
        "ip": {
            "host": "whois.arin.net",
            "query": "n + $addr\r\n"
        }
    }
}"#;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EnrichmentResult {
    pub reverse_dns: Option<String>,
    pub whois_org: Option<String>,
    pub asn: Option<u32>,
    pub country: Option<String>,
}

pub struct EnrichmentService {
    resolver: TokioResolver,
    whois: Arc<WhoIs>,
    cache: Cache<IpAddr, EnrichmentResult>,
}

impl EnrichmentService {
    pub fn new() -> anyhow::Result<Self> {
        let resolver = TokioResolver::builder_with_config(
            ResolverConfig::google(),
            TokioConnectionProvider::default(),
        )
        .build();

        let whois = WhoIs::from_string(WHOIS_SERVERS_JSON)
            .map_err(|e| anyhow::anyhow!("Failed to parse WHOIS servers config: {}", e))?;

        let cache = Cache::builder()
            .time_to_live(CACHE_TTL)
            .max_capacity(CACHE_MAX_ENTRIES)
            .build();

        Ok(Self {
            resolver,
            whois: Arc::new(whois),
            cache,
        })
    }

    pub async fn enrich(&self, ip: IpAddr) -> EnrichmentResult {
        if let Some(cached) = self.cache.get(&ip).await {
            debug!("Enrichment cache hit for {}", ip);
            return cached;
        }

        let reverse_dns = self.lookup_reverse_dns(ip).await;
        let (whois_org, asn, country) = self.lookup_whois(ip).await;

        let result = EnrichmentResult {
            reverse_dns,
            whois_org,
            asn,
            country,
        };

        self.cache.insert(ip, result.clone()).await;
        result
    }

    pub async fn enrich_host(&self, host: &str) -> EnrichmentResult {
        if let Ok(ip) = host.parse::<IpAddr>() {
            return self.enrich(ip).await;
        }

        let ip = match self.resolve_domain_to_ip(host).await {
            Some(ip) => ip,
            None => {
                return EnrichmentResult {
                    reverse_dns: Some(host.to_string()),
                    whois_org: None,
                    asn: None,
                    country: None,
                };
            }
        };

        let mut result = self.enrich(ip).await;
        result.reverse_dns = Some(host.to_string());
        result
    }

    async fn resolve_domain_to_ip(&self, domain: &str) -> Option<IpAddr> {
        match timeout(LOOKUP_TIMEOUT, self.resolver.lookup_ip(domain)).await {
            Ok(Ok(lookup)) => lookup.iter().next(),
            Ok(Err(e)) => {
                debug!("DNS A lookup failed for {}: {}", domain, e);
                None
            }
            Err(_) => {
                warn!("DNS A lookup timeout for {}", domain);
                None
            }
        }
    }

    async fn lookup_reverse_dns(&self, ip: IpAddr) -> Option<String> {
        match timeout(LOOKUP_TIMEOUT, self.resolver.reverse_lookup(ip)).await {
            Ok(Ok(lookup)) => lookup.iter().next().map(|name| {
                let name_str = name.to_string();
                name_str.trim_end_matches('.').to_string()
            }),
            Ok(Err(e)) => {
                debug!("Reverse DNS lookup failed for {}: {}", ip, e);
                None
            }
            Err(_) => {
                warn!("Reverse DNS lookup timeout for {}", ip);
                None
            }
        }
    }

    async fn lookup_whois(&self, ip: IpAddr) -> (Option<String>, Option<u32>, Option<String>) {
        let whois = self.whois.clone();
        let ip_str = ip.to_string();

        let response = match timeout(
            LOOKUP_TIMEOUT,
            tokio::task::spawn_blocking(move || {
                let options = match WhoIsLookupOptions::from_string(&ip_str) {
                    Ok(opts) => opts,
                    Err(e) => {
                        debug!("Failed to create WHOIS options for {}: {}", ip_str, e);
                        return None;
                    }
                };
                match whois.lookup(options) {
                    Ok(response) => Some(response),
                    Err(e) => {
                        debug!("WHOIS lookup failed for {}: {}", ip_str, e);
                        None
                    }
                }
            }),
        )
        .await
        {
            Ok(Ok(Some(response))) => response,
            Ok(Ok(None)) => return (None, None, None),
            Ok(Err(e)) => {
                warn!("WHOIS task failed for {}: {}", ip, e);
                return (None, None, None);
            }
            Err(_) => {
                warn!("WHOIS lookup timeout for {}", ip);
                return (None, None, None);
            }
        };

        let org = parse_whois_org(&response);
        let asn = parse_whois_asn(&response);
        let country = parse_whois_country(&response);

        (org, asn, country)
    }

    pub fn cache_size(&self) -> u64 {
        self.cache.entry_count()
    }
}

fn parse_whois_org(response: &str) -> Option<String> {
    for line in response.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.starts_with("orgname:")
            || line_lower.starts_with("org-name:")
            || line_lower.starts_with("organisation:")
            || line_lower.starts_with("organization:")
            || line_lower.starts_with("owner:")
            || line_lower.starts_with("descr:")
        {
            let value = line.splitn(2, ':').nth(1)?.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn parse_whois_asn(response: &str) -> Option<u32> {
    for line in response.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.starts_with("originas:")
            || line_lower.starts_with("origin:")
            || line_lower.starts_with("originateas:")
        {
            let value = line.splitn(2, ':').nth(1)?.trim();
            let asn_str = value.trim_start_matches(|c| c == 'A' || c == 'S' || c == 'a' || c == 's');
            if let Ok(asn) = asn_str.parse::<u32>() {
                return Some(asn);
            }
        }
    }
    None
}

fn parse_whois_country(response: &str) -> Option<String> {
    for line in response.lines() {
        let line_lower = line.to_lowercase();
        if line_lower.starts_with("country:") {
            let value = line.splitn(2, ':').nth(1)?.trim().to_uppercase();
            if value.len() == 2 && value.chars().all(|c| c.is_ascii_alphabetic()) {
                return Some(value);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_whois_org_orgname() {
        let response = "NetRange:       8.0.0.0 - 8.255.255.255\nOrgName:        Google LLC\nOrgId:          GOGL";
        assert_eq!(parse_whois_org(response), Some("Google LLC".to_string()));
    }

    #[test]
    fn test_parse_whois_org_organisation() {
        let response = "inetnum:        1.0.0.0 - 1.255.255.255\norganisation:   APNIC\ndescr:          Asia Pacific";
        assert_eq!(parse_whois_org(response), Some("APNIC".to_string()));
    }

    #[test]
    fn test_parse_whois_asn_origin() {
        let response = "route:          8.8.8.0/24\norigin:         AS15169\ndescr:          Google";
        assert_eq!(parse_whois_asn(response), Some(15169));
    }

    #[test]
    fn test_parse_whois_asn_originas() {
        let response = "NetRange:       8.0.0.0\nOriginAS:       AS15169";
        assert_eq!(parse_whois_asn(response), Some(15169));
    }

    #[test]
    fn test_parse_whois_country() {
        let response = "OrgName:        Google LLC\nCountry:        US\nCity:           Mountain View";
        assert_eq!(parse_whois_country(response), Some("US".to_string()));
    }

    #[test]
    fn test_parse_whois_country_lowercase() {
        let response = "country:        de\norg:            Deutsche Telekom";
        assert_eq!(parse_whois_country(response), Some("DE".to_string()));
    }

    #[test]
    fn test_parse_whois_missing_fields() {
        let response = "NetRange:       8.0.0.0 - 8.255.255.255\nNetName:        LVLT-ORG-8-8";
        assert_eq!(parse_whois_org(response), None);
        assert_eq!(parse_whois_asn(response), None);
        assert_eq!(parse_whois_country(response), None);
    }

    #[test]
    fn test_enrichment_result_default() {
        let result = EnrichmentResult::default();
        assert!(result.reverse_dns.is_none());
        assert!(result.whois_org.is_none());
        assert!(result.asn.is_none());
        assert!(result.country.is_none());
    }

    #[tokio::test]
    async fn test_enrichment_service_creation() {
        let service = EnrichmentService::new();
        assert!(service.is_ok());
    }

    #[tokio::test]
    async fn test_cache_starts_empty() {
        let service = EnrichmentService::new().unwrap();
        assert_eq!(service.cache_size(), 0);
    }
}
