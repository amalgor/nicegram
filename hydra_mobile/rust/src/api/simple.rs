#[flutter_rust_bridge::frb(sync)]
pub fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

/// Install logging. `log_dir` enables persistent, rotating log files.
/// Safe to call more than once (later calls only add the file sink if missing).
#[flutter_rust_bridge::frb(sync)]
pub fn init_app(log_dir: Option<String>) {
    crate::logging::init(log_dir.map(PathBuf::from));
    tracing::info!(
        "Hydra proxy-only runtime initialized (rust_lib_hydra_mobile {}, {} {})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
}

use hydra_config::HydraConfig;
use hydra_core::connections::ConnectionRegistry;
use hydra_core::transport::{self, ConfiguredTransport};
use hydra_core::Socks5Server;
use serde::Serialize;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

const KNOWN_HOSTS_FILE: &str = "ssh_known_hosts.json";

struct RunningNode {
    addr: SocketAddr,
    started_at_ms: u64,
    abort: tokio::task::AbortHandle,
    finished: Arc<AtomicBool>,
    transports: Arc<RwLock<Vec<ConfiguredTransport>>>,
    registry: Arc<ConnectionRegistry>,
    proxy_mode: Arc<RwLock<String>>,
}

static NODE: LazyLock<tokio::sync::Mutex<Option<RunningNode>>> =
    LazyLock::new(|| tokio::sync::Mutex::new(None));
static LAST_ERROR: LazyLock<StdMutex<Option<String>>> = LazyLock::new(|| StdMutex::new(None));
static RUNNING: AtomicBool = AtomicBool::new(false);

fn set_last_error(error: Option<String>) {
    *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = error;
}

fn last_error() -> Option<String> {
    LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn init_extension_runtime(base_dir: String) -> anyhow::Result<()> {
    init_app(None);
    crate::api::shared_state::init_shared_base_dir(base_dir)?;
    Ok(())
}

pub async fn prepare_local_runtime(base_dir: String) -> anyhow::Result<()> {
    let base_path = PathBuf::from(&base_dir);
    crate::api::shared_state::init_shared_base_dir(&base_path)?;
    hydra_core::transport::ssh::set_known_hosts_path(base_path.join(KNOWN_HOSTS_FILE));

    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;

    crate::api::vpn::SOCKS5_PORT.store(config.network.socks5_port, Ordering::Relaxed);

    let _ = crate::api::routes::bootstrap(&base_path, &config)?;

    info!("Hydra local runtime prepared at {}", base_path.display());
    Ok(())
}

/// Start the local SOCKS5 server (127.0.0.1:<socks5_port>) over the enabled
/// route profiles. Idempotent. Returns an error if the port cannot be bound.
pub async fn start_hydra_node(base_dir: String) -> anyhow::Result<()> {
    let mut node = NODE.lock().await;
    if let Some(running) = node.as_ref() {
        if !running.finished.load(Ordering::SeqCst) {
            info!(proxy_event = "start_skipped", "SOCKS5 server already running on {}", running.addr);
            return Ok(());
        }
        warn!(proxy_event = "stale_server", "Previous SOCKS5 server task had stopped; starting a new one");
        *node = None;
    }

    info!(proxy_event = "starting", "Starting Hydra SOCKS5 server...");
    match start_inner(&base_dir).await {
        Ok(running) => {
            info!(
                proxy_event = "started",
                listen = %running.addr,
                "SOCKS5 proxy ready on {}",
                running.addr
            );
            set_last_error(None);
            RUNNING.store(true, Ordering::SeqCst);
            warm_up_transports(running.transports.clone());
            *node = Some(running);
            Ok(())
        }
        Err(e) => {
            let message = format!("{e:#}");
            error!(proxy_event = "start_failed", error = %message, "SOCKS5 server failed to start");
            set_last_error(Some(message));
            RUNNING.store(false, Ordering::SeqCst);
            Err(e)
        }
    }
}

async fn start_inner(base_dir: &str) -> anyhow::Result<RunningNode> {
    let base_path = PathBuf::from(base_dir);
    crate::api::shared_state::init_shared_base_dir(&base_path)?;
    hydra_core::transport::ssh::set_known_hosts_path(base_path.join(KNOWN_HOSTS_FILE));
    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;
    let usage_recorder = crate::api::routes::bootstrap(&base_path, &config)?;

    crate::api::vpn::SOCKS5_PORT.store(config.network.socks5_port, Ordering::Relaxed);

    let profiles = crate::api::routes::current_profiles()?;
    for profile in profiles.iter().filter(|p| p.enabled) {
        info!(
            proxy_event = "route",
            profile_id = %profile.id,
            kind = ?profile.kind,
            priority = profile.priority,
            "Active route: {}",
            profile.label
        );
    }
    if !profiles.iter().any(|p| p.enabled) {
        warn!(proxy_event = "no_routes", "No route is enabled: every proxied request will fail until a server is configured");
    }

    let transports = transport::build_transports_from_profiles(&profiles)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], config.network.socks5_port));

    let server = Socks5Server::new_proxy_only(
        addr,
        transports,
        config.network.proxy_mode.clone(),
        Some(usage_recorder),
    )?;
    let listener = server.bind().await?;

    let transports = server.transports_handle();
    let registry = server.registry();
    let proxy_mode = server.proxy_mode_handle();
    crate::api::routes::attach_runtime_handles(registry.clone(), transports.clone())?;

    let finished = Arc::new(AtomicBool::new(false));
    let serve = tokio::spawn(async move { server.serve(listener).await });
    let abort = serve.abort_handle();
    let finished_flag = finished.clone();
    tokio::spawn(async move {
        let outcome = serve.await;
        finished_flag.store(true, Ordering::SeqCst);
        RUNNING.store(false, Ordering::SeqCst);
        match outcome {
            Ok(Ok(())) => info!(proxy_event = "stopped", "SOCKS5 server stopped"),
            Ok(Err(e)) => {
                error!(proxy_event = "crashed", error = %format!("{e:#}"), "SOCKS5 server exited with error");
                set_last_error(Some(format!("Proxy server stopped: {e:#}")));
            }
            Err(e) if e.is_cancelled() => info!(proxy_event = "stopped", "SOCKS5 server stopped"),
            Err(e) => {
                error!(proxy_event = "crashed", error = %e, "SOCKS5 server task panicked");
                set_last_error(Some(format!("Proxy server crashed: {e}")));
            }
        }
    });

    Ok(RunningNode {
        addr,
        started_at_ms: now_ms(),
        abort,
        finished,
        transports,
        registry,
        proxy_mode,
    })
}

/// Stop the SOCKS5 server and drop SSH sessions.
pub async fn stop_hydra_node() -> anyhow::Result<()> {
    let running = NODE.lock().await.take();
    RUNNING.store(false, Ordering::SeqCst);
    let Some(running) = running else {
        return Ok(());
    };
    info!(proxy_event = "stopping", "Stopping SOCKS5 server on {}", running.addr);
    running.abort.abort();
    let transports = snapshot_transports(&running.transports);
    for configured in transports {
        configured.transport.reset().await;
    }
    Ok(())
}

/// Drop and re-establish long-lived transport sessions. Call after network
/// changes or when returning to the foreground.
pub async fn reconnect_transports(reason: String) -> anyhow::Result<()> {
    let transports = {
        let node = NODE.lock().await;
        match node.as_ref() {
            Some(running) => running.transports.clone(),
            None => return Ok(()),
        }
    };
    info!(proxy_event = "reconnect", reason = %reason, "Reconnecting transports");
    for configured in snapshot_transports(&transports) {
        configured.transport.reset().await;
    }
    warm_up_transports(transports);
    Ok(())
}

/// Start connecting long-lived sessions in the background (like `ssh -D`
/// connects before the first request).
#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn warm_up_transports(transports: Arc<RwLock<Vec<ConfiguredTransport>>>) {
    for configured in snapshot_transports(&transports) {
        let label = configured.metadata.label.clone();
        tokio::spawn(async move {
            if let Err(e) = configured.transport.warm_up().await {
                warn!(proxy_event = "warm_up_failed", route = %label, error = %format!("{e:#}"), "Route is not ready");
            }
        });
    }
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) async fn running_transports() -> Option<Arc<RwLock<Vec<ConfiguredTransport>>>> {
    NODE.lock().await.as_ref().map(|running| running.transports.clone())
}

fn snapshot_transports(handle: &Arc<RwLock<Vec<ConfiguredTransport>>>) -> Vec<ConfiguredTransport> {
    handle.read().unwrap_or_else(|e| e.into_inner()).clone()
}

#[derive(Serialize)]
struct ProxyStatus {
    running: bool,
    listen: Option<String>,
    port: u16,
    started_at_ms: Option<u64>,
    last_error: Option<String>,
    proxy_mode: Option<String>,
    connections_active: usize,
    connections_total: usize,
    bytes_up: u64,
    bytes_down: u64,
    routes: Vec<RouteStatus>,
    ssh: Vec<hydra_core::transport::ssh::SshStatus>,
}

#[derive(Serialize)]
struct RouteStatus {
    label: String,
    kind: &'static str,
    profile_id: Option<String>,
}

/// JSON snapshot for the UI; cheap enough to poll every second.
pub async fn get_proxy_status() -> anyhow::Result<String> {
    let node = NODE.lock().await;
    let running = node
        .as_ref()
        .filter(|running| !running.finished.load(Ordering::SeqCst));
    let stats = running.map(|r| r.registry.stats());
    let routes = running
        .map(|r| {
            snapshot_transports(&r.transports)
                .into_iter()
                .map(|t| RouteStatus {
                    label: t.metadata.label.clone(),
                    kind: t.kind.as_str(),
                    profile_id: t.metadata.profile_id.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let status = ProxyStatus {
        running: running.is_some(),
        listen: running.map(|r| r.addr.to_string()),
        port: get_socks5_port(),
        started_at_ms: running.map(|r| r.started_at_ms),
        last_error: last_error(),
        proxy_mode: running.map(|r| r.proxy_mode.read().unwrap_or_else(|e| e.into_inner()).clone()),
        connections_active: stats.as_ref().map(|s| s.active_count).unwrap_or(0),
        connections_total: stats.as_ref().map(|s| s.total_count).unwrap_or(0),
        bytes_up: stats.as_ref().map(|s| s.total_bytes_up).unwrap_or(0),
        bytes_down: stats.as_ref().map(|s| s.total_bytes_down).unwrap_or(0),
        routes,
        ssh: hydra_core::transport::ssh::status_snapshot(),
    };
    Ok(serde_json::to_string(&status)?)
}

/// Set proxy mode at runtime. Values: "off", "telegram", "full".
pub async fn set_proxy_mode(mode: String) -> anyhow::Result<()> {
    let node = NODE.lock().await;
    let running = node
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    if let Ok(mut m) = running.proxy_mode.write() {
        info!("Proxy mode changed to: {}", mode);
        *m = mode;
    }
    Ok(())
}

/// Get SOCKS5 proxy listen port.
#[flutter_rust_bridge::frb(sync)]
pub fn get_socks5_port() -> u16 {
    crate::api::vpn::SOCKS5_PORT.load(Ordering::Relaxed)
}

/// True while the SOCKS5 server task is alive.
#[flutter_rust_bridge::frb(sync)]
pub fn is_node_running() -> bool {
    RUNNING.load(Ordering::SeqCst)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
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
