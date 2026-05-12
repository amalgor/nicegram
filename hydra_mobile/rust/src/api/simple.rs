#[flutter_rust_bridge::frb(sync)]
pub fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

#[flutter_rust_bridge::frb(sync)]
pub fn init_app() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::new(
        "info,hydra_core::transport=debug,rustls=warn,tungstenite=debug"
    );
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(crate::api::telemetry::FlutterLogLayer);
    let _ = tracing::subscriber::set_global_default(subscriber);

    tracing::info!("Hydra proxy-only node initialized.");
}

use hydra_config::HydraConfig;
use hydra_core::{transport, Socks5Server};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

lazy_static::lazy_static! {
    static ref SHARED_PROXY_MODE: tokio::sync::Mutex<Option<Arc<std::sync::RwLock<String>>>> =
        tokio::sync::Mutex::new(None);
    static ref NODE_STARTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
}

pub fn init_extension_runtime(base_dir: String) -> anyhow::Result<()> {
    init_app();
    crate::api::shared_state::init_shared_base_dir(base_dir)?;
    Ok(())
}

pub async fn prepare_local_runtime(base_dir: String) -> anyhow::Result<()> {
    let base_path = PathBuf::from(&base_dir);
    crate::api::shared_state::init_shared_base_dir(&base_path)?;

    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;

    crate::api::vpn::SOCKS5_PORT.store(
        config.network.socks5_port,
        std::sync::atomic::Ordering::Relaxed,
    );

    let _ = crate::api::routes::bootstrap(&base_path, &config)?;

    tracing::info!("Hydra local runtime prepared at {}", base_path.display());
    Ok(())
}

/// Start SOCKS5 proxy server with configured transports (VLESS, SSH).
/// No VPN, no P2P, no AI, no classifier — pure proxy mode.
pub async fn start_hydra_node(base_dir: String) -> anyhow::Result<()> {
    if NODE_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        tracing::warn!("start_hydra_node called again — node already running, skipping.");
        return Ok(());
    }
    tracing::info!("Starting Hydra proxy-only node...");

    let base_path = PathBuf::from(&base_dir);
    crate::api::shared_state::init_shared_base_dir(&base_path)?;
    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;
    let usage_recorder = crate::api::routes::bootstrap(&base_path, &config)?;

    crate::api::vpn::SOCKS5_PORT.store(
        config.network.socks5_port,
        std::sync::atomic::Ordering::Relaxed,
    );

    let transports =
        transport::build_transports_from_profiles(&crate::api::routes::current_profiles()?)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], config.network.socks5_port));
    tracing::info!("Starting SOCKS5 proxy on {}", addr);

    let server = Socks5Server::new_proxy_only(
        addr,
        transports,
        config.network.proxy_mode.clone(),
        Some(usage_recorder),
    )?;

    {
        let mut shared = SHARED_PROXY_MODE.lock().await;
        *shared = Some(server.proxy_mode_handle());
    }
    crate::api::routes::attach_runtime_handles(server.registry(), server.transports_handle())?;

    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("SOCKS5 proxy error: {}", e);
        }
    });

    tracing::info!("Hydra SOCKS5 proxy ready on {}", addr);
    Ok(())
}

/// Set proxy mode at runtime. Values: "off", "telegram", "full".
pub async fn set_proxy_mode(mode: String) -> anyhow::Result<()> {
    let guard = SHARED_PROXY_MODE.lock().await;
    let proxy_mode = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    if let Ok(mut m) = proxy_mode.write() {
        tracing::info!("Proxy mode changed to: {}", mode);
        *m = mode;
    }
    Ok(())
}

/// Get SOCKS5 proxy listen port.
#[flutter_rust_bridge::frb(sync)]
pub fn get_socks5_port() -> u16 {
    crate::api::vpn::SOCKS5_PORT.load(std::sync::atomic::Ordering::Relaxed)
}

/// Check if the SOCKS5 node is running.
#[flutter_rust_bridge::frb(sync)]
pub fn is_node_running() -> bool {
    NODE_STARTED.load(std::sync::atomic::Ordering::SeqCst)
}

/// Execute a shell command on the device.
/// Returns JSON: { "exit_code": N|null, "stdout": "...", "stderr": "...", "truncated": bool, "timed_out": bool }
///
/// Only read-only inspection commands are allowed (ls, cat, ps, etc.).
/// Destructive commands (rm, kill, reboot, etc.) are blocked.
pub async fn shell_exec(command: String) -> anyhow::Result<String> {
    use hydra_core::shell_executor::ShellExecutor;

    let executor = ShellExecutor::new();
    let result = executor.exec(&command).await?;
    let json = serde_json::json!({
        "exit_code": result.exit_code,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "truncated": result.truncated,
        "timed_out": result.timed_out,
    });
    Ok(json.to_string())
}

/// Execute multiple shell commands sequentially.
/// Returns JSON array of results.
pub async fn shell_exec_batch(commands: Vec<String>) -> anyhow::Result<String> {
    use hydra_core::shell_executor::ShellExecutor;

    let executor = ShellExecutor::new();
    let refs: Vec<&str> = commands.iter().map(|s| s.as_str()).collect();
    let results = executor.exec_batch(&refs).await;

    let json_results: Vec<serde_json::Value> = results
        .into_iter()
        .map(|r| match r {
            Ok(cr) => serde_json::json!({
                "exit_code": cr.exit_code,
                "stdout": cr.stdout,
                "stderr": cr.stderr,
                "truncated": cr.truncated,
                "timed_out": cr.timed_out,
            }),
            Err(e) => serde_json::json!({
                "exit_code": null,
                "stdout": "",
                "stderr": format!("[ERROR] {}", e),
                "truncated": false,
                "timed_out": false,
            }),
        })
        .collect();

    Ok(serde_json::json!(json_results).to_string())
}

/// Collect a full network inspection from the device.
pub async fn inspect_device_network() -> anyhow::Result<String> {
    use hydra_core::shell_commands::DeviceInspector;

    let inspector = DeviceInspector::new();
    let inspection = inspector.inspect_network().await;
    let json = serde_json::json!({
        "tcp_connections": inspection.tcp_connections,
        "tcp6_connections": inspection.tcp6_connections,
        "udp_sockets": inspection.udp_sockets,
        "processes": inspection.processes,
        "net_interfaces": inspection.net_interfaces,
        "dns_config": inspection.dns_config,
        "routes": inspection.routes,
        "uid_stats": inspection.uid_stats,
        "errors": inspection.errors,
    });
    Ok(json.to_string())
}

/// Quick one-line network summary: established/listening TCP counts + top UIDs.
pub async fn quick_network_summary() -> anyhow::Result<String> {
    use hydra_core::shell_commands::DeviceInspector;

    let inspector = DeviceInspector::new();
    inspector.quick_network_summary().await
}
