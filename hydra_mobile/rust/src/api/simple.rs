#[flutter_rust_bridge::frb(sync)]
pub fn greet(name: String) -> String {
    format!("Hello, {name}!")
}

pub fn init_app() {
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::EnvFilter;
    let filter = EnvFilter::new(
        "info,hydra_core::transport=debug,libp2p=warn,libp2p_noise=warn,libp2p_kad=warn,libp2p_gossipsub=warn,libp2p_swarm=warn,libp2p_dns=warn,libp2p_identify=warn,libp2p_mdns=warn,hickory=warn,rustls=warn,tungstenite=debug"
    );
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(crate::api::telemetry::FlutterLogLayer);
    let _ = tracing::subscriber::set_global_default(subscriber);

    tracing::info!("Hydra P2P node initialized and ready.");
}

use hydra_ai::AiNegotiator;
use hydra_config::HydraConfig;
use hydra_core::connections::ConnectionRegistry;
use hydra_core::{transport, Socks5Server};
use hydra_econ::EconLedger;
use hydra_p2p::{P2PHandle, P2PNode};
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

lazy_static::lazy_static! {
    static ref SHARED_REGISTRY: tokio::sync::Mutex<Option<Arc<ConnectionRegistry>>> =
        tokio::sync::Mutex::new(None);
    static ref SHARED_PROXY_MODE: tokio::sync::Mutex<Option<Arc<std::sync::RwLock<String>>>> =
        tokio::sync::Mutex::new(None);
    static ref SHARED_P2P_HANDLE: tokio::sync::Mutex<Option<P2PHandle>> =
        tokio::sync::Mutex::new(None);
    static ref SHARED_CLASSIFIER: tokio::sync::Mutex<Option<Arc<hydra_core::classifier::ConnectionClassifier>>> =
        tokio::sync::Mutex::new(None);
    static ref NODE_STARTED: std::sync::atomic::AtomicBool =
        std::sync::atomic::AtomicBool::new(false);
}

static SNAPSHOT_TASK_STARTED: AtomicBool = AtomicBool::new(false);

fn init_quota_from_config(config: &HydraConfig) {
    let (transport_url, device_id) = config
        .primary_quota_transport()
        .unwrap_or_else(|| (String::new(), "hydra-mobile".to_string()));
    crate::api::quota::init_quota(transport_url, device_id);
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

    init_quota_from_config(&config);
    let _ = crate::credit_runtime::init_credit_services(base_path.clone(), &config).await?;
    let _ = crate::api::routes::bootstrap(&base_path, &config)?;

    let ai = Arc::new(AiNegotiator::new(&config.ai));
    {
        let mut shared = crate::api::model_manager::SHARED_AI.lock().await;
        *shared = Some(ai);
    }

    tracing::info!("Hydra local runtime prepared at {}", base_path.display());
    Ok(())
}

pub async fn start_hydra_node(base_dir: String) -> anyhow::Result<()> {
    if NODE_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        tracing::warn!("start_hydra_node called again — node already running, skipping.");
        return Ok(());
    }
    tracing::info!("Starting real Hydra mobile node...");

    let base_path = PathBuf::from(&base_dir);
    crate::api::shared_state::init_shared_base_dir(&base_path)?;
    let config_path = base_path.join("hydra.toml");
    let config = HydraConfig::load_with_base_dir(&config_path, &base_path)?;
    let usage_recorder = crate::api::routes::bootstrap(&base_path, &config)?;

    init_quota_from_config(&config);
    let (discovery, credit, provider_metrics) =
        crate::credit_runtime::init_credit_services(base_path.clone(), &config).await?;

    // Store the SOCKS5 port for tun2proxy to read
    crate::api::vpn::SOCKS5_PORT.store(
        config.network.socks5_port,
        std::sync::atomic::Ordering::Relaxed,
    );

    let econ = match EconLedger::new(config.econ.db_path.to_str().unwrap()) {
        Ok(e) => {
            tracing::info!("Economic ledger initialized");
            Arc::new(e)
        }
        Err(e) => {
            tracing::error!("Failed to init EconLedger: {}. Check that no other Hydra instance is using the same db_path.", e);
            return Err(anyhow::anyhow!("EconLedger init failed: {}", e));
        }
    };

    let ai = Arc::new(AiNegotiator::new(&config.ai));
    {
        let mut shared = crate::api::model_manager::SHARED_AI.lock().await;
        *shared = Some(ai.clone());
    }

    if config.ai.model_path.exists() {
        tracing::info!(
            "AI model available at {} — will load on first use (lazy) to conserve memory.",
            config.ai.model_path.display()
        );
    } else {
        tracing::info!(
            "AI model not found: {}. LLM features disabled until model is downloaded.",
            config.ai.model_path.display()
        );
    }

    tracing::info!("Initializing P2P Node & mDNS discovery...");
    let p2p_handle = match P2PNode::new(None, config.network.p2p_listen_port, &config.network).await
    {
        Ok((p2p_node, handle)) => {
            tokio::spawn(async move {
                if let Err(e) = p2p_node.run().await {
                    tracing::error!("P2P node error: {}", e);
                }
            });
            tracing::info!("P2P node started");
            handle
        }
        Err(e) => {
            tracing::warn!(
                "P2P node init failed (expected on Android — no /etc/resolv.conf): {}. \
                 Continuing without peer discovery; transport routing still available.",
                e
            );
            P2PNode::dummy_handle()
        }
    };

    let transports =
        transport::build_transports_from_profiles(&crate::api::routes::current_profiles()?)?;
    if let Some(service) = &discovery {
        service.attach_p2p_handle(p2p_handle.clone()).await;
    }
    let addr = SocketAddr::from(([127, 0, 0, 1], config.network.socks5_port));
    tracing::info!("Starting SOCKS5 Server on {}", addr);
    let server = Socks5Server::new(
        addr,
        ai,
        p2p_handle.clone(),
        econ,
        transports,
        config.network.proxy_mode.clone(),
        discovery,
        credit,
        provider_metrics,
        Some(usage_recorder),
        &config.intelligence,
    )?;

    // Store registry and proxy mode handle for FRB API access
    {
        let mut shared = SHARED_REGISTRY.lock().await;
        *shared = Some(server.registry());
    }
    {
        let mut shared = SHARED_PROXY_MODE.lock().await;
        *shared = Some(server.proxy_mode_handle());
    }
    {
        let mut shared = SHARED_P2P_HANDLE.lock().await;
        *shared = Some(p2p_handle.clone());
    }
    {
        let mut shared = SHARED_CLASSIFIER.lock().await;
        *shared = Some(server.classifier());
    }
    crate::api::routes::attach_runtime_handles(server.registry(), server.transports_handle())?;

    tokio::spawn(async move {
        if let Err(e) = server.run().await {
            tracing::error!("Socks5 server error: {}", e);
        }
    });

    ensure_snapshot_writer();

    tracing::info!("Hydra Core ready!");
    Ok(())
}

pub(crate) async fn shared_p2p_handle() -> Option<P2PHandle> {
    SHARED_P2P_HANDLE.lock().await.clone()
}

fn protocol_number(protocol: &str) -> i32 {
    match protocol.to_ascii_lowercase().as_str() {
        "udp" => 17,
        _ => 6,
    }
}

async fn sync_pending_app_resolutions() {
    let registry = {
        let guard = SHARED_REGISTRY.lock().await;
        guard.clone()
    };

    let Some(registry) = registry else {
        return;
    };

    let snapshots = registry.snapshot(true);
    for snapshot in &snapshots {
        if snapshot.app_uid.is_some() {
            continue;
        }

        let (Some(local_ip), Some(local_port)) = (snapshot.src_ip.as_deref(), snapshot.src_port)
        else {
            continue;
        };

        let remote_ip = snapshot.dst_ip.as_deref().unwrap_or(&snapshot.target_host);

        let protocol = protocol_number(snapshot.src_protocol.as_deref().unwrap_or("tcp"));
        if let Some(attribution) = crate::api::app_resolver::queue_resolution(
            snapshot.id,
            protocol,
            local_ip,
            local_port,
            remote_ip,
            snapshot.target_port,
        ) {
            registry.update_app_attribution(snapshot.id, &attribution);
        }
    }

    for (connection_id, attribution) in crate::api::app_resolver::take_completed_resolutions() {
        registry.update_app_attribution(connection_id, &attribution);
    }
}

/// Get active connections as JSON for Flutter UI.
pub async fn get_active_connections() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let snaps = registry.snapshot(false);

    let json_arr: Vec<serde_json::Value> = snaps
        .iter()
        .map(|s| {
            serde_json::json!({
                "id": s.id,
                "target_host": s.target_host,
                "target_port": s.target_port,
                "route_type": s.route_type,
                "bytes_up": s.bytes_up,
                "bytes_down": s.bytes_down,
                "duration_ms": s.duration_ms,
                "is_proxied": s.is_proxied,
                "status": s.status,
                "ai_reason": s.ai_reason,
                "group_kind": s.group_kind,
                "group_key": s.group_key,
                "app_label": s.app_label,
                "package_name": s.package_name,
                "app_uid": s.app_uid,
                "reverse_dns": s.reverse_dns,
                "whois_org": s.whois_org,
                "whois_asn": s.whois_asn,
                "whois_country": s.whois_country,
                "classification_category": s.classification_category,
                "classification_confidence": s.classification_confidence,
                "classification_source": s.classification_source,
                "classification_explanation": s.classification_explanation,
                "resolved_policy": s.resolved_policy,
                "transport_label": s.transport_label,
                "src_ip": s.src_ip,
                "src_port": s.src_port,
                "src_protocol": s.src_protocol,
            })
        })
        .collect();

    let json = serde_json::to_string(&json_arr)?;
    let _ = crate::api::shared_state::persist_active_connections(&json);
    Ok(json)
}

/// Get connection stats summary as JSON.
pub async fn get_connection_stats() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let stats = registry.stats();

    let json = serde_json::json!({
        "active_count": stats.active_count,
        "total_count": stats.total_count,
        "proxied_count": stats.proxied_count,
        "blocked_count": stats.blocked_count,
        "tracker_count": stats.tracker_count,
        "total_bytes_up": stats.total_bytes_up,
        "total_bytes_down": stats.total_bytes_down,
    })
    .to_string();
    let _ = crate::api::shared_state::persist_connection_stats(&json);
    Ok(json)
}

/// Toggle proxy for a specific connection.
pub async fn set_connection_proxy(conn_id: u64, proxied: bool) -> anyhow::Result<()> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    registry.set_force_proxy(conn_id, proxied);
    Ok(())
}

fn ensure_snapshot_writer() {
    if SNAPSHOT_TASK_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    tokio::spawn(async {
        loop {
            let _ = crate::api::routes::reload_from_disk();
            sync_pending_app_resolutions().await;
            if let Ok(json) = get_active_connections().await {
                let _ = crate::api::shared_state::persist_active_connections(&json);
            }
            if let Ok(json) = get_connection_stats().await {
                let _ = crate::api::shared_state::persist_connection_stats(&json);
            }
            let _ = crate::api::shared_state::persist_quota_status(
                &crate::api::quota::get_quota_status(),
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    });
}

/// Set proxy mode at runtime. Values: "off", "telegram", "full".
/// Called from Flutter Settings when user changes proxy mode.
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

/// Connection security analysis using classifier stats and cached verdicts.
/// Returns a structured JSON summary instead of free-text LLM output.
pub async fn analyze_connections(_connections_json: String) -> anyhow::Result<String> {
    let classifier_guard = SHARED_CLASSIFIER.lock().await;
    let classifier = match classifier_guard.as_ref() {
        Some(c) => c,
        None => return Ok(serde_json::json!({"error": "Node not started"}).to_string()),
    };
    let stats = classifier.stats();

    let registry_guard = SHARED_REGISTRY.lock().await;
    let registry = match registry_guard.as_ref() {
        Some(r) => r,
        None => return Ok(serde_json::json!({"error": "Node not started"}).to_string()),
    };

    let snaps = registry.snapshot(false);
    let total = snaps.len();
    let classified = snaps
        .iter()
        .filter(|s| s.classification_category.is_some())
        .count();
    let tracker_categories = ["advertising", "analytics", "telemetry", "social_tracking"];
    let trackers = snaps
        .iter()
        .filter(|s| {
            s.classification_category
                .as_deref()
                .map(|c| tracker_categories.contains(&c))
                .unwrap_or(false)
        })
        .count();
    let malware = snaps
        .iter()
        .filter(|s| s.classification_category.as_deref() == Some("malware"))
        .count();
    let blocked = snaps
        .iter()
        .filter(|s| s.route_type == "blocked")
        .count();

    // Collect tracker domains for the summary
    let mut tracker_domains: Vec<&str> = snaps
        .iter()
        .filter(|s| {
            s.classification_category
                .as_deref()
                .map(|c| tracker_categories.contains(&c))
                .unwrap_or(false)
        })
        .map(|s| s.target_host.as_str())
        .collect();
    tracker_domains.sort();
    tracker_domains.dedup();
    tracker_domains.truncate(10);

    let json = serde_json::json!({
        "total_connections": total,
        "classified": classified,
        "trackers_found": trackers,
        "malware_found": malware,
        "blocked": blocked,
        "tracker_domains": tracker_domains,
        "classifier_stats": {
            "cache_size": stats.cache_size,
            "cache_hit_rate": stats.cache_hit_rate,
            "tracker_hits": stats.tracker_hits,
            "rules_applied": stats.rules_applied,
            "llm_pending": stats.llm_pending,
        },
    });
    Ok(json.to_string())
}

/// Get classifier statistics as JSON.
pub async fn get_classifier_stats() -> anyhow::Result<String> {
    let guard = SHARED_CLASSIFIER.lock().await;
    let classifier = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let stats = classifier.stats();

    let json = serde_json::json!({
        "cache_size": stats.cache_size,
        "cache_hit_rate": stats.cache_hit_rate,
        "tracker_hits": stats.tracker_hits,
        "rules_applied": stats.rules_applied,
        "llm_pending": stats.llm_pending,
    })
    .to_string();
    Ok(json)
}

/// Toggle intelligence auto-block feature at runtime.
pub async fn set_intelligence_auto_block(enabled: bool) -> anyhow::Result<()> {
    let guard = SHARED_CLASSIFIER.lock().await;
    let classifier = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    classifier.set_auto_block(enabled);
    tracing::info!(enabled = enabled, "Intelligence auto_block updated from mobile UI");
    Ok(())
}

/// Host classification lookup — returns cached verdict from the intelligence pipeline.
/// If no classification is cached yet, returns "pending" status.
pub async fn analyze_host(
    host: String,
    port: u16,
    _is_proxied: bool,
    _bytes_total: u64,
) -> anyhow::Result<String> {
    let classifier_guard = SHARED_CLASSIFIER.lock().await;
    if let Some(classifier) = classifier_guard.as_ref() {
        let key = hydra_core::classifier::VerdictKey {
            host_or_domain: host.to_lowercase(),
            app_uid: None,
        };
        if let Some(cached) = classifier.verdict_cache().get(&key) {
            return Ok(serde_json::json!({
                "host": host,
                "port": port,
                "category": cached.category.as_str(),
                "confidence": cached.confidence,
                "source": cached.source.as_str(),
                "explanation": cached.explanation,
            })
            .to_string());
        }
    }

    Ok(serde_json::json!({
        "host": host,
        "port": port,
        "category": "unknown",
        "confidence": 0.0,
        "source": "pending",
        "explanation": "Classification pending",
    })
    .to_string())
}

/// Get classification log stats as JSON.
pub async fn get_classification_log_stats() -> anyhow::Result<String> {
    let base_dir = crate::api::shared_state::shared_base_dir()
        .ok_or_else(|| anyhow::anyhow!("Shared base dir not initialized"))?;
    let primary = base_dir.join("classification_events.jsonl");
    let mut file_size_bytes = std::fs::metadata(&primary).map(|m| m.len()).unwrap_or(0);
    let mut total_events = 0u64;
    let mut oldest_timestamp: Option<u64> = None;

    for path in classification_log_paths(&base_dir) {
        if !path.exists() {
            continue;
        }
        let file = std::fs::File::open(&path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                total_events = total_events.saturating_add(1);
                if let Some(ts) = value.get("timestamp").and_then(|v| v.as_u64()) {
                    oldest_timestamp = Some(oldest_timestamp.map_or(ts, |old| old.min(ts)));
                }
            }
        }
    }

    if file_size_bytes == 0 {
        file_size_bytes = classification_log_paths(&base_dir)
            .iter()
            .filter_map(|path| std::fs::metadata(path).ok())
            .map(|meta| meta.len())
            .sum();
    }

    Ok(serde_json::json!({
        "total_events": total_events,
        "file_size_bytes": file_size_bytes,
        "oldest_timestamp": oldest_timestamp,
    })
    .to_string())
}

/// Export the last N classification events as a JSON array.
pub async fn export_classification_events(limit: u32) -> anyhow::Result<String> {
    let base_dir = crate::api::shared_state::shared_base_dir()
        .ok_or_else(|| anyhow::anyhow!("Shared base dir not initialized"))?;
    let mut events: Vec<serde_json::Value> = Vec::new();

    for path in classification_log_paths(&base_dir) {
        if !path.exists() {
            continue;
        }
        let file = std::fs::File::open(&path)?;
        let reader = std::io::BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                events.push(value);
            }
        }
    }

    if events.len() > limit as usize {
        let start = events.len() - limit as usize;
        events = events.split_off(start);
    }

    Ok(serde_json::to_string(&events)?)
}

fn classification_log_paths(base_dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths = vec![base_dir.join("classification_events.jsonl")];
    for idx in 1..=3 {
        paths.push(base_dir.join(format!("classification_events.{idx}.jsonl")));
    }
    paths
}

/// Group connections by application (package_name).
/// Returns JSON: { "groups": [ { "app": "...", "label": "...", "count": N, "bytes": N, "trackers": N } ] }
pub async fn get_connections_by_app() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let snaps = registry.snapshot(false);

    let mut groups: std::collections::HashMap<String, AppGroup> = std::collections::HashMap::new();
    for s in &snaps {
        let key = s.package_name.clone().unwrap_or_else(|| "unknown".to_string());
        let entry = groups.entry(key).or_insert_with(|| AppGroup {
            app: s.package_name.clone().unwrap_or_else(|| "unknown".to_string()),
            label: s.app_label.clone(),
            count: 0,
            bytes: 0,
            trackers: 0,
        });
        entry.count += 1;
        entry.bytes += s.bytes_up + s.bytes_down;
        if is_tracker_category(s.classification_category.as_deref()) {
            entry.trackers += 1;
        }
    }

    let mut list: Vec<_> = groups.into_values().collect();
    list.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    Ok(serde_json::json!({ "groups": list }).to_string())
}

#[derive(serde::Serialize)]
struct AppGroup {
    app: String,
    label: Option<String>,
    count: u64,
    bytes: u64,
    trackers: u64,
}

/// Group connections by classification category.
/// Returns JSON: { "groups": [ { "category": "...", "count": N, "bytes": N } ] }
pub async fn get_connections_by_category() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let snaps = registry.snapshot(false);

    let mut groups: std::collections::HashMap<String, CategoryGroup> = std::collections::HashMap::new();
    for s in &snaps {
        let key = s.classification_category.clone().unwrap_or_else(|| "unknown".to_string());
        let entry = groups.entry(key.clone()).or_insert_with(|| CategoryGroup {
            category: key,
            count: 0,
            bytes: 0,
        });
        entry.count += 1;
        entry.bytes += s.bytes_up + s.bytes_down;
    }

    let mut list: Vec<_> = groups.into_values().collect();
    list.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    Ok(serde_json::json!({ "groups": list }).to_string())
}

#[derive(serde::Serialize)]
struct CategoryGroup {
    category: String,
    count: u64,
    bytes: u64,
}

/// Group connections by country (from WHOIS).
/// Returns JSON: { "groups": [ { "country": "...", "count": N, "bytes": N, "trackers": N } ] }
pub async fn get_connections_by_country() -> anyhow::Result<String> {
    let guard = SHARED_REGISTRY.lock().await;
    let registry = guard
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Node not started"))?;
    let snaps = registry.snapshot(false);

    let mut groups: std::collections::HashMap<String, CountryGroup> = std::collections::HashMap::new();
    for s in &snaps {
        let key = s.whois_country.clone().unwrap_or_else(|| "unknown".to_string());
        let entry = groups.entry(key.clone()).or_insert_with(|| CountryGroup {
            country: key,
            count: 0,
            bytes: 0,
            trackers: 0,
        });
        entry.count += 1;
        entry.bytes += s.bytes_up + s.bytes_down;
        if is_tracker_category(s.classification_category.as_deref()) {
            entry.trackers += 1;
        }
    }

    let mut list: Vec<_> = groups.into_values().collect();
    list.sort_by(|a, b| b.bytes.cmp(&a.bytes));

    Ok(serde_json::json!({ "groups": list }).to_string())
}

#[derive(serde::Serialize)]
struct CountryGroup {
    country: String,
    count: u64,
    bytes: u64,
    trackers: u64,
}

fn is_tracker_category(cat: Option<&str>) -> bool {
    matches!(cat, Some("advertising") | Some("analytics") | Some("telemetry") | Some("social_tracking"))
}
