use anyhow::Result;

/// Write a Dart-side event into the shared log timeline (file + live view).
#[flutter_rust_bridge::frb(sync)]
pub fn log_message(level: String, target: String, message: String) {
    let level = match level.to_ascii_uppercase().as_str() {
        "ERROR" => "ERROR",
        "WARN" | "WARNING" => "WARN",
        "DEBUG" => "DEBUG",
        "TRACE" => "TRACE",
        _ => "INFO",
    };
    let target = if target.is_empty() { "dart".to_string() } else { format!("dart::{target}") };
    crate::logging::emit(level, &target, &message);
}

#[flutter_rust_bridge::frb(sync)]
pub fn log_directory() -> Option<String> {
    crate::logging::log_dir().map(|p| p.display().to_string())
}

/// Session log files, newest first.
#[flutter_rust_bridge::frb(sync)]
pub fn list_log_files() -> Vec<String> {
    crate::logging::log_files()
        .into_iter()
        .map(|p| p.display().to_string())
        .collect()
}

/// Write a self-contained diagnostics report (status, redacted servers,
/// recent log lines) next to the logs and return its path.
pub async fn write_diagnostics_report(app_info: String) -> Result<String> {
    let dir = crate::logging::log_dir()
        .ok_or_else(|| anyhow::anyhow!("File logging is not initialized"))?;
    let status = crate::api::simple::get_proxy_status()
        .await
        .unwrap_or_else(|e| format!("<unavailable: {e}>"));
    let status = serde_json::from_str::<serde_json::Value>(&status)
        .and_then(|v| serde_json::to_string_pretty(&v))
        .unwrap_or(status);
    let servers = crate::api::routes::redacted_profiles_json();
    let config = crate::api::shared_state::shared_base_dir()
        .and_then(|base| std::fs::read_to_string(base.join("hydra.toml")).ok())
        .unwrap_or_else(|| "<unavailable>".into());
    let files = crate::logging::log_files()
        .into_iter()
        .map(|p| {
            let size = std::fs::metadata(&p).map(|m| m.len()).unwrap_or(0);
            format!("  {} ({} bytes)", p.display(), size)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let lines = crate::api::shared_state::read_log_lines().unwrap_or_default();
    let tail = lines[lines.len().saturating_sub(1500)..].join("\n");

    let report = format!(
        "Hydra diagnostics report\n\
         generated: {}\n\
         rust: rust_lib_hydra_mobile {} ({} {})\n\
         app: {}\n\n\
         == proxy status ==\n{}\n\n\
         == servers (secrets removed) ==\n{}\n\n\
         == hydra.toml ==\n{}\n\n\
         == log files ==\n{}\n\n\
         == recent log ({} lines) ==\n{}\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z"),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        app_info,
        status,
        servers,
        config.trim(),
        files,
        lines.len().min(1500),
        tail
    );
    let path = dir.join(format!(
        "diagnostics-{}.txt",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    ));
    std::fs::write(&path, report)?;
    tracing::info!(path = %path.display(), "Diagnostics report written");
    Ok(path.display().to_string())
}
