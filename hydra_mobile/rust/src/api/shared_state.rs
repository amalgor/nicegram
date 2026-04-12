use anyhow::Result;
use lazy_static::lazy_static;
use serde_json::Value;
use std::path::Path;
pub use std::path::PathBuf;
use std::sync::Mutex;

const MAX_LOG_LINES: usize = 10_000;

lazy_static! {
    static ref SHARED_BASE_DIR: Mutex<Option<PathBuf>> = Mutex::new(None);
    static ref LOG_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
}

pub fn init_shared_base_dir<P: AsRef<Path>>(base_dir: P) -> Result<()> {
    let path = base_dir.as_ref().to_path_buf();
    std::fs::create_dir_all(&path)?;

    {
        let mut guard = SHARED_BASE_DIR
            .lock()
            .map_err(|e| anyhow::anyhow!("Shared base dir lock poisoned: {}", e))?;
        *guard = Some(path.clone());
    }

    if let Ok(existing) = read_log_lines() {
        let mut logs = LOG_LINES
            .lock()
            .map_err(|e| anyhow::anyhow!("Log buffer lock poisoned: {}", e))?;
        *logs = existing;
    }

    Ok(())
}

pub fn shared_base_dir() -> Option<PathBuf> {
    SHARED_BASE_DIR.lock().ok().and_then(|guard| guard.clone())
}

pub fn append_log_line(line: String) {
    let snapshot = {
        let mut guard = match LOG_LINES.lock() {
            Ok(guard) => guard,
            Err(_) => return,
        };
        guard.push(line);
        if guard.len() > MAX_LOG_LINES {
            let overflow = guard.len() - MAX_LOG_LINES;
            guard.drain(0..overflow);
        }
        guard.clone()
    };

    let _ = write_json_array("logs.json", &snapshot);
}

pub fn read_log_lines() -> Result<Vec<String>> {
    match read_json_value("logs.json")? {
        Some(Value::Array(items)) => Ok(items
            .into_iter()
            .filter_map(|value| value.as_str().map(ToOwned::to_owned))
            .collect()),
        Some(_) => Ok(Vec::new()),
        None => Ok(Vec::new()),
    }
}

pub fn persist_active_connections(json: &str) -> Result<()> {
    write_json_raw("active_connections.json", json)
}

pub fn persist_connection_stats(json: &str) -> Result<()> {
    write_json_raw("connection_stats.json", json)
}

pub fn persist_quota_status(json: &str) -> Result<()> {
    write_json_raw("quota_status.json", json)
}

pub fn snapshot_json() -> Result<String> {
    let active_connections =
        read_json_value("active_connections.json")?.unwrap_or_else(|| Value::Array(Vec::new()));
    let connection_stats = read_json_value("connection_stats.json")?.unwrap_or_else(|| {
        serde_json::json!({
            "active_count": 0,
            "total_count": 0,
            "proxied_count": 0,
            "total_bytes_up": 0,
            "total_bytes_down": 0,
        })
    });
    let quota_status = read_json_value("quota_status.json")?.unwrap_or_else(|| {
        serde_json::json!({
            "used": 0,
            "limit": 0,
            "remaining": 0,
            "resets_at": "",
        })
    });
    let logs = read_json_value("logs.json")?.unwrap_or_else(|| Value::Array(Vec::new()));

    Ok(serde_json::json!({
        "active_connections": active_connections,
        "connection_stats": connection_stats,
        "quota_status": quota_status,
        "logs": logs,
    })
    .to_string())
}

fn shared_file(name: &str) -> Result<PathBuf> {
    let guard = SHARED_BASE_DIR
        .lock()
        .map_err(|e| anyhow::anyhow!("Shared base dir lock poisoned: {}", e))?;
    let base_dir = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Shared base dir not initialized"))?;
    Ok(base_dir.join(name))
}

fn write_json_raw(name: &str, json: &str) -> Result<()> {
    let path = shared_file(name)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn write_json_array(name: &str, lines: &[String]) -> Result<()> {
    let path = shared_file(name)?;
    let json = serde_json::to_vec(lines)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn read_json_value(name: &str) -> Result<Option<Value>> {
    let path = match shared_file(name) {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    if !path.exists() {
        return Ok(None);
    }

    let bytes = std::fs::read(path)?;
    Ok(Some(serde_json::from_slice(&bytes)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn snapshot_round_trip_persists_files() {
        let _guard = crate::test_support::GLOBAL_TEST_GUARD.lock().unwrap();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = std::env::temp_dir().join(format!("hydra-shared-state-test-{}", unique));
        std::fs::create_dir_all(&tmp).unwrap();
        init_shared_base_dir(&tmp).unwrap();
        persist_active_connections(r#"[{"id":1}]"#).unwrap();
        persist_connection_stats(r#"{"active_count":1}"#).unwrap();
        persist_quota_status(r#"{"used":2,"limit":3,"remaining":1,"resets_at":"x"}"#).unwrap();
        append_log_line("hello".to_string());

        let snapshot = snapshot_json().unwrap();
        let parsed: Value = serde_json::from_str(&snapshot).unwrap();
        assert_eq!(parsed["active_connections"][0]["id"], 1);
        assert_eq!(parsed["connection_stats"]["active_count"], 1);
        assert_eq!(parsed["logs"][0], "hello");
        let _ = std::fs::remove_dir_all(tmp);
    }
}
