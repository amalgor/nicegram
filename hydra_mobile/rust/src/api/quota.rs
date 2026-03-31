use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static LOCAL_BYTES_USED: AtomicU64 = AtomicU64::new(0);
static QUOTA_LIMIT: AtomicU64 = AtomicU64::new(52_428_800); // 50 MB default
static RELAY_URL: OnceLock<String> = OnceLock::new();
static DEVICE_ID: OnceLock<String> = OnceLock::new();

/// Initialize the quota system with relay URL and device ID.
pub fn init_quota(relay_url: String, device_id: String) {
    let _ = RELAY_URL.set(relay_url);
    let _ = DEVICE_ID.set(device_id);
    let _ = crate::api::shared_state::persist_quota_status(&get_quota_status());
}

/// Record bytes consumed locally (called from relay/proxy layer).
pub fn record_bytes(bytes: u64) {
    LOCAL_BYTES_USED.fetch_add(bytes, Ordering::Relaxed);
    let _ = crate::api::shared_state::persist_quota_status(&get_quota_status());
}

/// Get current quota status as JSON string.
/// Returns: {"used": N, "limit": M, "remaining": R, "resets_at": "..."}
pub fn get_quota_status() -> String {
    let used = LOCAL_BYTES_USED.load(Ordering::Relaxed);
    let limit = QUOTA_LIMIT.load(Ordering::Relaxed);
    let remaining = limit.saturating_sub(used);

    let now = chrono::Utc::now();
    let tomorrow = (now + chrono::Duration::days(1))
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let resets_at = tomorrow.format("%Y-%m-%dT00:00:00Z").to_string();

    let json = serde_json::json!({
        "used": used,
        "limit": limit,
        "remaining": remaining,
        "resets_at": resets_at,
    })
    .to_string();
    let _ = crate::api::shared_state::persist_quota_status(&json);
    json
}

/// Sync local quota with the server. Returns updated quota JSON.
pub async fn sync_quota_with_server() -> anyhow::Result<String> {
    let relay_url = RELAY_URL.get().cloned().unwrap_or_default();
    let device_id = DEVICE_ID
        .get()
        .cloned()
        .unwrap_or_else(|| "anonymous".to_string());

    if relay_url.is_empty() {
        return Ok(get_quota_status());
    }

    let quota_url = relay_url
        .replace("wss://", "https://")
        .replace("ws://", "http://")
        .trim_end_matches('/')
        .to_string()
        + &format!("/quota?device_id={}", device_id);

    let client = reqwest::Client::new();
    let resp = client.get(&quota_url).send().await?;
    let body: serde_json::Value = resp.json().await?;

    let server_remaining = body["remaining"].as_u64().unwrap_or(52_428_800);
    let server_limit = body["limit"].as_u64().unwrap_or(52_428_800);
    let server_used = server_limit.saturating_sub(server_remaining);

    let local_used = LOCAL_BYTES_USED.load(Ordering::Relaxed);
    let effective_used = local_used.max(server_used);
    LOCAL_BYTES_USED.store(effective_used, Ordering::Relaxed);
    QUOTA_LIMIT.store(server_limit, Ordering::Relaxed);

    let json = get_quota_status();
    let _ = crate::api::shared_state::persist_quota_status(&json);
    Ok(json)
}

/// Reset daily quota counter.
pub fn reset_daily_quota() {
    LOCAL_BYTES_USED.store(0, Ordering::Relaxed);
    let _ = crate::api::shared_state::persist_quota_status(&get_quota_status());
}

/// Get raw quota values for testing/UI.
pub fn get_quota_raw() -> (u64, u64) {
    let used = LOCAL_BYTES_USED.load(Ordering::Relaxed);
    let limit = QUOTA_LIMIT.load(Ordering::Relaxed);
    (used, limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reset_state() {
        LOCAL_BYTES_USED.store(0, Ordering::Relaxed);
        QUOTA_LIMIT.store(52_428_800, Ordering::Relaxed);
    }

    #[test]
    fn test_record_bytes_accumulates() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        reset_state();
        record_bytes(1000);
        record_bytes(2000);
        let (used, _) = get_quota_raw();
        assert_eq!(used, 3000);
    }

    #[test]
    fn test_reset_daily_quota() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        reset_state();
        record_bytes(5_000_000);
        assert_eq!(get_quota_raw().0, 5_000_000);
        reset_daily_quota();
        assert_eq!(get_quota_raw().0, 0);
    }

    #[test]
    fn test_get_quota_status_json() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        reset_state();
        record_bytes(10_000_000);
        let json_str = get_quota_status();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["used"], 10_000_000u64);
        assert_eq!(v["limit"], 52_428_800u64);
        assert_eq!(v["remaining"], 52_428_800u64 - 10_000_000u64);
        assert!(v["resets_at"].as_str().unwrap().contains("T00:00:00Z"));
    }

    #[test]
    fn test_quota_saturating_sub() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        reset_state();
        QUOTA_LIMIT.store(100, Ordering::Relaxed);
        record_bytes(200);
        let (used, limit) = get_quota_raw();
        assert_eq!(used, 200);
        assert_eq!(limit, 100);
        let json_str = get_quota_status();
        let v: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(v["remaining"], 0u64);
    }

    #[test]
    fn test_init_quota() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        init_quota(
            "wss://relay.example.com".to_string(),
            "device-123".to_string(),
        );
        assert_eq!(RELAY_URL.get().unwrap(), "wss://relay.example.com");
        assert_eq!(DEVICE_ID.get().unwrap(), "device-123");
    }
}
