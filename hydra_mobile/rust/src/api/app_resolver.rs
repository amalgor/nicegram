use async_trait::async_trait;
use hydra_core::connections::AppAttribution as CoreAppAttribution;
use moka::sync::Cache;
use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppAttribution {
    pub uid: i32,
    pub package_name: Option<String>,
    pub app_label: Option<String>,
}

impl AppAttribution {
    pub fn unknown() -> Self {
        Self {
            uid: -1,
            package_name: None,
            app_label: None,
        }
    }

    pub fn is_valid(&self) -> bool {
        self.uid > 0
            && self
                .package_name
                .as_deref()
                .is_some_and(|value| !value.is_empty())
    }

    fn to_core(&self) -> Option<CoreAppAttribution> {
        let uid = u32::try_from(self.uid).ok()?;
        let package_name = self.package_name.clone()?;
        if package_name.is_empty() {
            return None;
        }
        Some(CoreAppAttribution {
            uid,
            package_name: package_name.clone(),
            app_label: self.app_label.clone().unwrap_or(package_name),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct PendingAppResolution {
    connection_id: u64,
    protocol: i32,
    local_ip: String,
    local_port: u16,
    remote_ip: String,
    remote_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SubmittedAppResolution {
    connection_id: u64,
    attribution: AppAttribution,
}

static UID_CACHE: OnceLock<Cache<i32, AppAttribution>> = OnceLock::new();
static CONNECTION_CACHE: OnceLock<Cache<String, AppAttribution>> = OnceLock::new();

pub struct MobileAppResolver;

#[async_trait]
impl hydra_core::AppResolver for MobileAppResolver {
    async fn resolve_app(
        &self,
        protocol: i32,
        local_ip: &str,
        local_port: u16,
        remote_ip: &str,
        remote_port: u16,
    ) -> Option<CoreAppAttribution> {
        resolve_app_for_connection(protocol, local_ip, local_port, remote_ip, remote_port).await
    }
}

fn uid_cache() -> &'static Cache<i32, AppAttribution> {
    UID_CACHE.get_or_init(|| {
        Cache::builder()
            .max_capacity(1000)
            .time_to_live(Duration::from_secs(3600))
            .build()
    })
}

fn connection_cache() -> &'static Cache<String, AppAttribution> {
    CONNECTION_CACHE.get_or_init(|| {
        Cache::builder()
            .max_capacity(1000)
            .time_to_live(Duration::from_secs(3600))
            .build()
    })
}

fn connection_key(
    protocol: i32,
    local_ip: &str,
    local_port: u16,
    remote_ip: &str,
    remote_port: u16,
) -> String {
    format!("{protocol}|{local_ip}:{local_port}|{remote_ip}:{remote_port}")
}

#[cfg(target_os = "android")]
fn android_vm() -> anyhow::Result<&'static jni::JavaVM> {
    static JVM: OnceLock<jni::JavaVM> = OnceLock::new();
    static JVM_INIT: Mutex<()> = Mutex::new(());

    if let Some(vm) = JVM.get() {
        return Ok(vm);
    }

    let _guard = JVM_INIT
        .lock()
        .map_err(|error| anyhow::anyhow!("android_vm init mutex poisoned: {error}"))?;

    if let Some(vm) = JVM.get() {
        return Ok(vm);
    }

    let vm = unsafe {
        jni::JavaVM::from_raw(ndk_context::android_context().vm().cast())
            .map_err(|error| anyhow::anyhow!("create JavaVM wrapper failed: {error}"))?
    };

    let _ = JVM.set(vm);
    JVM.get()
        .ok_or_else(|| anyhow::anyhow!("android_vm init failed"))
}

#[cfg(target_os = "android")]
fn android_resolve_app_json(
    protocol: i32,
    local_ip: &str,
    local_port: u16,
    remote_ip: &str,
    remote_port: u16,
) -> anyhow::Result<String> {
    use jni::objects::{JString, JValue};

    let mut env = android_vm()?
        .attach_current_thread()
        .map_err(|error| anyhow::anyhow!("attach_current_thread failed: {error}"))?;

    let local_ip = env.new_string(local_ip)?;
    let remote_ip = env.new_string(remote_ip)?;
    let result = env.call_static_method(
        "com/hydra/network/hydra_mobile/AppResolver",
        "resolveByConnectionJson",
        "(ILjava/lang/String;ILjava/lang/String;I)Ljava/lang/String;",
        &[
            JValue::Int(protocol),
            JValue::Object(&local_ip),
            JValue::Int(i32::from(local_port)),
            JValue::Object(&remote_ip),
            JValue::Int(i32::from(remote_port)),
        ],
    )?;

    let value = result.l()?;
    if value.is_null() {
        return Ok("null".to_string());
    }

    let value = JString::from(value);
    let resolved: String = env.get_string(&value)?.into();
    Ok(resolved)
}

#[cfg(target_os = "android")]
fn resolve_android_with_retries(
    protocol: i32,
    local_ip: &str,
    local_port: u16,
    remote_ip: &str,
    remote_port: u16,
) -> Option<AppAttribution> {
    let retry_delays = [0_u64, 30, 90];
    for delay_ms in retry_delays {
        if delay_ms > 0 {
            std::thread::sleep(Duration::from_millis(delay_ms));
        }

        match android_resolve_app_json(protocol, local_ip, local_port, remote_ip, remote_port) {
            Ok(json) => {
                if let Some(attr) = parse_attribution_json(&json) {
                    return Some(attr);
                }
            }
            Err(error) => {
                tracing::debug!(
                    protocol,
                    local_ip,
                    local_port,
                    remote_ip,
                    remote_port,
                    "android app resolve failed: {error}"
                );
            }
        }
    }

    None
}

pub async fn resolve_app_for_connection(
    protocol: i32,
    local_ip: &str,
    local_port: u16,
    remote_ip: &str,
    remote_port: u16,
) -> Option<CoreAppAttribution> {
    let key = connection_key(protocol, local_ip, local_port, remote_ip, remote_port);
    if let Some(attr) = connection_cache().get(&key) {
        return attr.to_core();
    }

    #[cfg(target_os = "android")]
    {
        let local_ip = local_ip.to_string();
        let remote_ip = remote_ip.to_string();
        let resolved = tokio::task::spawn_blocking(move || {
            resolve_android_with_retries(protocol, &local_ip, local_port, &remote_ip, remote_port)
        })
        .await
        .ok()
        .flatten();

        if let Some(attr) = resolved {
            cache_resolution(&key, attr.clone());
            return attr.to_core();
        }
    }

    None
}

pub fn get_cached_attribution(host: &str, port: u16) -> Option<AppAttribution> {
    let key = connection_key(6, "0.0.0.0", 0, host, port);
    connection_cache().get(&key)
}

pub fn cache_attribution(host: &str, port: u16, attribution: AppAttribution) {
    let key = connection_key(6, "0.0.0.0", 0, host, port);
    cache_resolution(&key, attribution);
}

fn cache_resolution(connection_key: &str, attribution: AppAttribution) {
    connection_cache().insert(connection_key.to_string(), attribution.clone());
    uid_cache().insert(attribution.uid, attribution);
}

pub fn parse_attribution_json(json: &str) -> Option<AppAttribution> {
    serde_json::from_str(json).ok().and_then(
        |attr: AppAttribution| {
            if attr.is_valid() {
                Some(attr)
            } else {
                None
            }
        },
    )
}

pub fn cache_attribution_from_json(host: &str, port: u16, json: &str) -> bool {
    if let Some(attr) = parse_attribution_json(json) {
        cache_attribution(host, port, attr);
        true
    } else {
        false
    }
}

pub fn cache_stats() -> (u64, u64) {
    let c = connection_cache();
    (c.entry_count(), c.weighted_size())
}

lazy_static::lazy_static! {
    static ref PENDING_RESOLUTIONS: Mutex<Vec<PendingAppResolution>> = Mutex::new(Vec::new());
    static ref COMPLETED_RESOLUTIONS: Mutex<Vec<SubmittedAppResolution>> = Mutex::new(Vec::new());
}

pub(crate) fn queue_resolution(
    connection_id: u64,
    protocol: i32,
    local_ip: &str,
    local_port: u16,
    remote_ip: &str,
    remote_port: u16,
) -> Option<hydra_core::connections::AppAttribution> {
    let key = connection_key(protocol, local_ip, local_port, remote_ip, remote_port);
    if let Some(attr) = connection_cache().get(&key) {
        return attr.to_core();
    }

    if let Ok(mut pending) = PENDING_RESOLUTIONS.lock() {
        let request = PendingAppResolution {
            connection_id,
            protocol,
            local_ip: local_ip.to_string(),
            local_port,
            remote_ip: remote_ip.to_string(),
            remote_port,
        };
        if !pending.contains(&request) && pending.len() < 100 {
            pending.push(request);
        }
    }

    None
}

pub(crate) fn take_completed_resolutions() -> Vec<(u64, CoreAppAttribution)> {
    if let Ok(mut completed) = COMPLETED_RESOLUTIONS.lock() {
        std::mem::take(&mut *completed)
            .into_iter()
            .filter_map(|entry| {
                entry
                    .attribution
                    .to_core()
                    .map(|attr| (entry.connection_id, attr))
            })
            .collect()
    } else {
        Vec::new()
    }
}

pub fn request_resolution(host: &str, port: u16) {
    if get_cached_attribution(host, port).is_some() {
        return;
    }
    let _ = queue_resolution(0, 6, "0.0.0.0", 0, host, port);
}

pub fn take_pending_resolutions() -> Vec<(String, u16)> {
    if let Ok(mut pending) = PENDING_RESOLUTIONS.lock() {
        let legacy = std::mem::take(&mut *pending);
        legacy
            .into_iter()
            .map(|entry| (entry.remote_ip, entry.remote_port))
            .collect()
    } else {
        Vec::new()
    }
}

fn take_pending_resolution_requests() -> Vec<PendingAppResolution> {
    if let Ok(mut pending) = PENDING_RESOLUTIONS.lock() {
        std::mem::take(&mut *pending)
    } else {
        Vec::new()
    }
}

#[flutter_rust_bridge::frb(sync)]
pub fn get_pending_app_resolutions() -> String {
    let pending = take_pending_resolution_requests();
    serde_json::to_string(&pending).unwrap_or_else(|_| "[]".to_string())
}

#[flutter_rust_bridge::frb(sync)]
pub fn submit_app_resolution(host: String, port: u16, json: String) -> bool {
    if let Ok(submitted) = serde_json::from_str::<SubmittedAppResolution>(&json) {
        let key = connection_key(6, "0.0.0.0", 0, &host, port);
        cache_resolution(&key, submitted.attribution.clone());
        if let Ok(mut completed) = COMPLETED_RESOLUTIONS.lock() {
            completed.push(submitted);
            return true;
        }
        return false;
    }

    cache_attribution_from_json(&host, port, &json)
}

#[flutter_rust_bridge::frb(sync)]
pub fn get_cached_app_attribution(host: String, port: u16) -> String {
    match get_cached_attribution(&host, port) {
        Some(attr) => serde_json::to_string(&attr).unwrap_or_else(|_| "null".to_string()),
        None => "null".to_string(),
    }
}

#[flutter_rust_bridge::frb(sync)]
pub fn get_app_cache_stats() -> String {
    let (count, size) = cache_stats();
    format!(r#"{{"entry_count":{},"weighted_size":{}}}"#, count, size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_attribution() {
        let attr = AppAttribution {
            uid: 10001,
            package_name: Some("com.example.app".to_string()),
            app_label: Some("Example App".to_string()),
        };

        cache_attribution("cache-test.example.com", 8443, attr.clone());

        let cached = get_cached_attribution("cache-test.example.com", 8443);
        assert!(cached.is_some());
        let cached = cached.unwrap();
        assert_eq!(cached.uid, 10001);
        assert_eq!(cached.package_name.as_deref(), Some("com.example.app"));
    }

    #[test]
    fn test_parse_attribution_json() {
        let json = r#"{"uid":10001,"package_name":"com.test","app_label":"Test"}"#;
        let attr = parse_attribution_json(json);
        assert!(attr.is_some());
        let attr = attr.unwrap();
        assert_eq!(attr.uid, 10001);
        assert_eq!(attr.package_name.as_deref(), Some("com.test"));
    }

    #[test]
    fn test_invalid_uid_rejected() {
        let json = r#"{"uid":0,"package_name":"","app_label":""}"#;
        let attr = parse_attribution_json(json);
        assert!(attr.is_none());
    }

    #[test]
    fn test_pending_resolutions() {
        let _ = queue_resolution(1, 6, "10.0.0.2", 12345, "test.com", 80);
        let _ = queue_resolution(1, 6, "10.0.0.2", 12345, "test.com", 80);
        let _ = queue_resolution(2, 6, "10.0.0.2", 12346, "other.com", 443);

        let pending = take_pending_resolution_requests();
        assert_eq!(pending.len(), 2);

        let pending2 = take_pending_resolution_requests();
        assert!(pending2.is_empty());
    }

    #[test]
    fn test_submit_completed_resolution() {
        let json = r#"{"connection_id":42,"attribution":{"uid":10001,"package_name":"com.test","app_label":"Test"}}"#;
        assert!(submit_app_resolution(
            "example.com".to_string(),
            443,
            json.to_string()
        ));
        let completed = take_completed_resolutions();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].0, 42);
        assert_eq!(completed[0].1.uid, 10001);
        assert_eq!(completed[0].1.package_name, "com.test");
    }
}
