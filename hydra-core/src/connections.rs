use crate::socks;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteType {
    Direct,
    Wss,
    Vless,
    Ssh,
    P2P,
    Blocked,
}

impl RouteType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteType::Direct => "direct",
            RouteType::Wss => "wss",
            RouteType::Vless => "vless",
            RouteType::Ssh => "ssh",
            RouteType::P2P => "p2p",
            RouteType::Blocked => "blocked",
        }
    }

    fn is_proxied(&self) -> bool {
        matches!(self, Self::Wss | Self::Vless | Self::Ssh | Self::P2P)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnStatus {
    Active,
    Closed,
}

impl ConnStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnStatus::Active => "active",
            ConnStatus::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionGroupKind {
    App,
    Domain,
}

impl ConnectionGroupKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Domain => "domain",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoutePolicyAction {
    Auto,
    Direct,
    Block,
    Wss { profile_id: String },
    Vless { profile_id: String },
}

impl Default for RoutePolicyAction {
    fn default() -> Self {
        Self::Auto
    }
}

impl RoutePolicyAction {
    pub fn as_label(&self) -> String {
        match self {
            Self::Auto => "Auto".to_string(),
            Self::Direct => "Direct".to_string(),
            Self::Block => "Block".to_string(),
            Self::Wss { profile_id } => format!("WSS ({profile_id})"),
            Self::Vless { profile_id } => format!("VLESS ({profile_id})"),
        }
    }

    pub fn profile_id(&self) -> Option<&str> {
        match self {
            Self::Wss { profile_id } | Self::Vless { profile_id } => Some(profile_id.as_str()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoutePolicyEntry {
    pub group_kind: ConnectionGroupKind,
    pub group_key: String,
    pub action: RoutePolicyAction,
    pub updated_at: u64,
}

#[derive(Debug, Clone)]
pub struct ConnectionGroup {
    pub group_kind: ConnectionGroupKind,
    pub group_key: String,
    pub app_label: Option<String>,
    pub package_name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedRoutePolicy {
    pub group: ConnectionGroup,
    pub action: RoutePolicyAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppAttribution {
    pub uid: u32,
    pub package_name: String,
    pub app_label: String,
}

impl AppAttribution {
    pub fn unknown() -> Self {
        Self {
            uid: 0,
            package_name: String::new(),
            app_label: String::new(),
        }
    }

    pub fn is_valid(&self) -> bool {
        self.uid > 0 && !self.package_name.is_empty()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionClassification {
    pub category: TrafficCategory,
    pub confidence: f32,
    pub source: ClassificationSource,
    pub explanation: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrafficCategory {
    Legitimate,
    Advertising,
    Analytics,
    Telemetry,
    SocialTracking,
    Malware,
    Unknown,
}

impl TrafficCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Legitimate => "legitimate",
            Self::Advertising => "advertising",
            Self::Analytics => "analytics",
            Self::Telemetry => "telemetry",
            Self::SocialTracking => "social_tracking",
            Self::Malware => "malware",
            Self::Unknown => "unknown",
        }
    }

    pub fn is_tracker(&self) -> bool {
        matches!(
            self,
            Self::Advertising | Self::Analytics | Self::Telemetry | Self::SocialTracking
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ClassificationSource {
    TrackerDb,
    RuleCache,
    LlmAnalysis,
    UserOverride,
}

impl ClassificationSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TrackerDb => "tracker_db",
            Self::RuleCache => "rule_cache",
            Self::LlmAnalysis => "llm_analysis",
            Self::UserOverride => "user_override",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: u64,
    pub target_host: String,
    pub target_port: u16,
    pub route_type: RouteType,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub start_time: Instant,
    pub is_proxied: bool,
    pub force_proxy: Option<bool>,
    pub status: ConnStatus,
    pub ai_reason: Option<String>,
    pub group_kind: ConnectionGroupKind,
    pub group_key: String,
    pub app_label: Option<String>,
    pub package_name: Option<String>,
    pub app_uid: Option<u32>,
    pub reverse_dns: Option<String>,
    pub whois_org: Option<String>,
    pub whois_asn: Option<u32>,
    pub whois_country: Option<String>,
    pub classification: Option<ConnectionClassification>,
    pub resolved_policy: RoutePolicyAction,
    pub transport_label: Option<String>,
    pub src_ip: Option<std::net::IpAddr>,
    pub src_port: Option<u16>,
    pub src_protocol: Option<String>,
    pub dst_ip: Option<std::net::IpAddr>,
}

#[derive(Debug, Clone)]
pub struct ConnectionSnapshot {
    pub id: u64,
    pub target_host: String,
    pub target_port: u16,
    pub route_type: String,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub duration_ms: u64,
    pub is_proxied: bool,
    pub status: String,
    pub ai_reason: Option<String>,
    pub group_kind: String,
    pub group_key: String,
    pub app_label: Option<String>,
    pub package_name: Option<String>,
    pub app_uid: Option<u32>,
    pub reverse_dns: Option<String>,
    pub whois_org: Option<String>,
    pub whois_asn: Option<u32>,
    pub whois_country: Option<String>,
    pub classification_category: Option<String>,
    pub classification_confidence: Option<f32>,
    pub classification_source: Option<String>,
    pub classification_explanation: Option<String>,
    pub resolved_policy: String,
    pub transport_label: Option<String>,
    pub src_ip: Option<String>,
    pub src_port: Option<u16>,
    pub src_protocol: Option<String>,
    pub dst_ip: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub active_count: usize,
    pub total_count: usize,
    pub proxied_count: usize,
    pub blocked_count: usize,
    pub tracker_count: usize,
    pub total_bytes_up: u64,
    pub total_bytes_down: u64,
}

#[derive(Debug, Clone)]
pub struct ConnectionRegistry {
    inner: Arc<RwLock<HashMap<u64, ConnectionInfo>>>,
    policies: Arc<RwLock<HashMap<String, RoutePolicyEntry>>>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            policies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(
        &self,
        target: &str,
        group: ConnectionGroup,
        resolved_policy: RoutePolicyAction,
        app_attribution: Option<&AppAttribution>,
        source_info: Option<&socks::SourceInfo>,
    ) -> u64 {
        let id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        let (host, port) = socks::split_target(target).unwrap_or((target, 0));

        let (app_uid, app_label, package_name) = if let Some(attr) = app_attribution {
            (
                Some(attr.uid),
                Some(attr.app_label.clone()),
                Some(attr.package_name.clone()),
            )
        } else {
            (None, group.app_label.clone(), group.package_name.clone())
        };

        let (src_ip, src_port, src_protocol, dst_ip) = if let Some(src) = source_info {
            (
                src.src_ip,
                src.src_port,
                if src.protocol.is_empty() {
                    None
                } else {
                    Some(src.protocol.clone())
                },
                src.dst_ip,
            )
        } else {
            (None, None, None, None)
        };

        let info = ConnectionInfo {
            id,
            target_host: host.to_string(),
            target_port: port,
            route_type: RouteType::Direct,
            bytes_up: 0,
            bytes_down: 0,
            start_time: Instant::now(),
            is_proxied: false,
            force_proxy: None,
            status: ConnStatus::Active,
            ai_reason: None,
            group_kind: group.group_kind,
            group_key: group.group_key,
            app_label,
            package_name,
            app_uid,
            reverse_dns: None,
            whois_org: None,
            whois_asn: None,
            whois_country: None,
            classification: None,
            resolved_policy,
            transport_label: None,
            src_ip,
            src_port,
            src_protocol,
            dst_ip,
        };

        self.inner.write().unwrap().insert(id, info);
        id
    }

    pub fn update_app_attribution(&self, id: u64, attribution: &AppAttribution) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.app_uid = Some(attribution.uid);
            conn.app_label = Some(attribution.app_label.clone());
            conn.package_name = Some(attribution.package_name.clone());
            if conn.group_kind == ConnectionGroupKind::Domain {
                conn.group_kind = ConnectionGroupKind::App;
                conn.group_key = attribution.package_name.clone();
            }
        }
    }

    pub fn update_enrichment(
        &self,
        id: u64,
        reverse_dns: Option<String>,
        whois_org: Option<String>,
        whois_asn: Option<u32>,
        whois_country: Option<String>,
    ) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.reverse_dns = reverse_dns;
            conn.whois_org = whois_org;
            conn.whois_asn = whois_asn;
            conn.whois_country = whois_country;
        }
    }

    pub fn update_classification(&self, id: u64, classification: ConnectionClassification) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.classification = Some(classification);
        }
    }

    pub fn update_route(
        &self,
        id: u64,
        route_type: RouteType,
        ai_reason: Option<String>,
        transport_label: Option<String>,
    ) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.route_type = route_type;
            conn.is_proxied = route_type.is_proxied();
            if ai_reason.is_some() {
                conn.ai_reason = ai_reason;
            }
            if transport_label.is_some() {
                conn.transport_label = transport_label;
            }
        }
    }

    pub fn update_bytes(&self, id: u64, bytes_up: u64, bytes_down: u64) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.bytes_up += bytes_up;
            conn.bytes_down += bytes_down;
        }
    }

    pub fn close(&self, id: u64) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.status = ConnStatus::Closed;
        }
    }

    pub fn set_force_proxy(&self, id: u64, force: bool) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.force_proxy = Some(force);
        }
    }

    pub fn get_force_proxy(&self, id: u64) -> Option<bool> {
        self.inner
            .read()
            .unwrap()
            .get(&id)
            .and_then(|c| c.force_proxy)
    }

    pub fn should_proxy(&self, target: &str) -> bool {
        socks::is_telegram_target(target)
    }

    pub fn list_policies(&self) -> Vec<RoutePolicyEntry> {
        let mut values = self
            .policies
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect::<Vec<_>>();
        values.sort_by(|a, b| {
            a.group_kind
                .as_str()
                .cmp(b.group_kind.as_str())
                .then_with(|| a.group_key.cmp(&b.group_key))
        });
        values
    }

    pub fn replace_policies(&self, policies: Vec<RoutePolicyEntry>) {
        let mut guard = self.policies.write().unwrap();
        guard.clear();
        for policy in policies {
            guard.insert(policy_key(policy.group_kind, &policy.group_key), policy);
        }
    }

    pub fn upsert_policy(
        &self,
        group_kind: ConnectionGroupKind,
        group_key: String,
        action: RoutePolicyAction,
    ) -> RoutePolicyEntry {
        let entry = RoutePolicyEntry {
            group_kind,
            group_key: group_key.clone(),
            action,
            updated_at: now_epoch_secs(),
        };
        self.policies
            .write()
            .unwrap()
            .insert(policy_key(group_kind, &group_key), entry.clone());
        entry
    }

    pub fn clear_policy(&self, group_kind: ConnectionGroupKind, group_key: &str) -> bool {
        self.policies
            .write()
            .unwrap()
            .remove(&policy_key(group_kind, group_key))
            .is_some()
    }

    pub fn resolve_policy(
        &self,
        target: &str,
        app_attribution: Option<&AppAttribution>,
    ) -> ResolvedRoutePolicy {
        let preferred_group = best_effort_group_for_target(target, app_attribution);
        let fallback_domain_group = domain_group_for_target(target);
        let guard = self.policies.read().unwrap();

        if preferred_group.group_kind == ConnectionGroupKind::App {
            if let Some(policy) = guard.get(&policy_key(
                preferred_group.group_kind,
                &preferred_group.group_key,
            )) {
                return ResolvedRoutePolicy {
                    group: preferred_group,
                    action: policy.action.clone(),
                };
            }
        }

        if let Some(policy) = guard.get(&policy_key(
            fallback_domain_group.group_kind,
            &fallback_domain_group.group_key,
        )) {
            return ResolvedRoutePolicy {
                group: preferred_group,
                action: policy.action.clone(),
            };
        }

        ResolvedRoutePolicy {
            group: preferred_group,
            action: RoutePolicyAction::Auto,
        }
    }

    pub fn snapshot(&self, active_only: bool) -> Vec<ConnectionSnapshot> {
        let guard = self.inner.read().unwrap();
        let now = Instant::now();
        guard
            .values()
            .filter(|c| !active_only || c.status == ConnStatus::Active)
            .map(|c| ConnectionSnapshot {
                id: c.id,
                target_host: c.target_host.clone(),
                target_port: c.target_port,
                route_type: c.route_type.as_str().to_string(),
                bytes_up: c.bytes_up,
                bytes_down: c.bytes_down,
                duration_ms: now.duration_since(c.start_time).as_millis() as u64,
                is_proxied: c.is_proxied,
                status: c.status.as_str().to_string(),
                ai_reason: c.ai_reason.clone(),
                group_kind: c.group_kind.as_str().to_string(),
                group_key: c.group_key.clone(),
                app_label: c.app_label.clone(),
                package_name: c.package_name.clone(),
                app_uid: c.app_uid,
                reverse_dns: c.reverse_dns.clone(),
                whois_org: c.whois_org.clone(),
                whois_asn: c.whois_asn,
                whois_country: c.whois_country.clone(),
                classification_category: c
                    .classification
                    .as_ref()
                    .map(|classification| classification.category.as_str().to_string()),
                classification_confidence: c
                    .classification
                    .as_ref()
                    .map(|classification| classification.confidence),
                classification_source: c
                    .classification
                    .as_ref()
                    .map(|classification| classification.source.as_str().to_string()),
                classification_explanation: c
                    .classification
                    .as_ref()
                    .and_then(|classification| classification.explanation.clone()),
                resolved_policy: c.resolved_policy.as_label(),
                transport_label: c.transport_label.clone(),
                src_ip: c.src_ip.map(|ip| ip.to_string()),
                src_port: c.src_port,
                src_protocol: c.src_protocol.clone(),
                dst_ip: c.dst_ip.map(|ip| ip.to_string()),
            })
            .collect()
    }

    pub fn stats(&self) -> ConnectionStats {
        let guard = self.inner.read().unwrap();
        let mut stats = ConnectionStats {
            active_count: 0,
            total_count: guard.len(),
            proxied_count: 0,
            blocked_count: 0,
            tracker_count: 0,
            total_bytes_up: 0,
            total_bytes_down: 0,
        };
        for c in guard.values() {
            if c.status == ConnStatus::Active {
                stats.active_count += 1;
            }
            if c.is_proxied {
                stats.proxied_count += 1;
            }
            if c.route_type == RouteType::Blocked {
                stats.blocked_count += 1;
            }
            if c.classification
                .as_ref()
                .is_some_and(|classification| classification.category.is_tracker())
            {
                stats.tracker_count += 1;
            }
            stats.total_bytes_up += c.bytes_up;
            stats.total_bytes_down += c.bytes_down;
        }
        stats
    }

    pub fn gc(&self, max_closed: usize) {
        let mut guard = self.inner.write().unwrap();
        let mut closed: Vec<(u64, Instant)> = guard
            .iter()
            .filter(|(_, c)| c.status == ConnStatus::Closed)
            .map(|(id, c)| (*id, c.start_time))
            .collect();

        if closed.len() > max_closed {
            closed.sort_by_key(|(_, t)| *t);
            let to_remove = closed.len() - max_closed;
            for (id, _) in closed.into_iter().take(to_remove) {
                guard.remove(&id);
            }
        }
    }
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn policy_key(group_kind: ConnectionGroupKind, group_key: &str) -> String {
    format!("{}:{}", group_kind.as_str(), group_key)
}

fn best_effort_group_for_target(
    target: &str,
    app_attribution: Option<&AppAttribution>,
) -> ConnectionGroup {
    if let Some(app_attribution) = app_attribution.filter(|attr| attr.is_valid()) {
        return ConnectionGroup {
            group_kind: ConnectionGroupKind::App,
            group_key: app_attribution.package_name.clone(),
            app_label: Some(app_attribution.app_label.clone()),
            package_name: Some(app_attribution.package_name.clone()),
        };
    }

    domain_group_for_target(target)
}

fn domain_group_for_target(target: &str) -> ConnectionGroup {
    let (host, _) = socks::split_target(target).unwrap_or((target, 0));
    let key = extract_domain_group(host);
    ConnectionGroup {
        group_kind: ConnectionGroupKind::Domain,
        group_key: key,
        app_label: None,
        package_name: None,
    }
}

fn extract_domain_group(host: &str) -> String {
    let host = host
        .trim_matches(|ch| ch == '[' || ch == ']')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.parse::<std::net::IpAddr>().is_ok() {
        return host;
    }

    let parts = host.split('.').collect::<Vec<_>>();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        host
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_snapshot_domain_group() {
        let reg = ConnectionRegistry::new();
        let resolved = reg.resolve_policy("149.154.167.50:443", None);
        let id = reg.register(
            "149.154.167.50:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );
        assert!(id > 0);

        let snaps = reg.snapshot(true);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].target_host, "149.154.167.50");
        assert_eq!(snaps[0].target_port, 443);
        assert_eq!(snaps[0].status, "active");
        assert_eq!(snaps[0].group_kind, "domain");
    }

    #[test]
    fn test_register_with_app_attribution() {
        let reg = ConnectionRegistry::new();
        let attr = AppAttribution {
            uid: 10001,
            package_name: "com.example.app".to_string(),
            app_label: "Example App".to_string(),
        };
        let resolved = reg.resolve_policy("example.com:443", Some(&attr));
        let attr = AppAttribution {
            uid: 10001,
            package_name: "com.example.app".to_string(),
            app_label: "Example App".to_string(),
        };
        reg.register(
            "example.com:443",
            resolved.group,
            resolved.action,
            Some(&attr),
            None,
        );

        let snaps = reg.snapshot(true);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].app_uid, Some(10001));
        assert_eq!(snaps[0].package_name.as_deref(), Some("com.example.app"));
        assert_eq!(snaps[0].app_label.as_deref(), Some("Example App"));
        assert_eq!(snaps[0].group_kind, "app");
    }

    #[test]
    fn test_update_app_attribution() {
        let reg = ConnectionRegistry::new();
        let resolved = reg.resolve_policy("example.com:443", None);
        let id = reg.register(
            "example.com:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        let snaps = reg.snapshot(true);
        assert_eq!(snaps[0].app_uid, None);
        assert_eq!(snaps[0].group_kind, "domain");

        let attr = AppAttribution {
            uid: 10002,
            package_name: "com.test.app".to_string(),
            app_label: "Test App".to_string(),
        };
        reg.update_app_attribution(id, &attr);

        let snaps = reg.snapshot(true);
        assert_eq!(snaps[0].app_uid, Some(10002));
        assert_eq!(snaps[0].group_kind, "app");
        assert_eq!(snaps[0].group_key, "com.test.app");
    }

    #[test]
    fn test_update_route_and_bytes() {
        let reg = ConnectionRegistry::new();
        let resolved = reg.resolve_policy("example.com:80", None);
        let id = reg.register(
            "example.com:80",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        reg.update_route(
            id,
            RouteType::Wss,
            Some("Using built-in relay".to_string()),
            Some("Hydra WSS Relay".to_string()),
        );
        reg.update_bytes(id, 1000, 2000);
        reg.update_bytes(id, 500, 300);

        let snaps = reg.snapshot(false);
        let conn = snaps.iter().find(|s| s.id == id).unwrap();
        assert_eq!(conn.route_type, "wss");
        assert_eq!(conn.bytes_up, 1500);
        assert_eq!(conn.bytes_down, 2300);
        assert!(conn.is_proxied);
        assert_eq!(conn.ai_reason.as_deref(), Some("Using built-in relay"));
        assert_eq!(conn.transport_label.as_deref(), Some("Hydra WSS Relay"));
    }

    #[test]
    fn test_close_connection() {
        let reg = ConnectionRegistry::new();
        let resolved = reg.resolve_policy("1.2.3.4:80", None);
        let id = reg.register("1.2.3.4:80", resolved.group, resolved.action, None, None);

        assert_eq!(reg.snapshot(true).len(), 1);
        reg.close(id);
        assert_eq!(reg.snapshot(true).len(), 0);
        assert_eq!(reg.snapshot(false).len(), 1);
    }

    #[test]
    fn test_telegram_detection() {
        let reg = ConnectionRegistry::new();
        assert!(reg.should_proxy("149.154.167.50:443"));
        assert!(reg.should_proxy("91.108.56.100:443"));
        assert!(!reg.should_proxy("8.8.8.8:53"));
    }

    #[test]
    fn test_force_proxy() {
        let reg = ConnectionRegistry::new();
        let resolved = reg.resolve_policy("example.com:443", None);
        let id = reg.register(
            "example.com:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        assert_eq!(reg.get_force_proxy(id), None);
        reg.set_force_proxy(id, true);
        assert_eq!(reg.get_force_proxy(id), Some(true));
    }

    #[test]
    fn test_stats() {
        let reg = ConnectionRegistry::new();
        let id1 = reg.register(
            "149.154.167.50:443",
            reg.resolve_policy("149.154.167.50:443", None).group,
            RoutePolicyAction::Auto,
            None,
            None,
        );
        let id2 = reg.register(
            "google.com:443",
            reg.resolve_policy("google.com:443", None).group,
            RoutePolicyAction::Auto,
            None,
            None,
        );

        reg.update_route(
            id1,
            RouteType::Wss,
            None,
            Some("Hydra WSS Relay".to_string()),
        );
        reg.update_route(id2, RouteType::Blocked, None, Some("Blocked".to_string()));
        reg.update_classification(
            id1,
            ConnectionClassification {
                category: TrafficCategory::Analytics,
                confidence: 0.95,
                source: ClassificationSource::TrackerDb,
                explanation: Some("Matched tracker list".to_string()),
            },
        );
        reg.update_bytes(id1, 100, 200);
        reg.update_bytes(id2, 50, 75);
        reg.close(id2);

        let stats = reg.stats();
        assert_eq!(stats.active_count, 1);
        assert_eq!(stats.total_count, 2);
        assert_eq!(stats.proxied_count, 1);
        assert_eq!(stats.blocked_count, 1);
        assert_eq!(stats.tracker_count, 1);
        assert_eq!(stats.total_bytes_up, 150);
        assert_eq!(stats.total_bytes_down, 275);
    }

    #[test]
    fn test_gc_removes_oldest_closed() {
        let reg = ConnectionRegistry::new();
        let id1 = reg.register(
            "a.com:80",
            reg.resolve_policy("a.com:80", None).group,
            RoutePolicyAction::Auto,
            None,
            None,
        );
        let id2 = reg.register(
            "b.com:80",
            reg.resolve_policy("b.com:80", None).group,
            RoutePolicyAction::Auto,
            None,
            None,
        );
        let id3 = reg.register(
            "c.com:80",
            reg.resolve_policy("c.com:80", None).group,
            RoutePolicyAction::Auto,
            None,
            None,
        );

        reg.close(id1);
        reg.close(id2);
        reg.close(id3);

        reg.gc(1);
        let snaps = reg.snapshot(false);
        assert_eq!(snaps.len(), 1);
    }

    #[test]
    fn test_unique_ids() {
        let reg = ConnectionRegistry::new();
        let id1 = reg.register(
            "a.com:80",
            reg.resolve_policy("a.com:80", None).group,
            RoutePolicyAction::Auto,
            None,
            None,
        );
        let id2 = reg.register(
            "b.com:80",
            reg.resolve_policy("b.com:80", None).group,
            RoutePolicyAction::Auto,
            None,
            None,
        );
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_domain_policy_resolution() {
        let reg = ConnectionRegistry::new();
        reg.upsert_policy(
            ConnectionGroupKind::Domain,
            "example.com".to_string(),
            RoutePolicyAction::Block,
        );

        let resolved = reg.resolve_policy("api.example.com:443", None);
        assert_eq!(resolved.group.group_kind, ConnectionGroupKind::Domain);
        assert_eq!(resolved.group.group_key, "example.com");
        assert_eq!(resolved.action, RoutePolicyAction::Block);
    }

    #[test]
    fn test_update_enrichment_and_classification_snapshot() {
        let reg = ConnectionRegistry::new();
        let resolved = reg.resolve_policy("api.example.com:443", None);
        let id = reg.register(
            "api.example.com:443",
            resolved.group,
            resolved.action,
            None,
            None,
        );

        reg.update_enrichment(
            id,
            Some("edge.example.com".to_string()),
            Some("Example Org".to_string()),
            Some(64512),
            Some("US".to_string()),
        );
        reg.update_classification(
            id,
            ConnectionClassification {
                category: TrafficCategory::Telemetry,
                confidence: 0.88,
                source: ClassificationSource::LlmAnalysis,
                explanation: Some("High-volume background telemetry".to_string()),
            },
        );

        let snaps = reg.snapshot(true);
        let snap = snaps.iter().find(|snap| snap.id == id).unwrap();
        assert_eq!(snap.reverse_dns.as_deref(), Some("edge.example.com"));
        assert_eq!(snap.whois_org.as_deref(), Some("Example Org"));
        assert_eq!(snap.whois_asn, Some(64512));
        assert_eq!(snap.whois_country.as_deref(), Some("US"));
        assert_eq!(snap.classification_category.as_deref(), Some("telemetry"));
        assert_eq!(snap.classification_confidence, Some(0.88));
        assert_eq!(snap.classification_source.as_deref(), Some("llm_analysis"));
        assert_eq!(
            snap.classification_explanation.as_deref(),
            Some("High-volume background telemetry")
        );
    }
}
