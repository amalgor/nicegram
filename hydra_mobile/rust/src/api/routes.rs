use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use base64::Engine;
use hydra_config::{HydraConfig, TransportConfig, TransportMode};
use hydra_core::connections::{
    ConnectionGroupKind, ConnectionRegistry, RoutePolicyAction, RoutePolicyEntry,
};
use hydra_core::transport::{
    ConfiguredTransport, RouteProfile, RouteProfileKind, RouteProfileSource, TransportKind,
    build_transports_from_profiles,
};
use hydra_core::UsageRecorder;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MOBILE_ROUTES_FILE: &str = "mobile_routes.json";
const ROUTE_POLICIES_FILE: &str = "route_policies.json";
const RELAY_USAGE_FILE: &str = "relay_usage.json";
lazy_static::lazy_static! {
    static ref ROUTE_RUNTIME: Mutex<Option<RouteRuntimeState>> = Mutex::new(None);
}

struct RouteRuntimeState {
    base_dir: PathBuf,
    profiles: Vec<RouteProfile>,
    registry: Option<Arc<ConnectionRegistry>>,
    transports_handle: Option<Arc<RwLock<Vec<ConfiguredTransport>>>>,
    relay_usage: Arc<RelayUsageStore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayUsageSample {
    pub bucket_start: i64,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayUsageSummary {
    pub today_bytes: u64,
    pub last_7d_bytes: u64,
    pub last_30d_bytes: u64,
    pub sample_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ImportReport {
    imported: Vec<RouteProfile>,
    skipped: Vec<String>,
    total_candidates: usize,
}

#[derive(Debug)]
pub struct RelayUsageStore {
    path: PathBuf,
    inner: Mutex<Vec<RelayUsageSample>>,
}

impl RelayUsageStore {
    fn load(base_dir: &Path) -> Result<Self> {
        let path = base_dir.join(RELAY_USAGE_FILE);
        let samples = if path.exists() {
            let raw = std::fs::read(&path)?;
            serde_json::from_slice(&raw).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self {
            path,
            inner: Mutex::new(samples),
        })
    }

    fn record_wss_usage(&self, bytes: u64) -> Result<()> {
        if bytes == 0 {
            return Ok(());
        }
        let bucket_start = current_hour_bucket();
        let mut guard = self
            .inner
            .lock()
            .map_err(|e| anyhow!("relay usage lock poisoned: {e}"))?;
        if let Some(sample) = guard.iter_mut().find(|sample| sample.bucket_start == bucket_start) {
            sample.bytes = sample.bytes.saturating_add(bytes);
        } else {
            guard.push(RelayUsageSample {
                bucket_start,
                bytes,
            });
        }
        guard.retain(|sample| sample.bucket_start >= current_hour_bucket() - (35 * 24 * 3600));
        guard.sort_by_key(|sample| sample.bucket_start);
        persist_json(&self.path, &*guard)?;
        Ok(())
    }

    fn summary(&self) -> Result<RelayUsageSummary> {
        let guard = self
            .inner
            .lock()
            .map_err(|e| anyhow!("relay usage lock poisoned: {e}"))?;
        let now = now_epoch_secs() as i64;
        let today_start = start_of_day(now);
        let week_start = today_start - (6 * 24 * 3600);
        let month_start = today_start - (29 * 24 * 3600);

        let mut summary = RelayUsageSummary {
            today_bytes: 0,
            last_7d_bytes: 0,
            last_30d_bytes: 0,
            sample_count: guard.len(),
        };
        for sample in guard.iter() {
            if sample.bucket_start >= today_start {
                summary.today_bytes = summary.today_bytes.saturating_add(sample.bytes);
            }
            if sample.bucket_start >= week_start {
                summary.last_7d_bytes = summary.last_7d_bytes.saturating_add(sample.bytes);
            }
            if sample.bucket_start >= month_start {
                summary.last_30d_bytes = summary.last_30d_bytes.saturating_add(sample.bytes);
            }
        }
        Ok(summary)
    }

    fn history(&self, days: u32) -> Result<Vec<RelayUsageSample>> {
        let guard = self
            .inner
            .lock()
            .map_err(|e| anyhow!("relay usage lock poisoned: {e}"))?;
        let cutoff = current_hour_bucket() - (days as i64 * 24 * 3600);
        Ok(guard
            .iter()
            .filter(|sample| sample.bucket_start >= cutoff)
            .cloned()
            .collect())
    }
}

#[async_trait]
impl UsageRecorder for RelayUsageStore {
    async fn record_usage(
        &self,
        transport: &ConfiguredTransport,
        bytes_total: u64,
        _duration: Duration,
    ) -> Result<()> {
        if transport.kind != TransportKind::Wss {
            return Ok(());
        }

        self.record_wss_usage(bytes_total)?;
        crate::api::quota::record_bytes(bytes_total);
        Ok(())
    }
}

pub(crate) fn bootstrap(base_dir: &Path, config: &HydraConfig) -> Result<Arc<RelayUsageStore>> {
    let profiles = load_or_seed_profiles(base_dir, config)?;
    let relay_usage = Arc::new(RelayUsageStore::load(base_dir)?);

    let mut guard = ROUTE_RUNTIME
        .lock()
        .map_err(|e| anyhow!("route runtime lock poisoned: {e}"))?;
    *guard = Some(RouteRuntimeState {
        base_dir: base_dir.to_path_buf(),
        profiles,
        registry: None,
        transports_handle: None,
        relay_usage: relay_usage.clone(),
    });
    Ok(relay_usage)
}

pub(crate) fn attach_runtime_handles(
    registry: Arc<ConnectionRegistry>,
    transports_handle: Arc<RwLock<Vec<ConfiguredTransport>>>,
) -> Result<()> {
    let mut guard = ROUTE_RUNTIME
        .lock()
        .map_err(|e| anyhow!("route runtime lock poisoned: {e}"))?;
    let state = guard
        .as_mut()
        .ok_or_else(|| anyhow!("route runtime not bootstrapped"))?;
    let policies = load_policies(&state.base_dir)?;
    registry.replace_policies(policies);
    state.registry = Some(registry);
    state.transports_handle = Some(transports_handle);
    sync_transports_locked(state)
}

pub(crate) fn current_profiles() -> Result<Vec<RouteProfile>> {
    with_state(|state| Ok(state.profiles.clone()))
}

pub(crate) fn reload_from_disk() -> Result<()> {
    with_state_mut(|state| {
        let profiles_path = state.base_dir.join(MOBILE_ROUTES_FILE);
        if profiles_path.exists() {
            let raw = std::fs::read(&profiles_path)?;
            let profiles = serde_json::from_slice::<Vec<RouteProfile>>(&raw)?;
            state.profiles = normalize_profiles(profiles)?;
            sync_transports_locked(state)?;
        }

        if let Some(registry) = &state.registry {
            let policies = load_policies(&state.base_dir)?;
            registry.replace_policies(policies);
        }

        Ok(())
    })
}

pub async fn list_route_profiles() -> Result<String> {
    let profiles = with_state(|state| Ok(state.profiles.clone()))?;
    to_json(&profiles)
}

pub async fn import_route_profiles(payload: String, import_format: String) -> Result<String> {
    let format = import_format.trim().to_ascii_lowercase();
    let source = match format.as_str() {
        "raw" => RouteProfileSource::ImportedRaw,
        "subscription" => RouteProfileSource::ImportedSubscription,
        _ => bail!("Unsupported import format: {format}"),
    };

    let candidates = if format == "subscription" {
        parse_subscription_payload(&payload)?
    } else {
        parse_raw_payload(&payload)
    };

    let total_candidates = candidates.len();
    let report = with_state_mut(|state| {
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        let mut next_priority = next_priority(&state.profiles);

        for (index, candidate) in candidates.iter().enumerate() {
            if !candidate.starts_with("vless://") {
                skipped.push(format!("Skipped unsupported entry: {candidate}"));
                continue;
            }

            if state.profiles.iter().any(|profile| profile_url(profile) == Some(candidate.as_str())) {
                skipped.push(format!("Skipped duplicate VLESS entry: {candidate}"));
                continue;
            }

            match build_imported_vless_profile(candidate, source, next_priority, index) {
                Ok(profile) => {
                    next_priority = next_priority.saturating_add(1);
                    state.profiles.push(profile.clone());
                    imported.push(profile);
                }
                Err(error) => skipped.push(format!("Skipped invalid VLESS entry: {error}")),
            }
        }

        state.profiles.sort_by_key(|profile| profile.priority);
        persist_profiles_locked(state)?;
        sync_transports_locked(state)?;

        Ok(ImportReport {
            imported,
            skipped,
            total_candidates,
        })
    })?;

    to_json(&report)
}

pub async fn update_route_profile(profile_json: String) -> Result<String> {
    let updated: RouteProfile = serde_json::from_str(&profile_json)
        .with_context(|| "Failed to parse route profile payload")?;
    let normalized = normalize_profile(updated)?;

    let profile = with_state_mut(|state| {
        let Some(index) = state.profiles.iter().position(|profile| profile.id == normalized.id) else {
            bail!("Unknown route profile");
        };
        if matches!(state.profiles[index].source, RouteProfileSource::Builtin)
            && !matches!(normalized.source, RouteProfileSource::Builtin)
        {
            bail!("Built-in profile source cannot change");
        }
        state.profiles[index] = normalized.clone();
        let updated = state.profiles[index].clone();
        state.profiles.sort_by_key(|profile| profile.priority);
        persist_profiles_locked(state)?;
        sync_transports_locked(state)?;
        Ok(updated)
    })?;

    to_json(&profile)
}

pub async fn delete_route_profile(profile_id: String) -> Result<()> {
    with_state_mut(|state| {
        let Some(index) = state.profiles.iter().position(|profile| profile.id == profile_id) else {
            bail!("Unknown route profile");
        };
        if matches!(state.profiles[index].source, RouteProfileSource::Builtin) {
            bail!("Built-in route profiles cannot be deleted");
        }
        state.profiles.remove(index);
        renumber_priorities(&mut state.profiles);
        persist_profiles_locked(state)?;
        sync_transports_locked(state)?;
        Ok(())
    })
}

pub async fn reorder_route_profiles(profile_ids_json: String) -> Result<String> {
    let requested: Vec<String> = serde_json::from_str(&profile_ids_json)
        .with_context(|| "Failed to parse route profile ordering payload")?;
    let profiles = with_state_mut(|state| {
        let mut ordered = Vec::with_capacity(state.profiles.len());
        for id in requested {
            if let Some(position) = state.profiles.iter().position(|profile| profile.id == id) {
                ordered.push(state.profiles.remove(position));
            }
        }
        ordered.append(&mut state.profiles);
        renumber_priorities(&mut ordered);
        state.profiles = ordered;
        persist_profiles_locked(state)?;
        sync_transports_locked(state)?;
        Ok(state.profiles.clone())
    })?;
    to_json(&profiles)
}

pub async fn list_route_policies() -> Result<String> {
    let policies = with_state(|state| {
        Ok(state
            .registry
            .as_ref()
            .map(|registry| registry.list_policies())
            .unwrap_or_default())
    })?;
    to_json(&policies)
}

pub async fn set_route_policy(
    group_kind: String,
    group_key: String,
    action: String,
    profile_id: String,
) -> Result<String> {
    let parsed_group_kind = parse_group_kind(&group_kind)?;
    let parsed_action = parse_policy_action(&action, &profile_id)?;
    let policy = with_state_mut(|state| {
        let registry = state
            .registry
            .as_ref()
            .ok_or_else(|| anyhow!("route runtime not attached"))?;
        let policy = registry.upsert_policy(parsed_group_kind, group_key.clone(), parsed_action);
        persist_json(&state.base_dir.join(ROUTE_POLICIES_FILE), &registry.list_policies())?;
        Ok(policy)
    })?;
    to_json(&policy)
}

pub async fn clear_route_policy(group_kind: String, group_key: String) -> Result<()> {
    let parsed_group_kind = parse_group_kind(&group_kind)?;
    with_state_mut(|state| {
        let registry = state
            .registry
            .as_ref()
            .ok_or_else(|| anyhow!("route runtime not attached"))?;
        registry.clear_policy(parsed_group_kind, &group_key);
        persist_json(&state.base_dir.join(ROUTE_POLICIES_FILE), &registry.list_policies())?;
        Ok(())
    })
}

pub async fn get_relay_usage_summary() -> Result<String> {
    let summary = with_state(|state| state.relay_usage.summary())?;
    to_json(&summary)
}

pub async fn get_relay_usage_history(days: u32) -> Result<String> {
    let history = with_state(|state| state.relay_usage.history(days.max(1)))?;
    to_json(&history)
}

fn with_state<T>(f: impl FnOnce(&RouteRuntimeState) -> Result<T>) -> Result<T> {
    let guard = ROUTE_RUNTIME
        .lock()
        .map_err(|e| anyhow!("route runtime lock poisoned: {e}"))?;
    let state = guard
        .as_ref()
        .ok_or_else(|| anyhow!("route runtime not bootstrapped"))?;
    f(state)
}

fn with_state_mut<T>(f: impl FnOnce(&mut RouteRuntimeState) -> Result<T>) -> Result<T> {
    let mut guard = ROUTE_RUNTIME
        .lock()
        .map_err(|e| anyhow!("route runtime lock poisoned: {e}"))?;
    let state = guard
        .as_mut()
        .ok_or_else(|| anyhow!("route runtime not bootstrapped"))?;
    f(state)
}

fn to_json<T: Serialize>(value: &T) -> Result<String> {
    Ok(serde_json::to_string(value)?)
}

fn load_or_seed_profiles(base_dir: &Path, config: &HydraConfig) -> Result<Vec<RouteProfile>> {
    let path = base_dir.join(MOBILE_ROUTES_FILE);
    if path.exists() {
        let raw = std::fs::read(&path)?;
        let profiles = serde_json::from_slice::<Vec<RouteProfile>>(&raw)?;
        return normalize_profiles(profiles);
    }

    let profiles = seed_profiles(config)?;
    persist_json(&path, &profiles)?;
    Ok(profiles)
}

fn load_policies(base_dir: &Path) -> Result<Vec<RoutePolicyEntry>> {
    let path = base_dir.join(ROUTE_POLICIES_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read(&path)?;
    Ok(serde_json::from_slice::<Vec<RoutePolicyEntry>>(&raw).unwrap_or_default())
}

fn seed_profiles(config: &HydraConfig) -> Result<Vec<RouteProfile>> {
    let mut profiles = Vec::new();
    for (index, transport) in config.transports.iter().enumerate() {
        let (kind, label) = match transport {
            TransportConfig::Wss { .. } => {
                let label = if index == 0 {
                    "Hydra WSS Relay".to_string()
                } else {
                    format!("Built-in WSS {}", index + 1)
                };
                (RouteProfileKind::Wss, label)
            }
            TransportConfig::Vless { .. } => (
                RouteProfileKind::Vless,
                format!("Built-in VLESS {}", index + 1),
            ),
        };
        profiles.push(RouteProfile::new(
            format!("builtin-{}-{index}", kind_name(kind)),
            label,
            kind,
            transport.mode(),
            true,
            index as u32,
            RouteProfileSource::Builtin,
            transport.clone(),
        ));
    }
    Ok(sort_profiles(profiles))
}

fn build_imported_vless_profile(
    url: &str,
    source: RouteProfileSource,
    priority: u32,
    index: usize,
) -> Result<RouteProfile> {
    let config = hydra_core::transport::vless::VlessConfig::try_from(url)
        .with_context(|| "Failed to parse VLESS URL")?;
    let label = format!("{}:{} ({})", config.address, config.port, config.server_name);
    let id = format!("imported-vless-{}-{index}", now_epoch_millis());
    Ok(RouteProfile::new(
        id,
        label,
        RouteProfileKind::Vless,
        TransportMode::All,
        true,
        priority,
        source,
        TransportConfig::Vless {
            url: url.to_string(),
            mode: TransportMode::All,
        },
    ))
}

fn parse_subscription_payload(payload: &str) -> Result<Vec<String>> {
    let trimmed = payload.trim();
    let decoded = decode_base64_relaxed(trimmed)?;
    let text = String::from_utf8(decoded).with_context(|| "Subscription payload is not UTF-8")?;
    Ok(parse_raw_payload(&text))
}

fn parse_raw_payload(payload: &str) -> Vec<String> {
    payload
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn decode_base64_relaxed(input: &str) -> Result<Vec<u8>> {
    let compact = input.lines().map(str::trim).collect::<String>();
    for engine in [
        &base64::engine::general_purpose::STANDARD,
        &base64::engine::general_purpose::URL_SAFE,
    ] {
        if let Ok(decoded) = engine.decode(&compact) {
            return Ok(decoded);
        }

        let mut padded = compact.clone();
        let missing = padded.len() % 4;
        if missing != 0 {
            padded.push_str(&"=".repeat(4 - missing));
        }
        if let Ok(decoded) = engine.decode(padded) {
            return Ok(decoded);
        }
    }

    bail!("Failed to decode V2Ray base64 subscription payload")
}

fn normalize_profile(mut profile: RouteProfile) -> Result<RouteProfile> {
    match (&profile.kind, &profile.config) {
        (RouteProfileKind::Wss, TransportConfig::Wss { endpoints, .. }) => {
            if endpoints.is_empty() {
                bail!("WSS profile requires at least one endpoint");
            }
            profile.config = TransportConfig::Wss {
                endpoints: endpoints.clone(),
                mode: profile.mode,
                device_id: match &profile.config {
                    TransportConfig::Wss { device_id, .. } => device_id.clone(),
                    _ => String::new(),
                },
            };
        }
        (RouteProfileKind::Vless, TransportConfig::Vless { url, .. }) => {
            hydra_core::transport::vless::VlessConfig::try_from(url.as_str())
                .with_context(|| "Invalid VLESS URL")?;
            profile.config = TransportConfig::Vless {
                url: url.clone(),
                mode: profile.mode,
            };
        }
        _ => bail!("Route profile kind does not match config payload"),
    }

    if profile.label.trim().is_empty() {
        bail!("Route profile label must not be empty");
    }

    Ok(profile)
}

fn profile_url(profile: &RouteProfile) -> Option<&str> {
    match &profile.config {
        TransportConfig::Vless { url, .. } => Some(url.as_str()),
        TransportConfig::Wss { .. } => None,
    }
}

fn sync_transports_locked(state: &mut RouteRuntimeState) -> Result<()> {
    if let Some(handle) = &state.transports_handle {
        let transports = build_transports_from_profiles(&state.profiles)?;
        *handle.write().unwrap() = transports;
    }
    Ok(())
}

fn persist_profiles_locked(state: &RouteRuntimeState) -> Result<()> {
    persist_json(&state.base_dir.join(MOBILE_ROUTES_FILE), &state.profiles)
}

fn persist_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn renumber_priorities(profiles: &mut [RouteProfile]) {
    for (index, profile) in profiles.iter_mut().enumerate() {
        profile.priority = index as u32;
    }
}

fn sort_profiles(mut profiles: Vec<RouteProfile>) -> Vec<RouteProfile> {
    profiles.sort_by_key(|profile| profile.priority);
    profiles
}

fn normalize_profiles(profiles: Vec<RouteProfile>) -> Result<Vec<RouteProfile>> {
    let mut normalized = Vec::with_capacity(profiles.len());
    for profile in profiles {
        normalized.push(normalize_profile(profile)?);
    }
    Ok(sort_profiles(normalized))
}

fn next_priority(profiles: &[RouteProfile]) -> u32 {
    profiles
        .iter()
        .map(|profile| profile.priority)
        .max()
        .unwrap_or(0)
        .saturating_add(1)
}

fn kind_name(kind: RouteProfileKind) -> &'static str {
    match kind {
        RouteProfileKind::Wss => "wss",
        RouteProfileKind::Vless => "vless",
    }
}

fn parse_group_kind(value: &str) -> Result<ConnectionGroupKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "app" => Ok(ConnectionGroupKind::App),
        "domain" => Ok(ConnectionGroupKind::Domain),
        _ => bail!("Unsupported group kind: {value}"),
    }
}

fn parse_policy_action(action: &str, profile_id: &str) -> Result<RoutePolicyAction> {
    match action.trim().to_ascii_lowercase().as_str() {
        "auto" => Ok(RoutePolicyAction::Auto),
        "direct" => Ok(RoutePolicyAction::Direct),
        "block" => Ok(RoutePolicyAction::Block),
        "wss" => {
            if profile_id.trim().is_empty() {
                bail!("WSS policy requires a profile id");
            }
            Ok(RoutePolicyAction::Wss {
                profile_id: profile_id.to_string(),
            })
        }
        "vless" => {
            if profile_id.trim().is_empty() {
                bail!("VLESS policy requires a profile id");
            }
            Ok(RoutePolicyAction::Vless {
                profile_id: profile_id.to_string(),
            })
        }
        _ => bail!("Unsupported route policy action: {action}"),
    }
}

fn current_hour_bucket() -> i64 {
    let now = now_epoch_secs() as i64;
    now - (now % 3600)
}

fn start_of_day(epoch_secs: i64) -> i64 {
    epoch_secs - (epoch_secs % 86_400)
}

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hydra_config::HydraConfig;

    fn test_config() -> HydraConfig {
        HydraConfig {
            network: hydra_config::NetworkConfig {
                proxy_mode: "full".to_string(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn seed_profiles_prefers_builtin_relay() {
        let profiles = seed_profiles(&test_config()).unwrap();
        assert_eq!(profiles.first().unwrap().label, "Hydra WSS Relay");
    }

    #[test]
    fn subscription_decode_handles_paddingless_base64() {
        let encoded = base64::engine::general_purpose::STANDARD
            .encode("vless://uuid@example.com:443?security=reality&type=tcp&sni=github.com&fp=chrome&pbk=key&sid=01");
        let decoded = parse_subscription_payload(encoded.trim_end_matches('=')).unwrap();
        assert_eq!(decoded.len(), 1);
        assert!(decoded[0].starts_with("vless://"));
    }

    #[test]
    fn relay_usage_summarizes_recent_windows() {
        let unique = now_epoch_millis();
        let path = std::env::temp_dir().join(format!("hydra-relay-usage-{unique}"));
        std::fs::create_dir_all(&path).unwrap();
        let store = RelayUsageStore::load(&path).unwrap();
        store.record_wss_usage(10).unwrap();
        let summary = store.summary().unwrap();
        assert!(summary.today_bytes >= 10);
        let _ = std::fs::remove_dir_all(path);
    }

    #[test]
    fn parse_policy_action_requires_profile_for_explicit_routes() {
        assert!(parse_policy_action("wss", "").is_err());
        assert!(parse_policy_action("vless", "").is_err());
        assert!(matches!(
            parse_policy_action("block", "").unwrap(),
            RoutePolicyAction::Block
        ));
    }
}
