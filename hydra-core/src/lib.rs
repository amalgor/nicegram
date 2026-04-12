pub mod classifier;
pub mod classification_log;
pub mod connections;
pub mod discovery;
pub mod enrichment;
pub mod llm_classifier;
pub mod onion;
pub mod socks;
pub mod tracker_db;
pub mod transport;
use anyhow::Result;
use async_trait::async_trait;
use classifier::{ClassificationResult, ConnectionClassifier, ClassificationRequest};
use classification_log::ClassificationEventLog;
use llm_classifier::LlmClassifier;
use connections::{ConnectionRegistry, ResolvedRoutePolicy, RoutePolicyAction, RouteType};
use discovery::RouteDiscoveryService;
use enrichment::EnrichmentService;
use hydra_ai::AiNegotiator;
use hydra_config::IntelligenceConfig;
use hydra_econ::EconLedger;
use hydra_econ::provider::ProviderMetricsLedger;
use hydra_p2p::P2PHandle;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracker_db::TrackerDatabase;
use tracing::{debug, error, info, warn};
use transport::{ConfiguredTransport, TransportKind};

#[derive(Debug, Clone, Default)]
pub struct CreditRuntimeStatus {
    pub premium_allowed: bool,
    pub throttle_factor: f64,
    pub fallback_to_free: bool,
}

#[async_trait]
pub trait CreditController: Send + Sync {
    async fn current_status(&self) -> Result<CreditRuntimeStatus>;
    async fn record_usage(
        &self,
        transport: &ConfiguredTransport,
        bytes_total: u64,
        duration: Duration,
    ) -> Result<()>;
}

#[async_trait]
pub trait UsageRecorder: Send + Sync {
    async fn record_usage(
        &self,
        transport: &ConfiguredTransport,
        bytes_total: u64,
        duration: Duration,
    ) -> Result<()>;
}

pub struct Socks5Server {
    addr: SocketAddr,
    #[allow(dead_code)]
    ai: Arc<AiNegotiator>,
    #[allow(dead_code)]
    p2p: P2PHandle,
    #[allow(dead_code)]
    econ: Arc<EconLedger>,
    transports: Arc<RwLock<Vec<ConfiguredTransport>>>,
    proxy_mode: Arc<RwLock<String>>,
    registry: Arc<ConnectionRegistry>,
    discovery: Option<Arc<RouteDiscoveryService>>,
    credit: Option<Arc<dyn CreditController>>,
    provider_metrics: Option<Arc<ProviderMetricsLedger>>,
    usage_recorder: Option<Arc<dyn UsageRecorder>>,
    enrichment: Arc<EnrichmentService>,
    classifier: Arc<ConnectionClassifier>,
    classification_log: Arc<ClassificationEventLog>,
    llm_rx: std::sync::Mutex<Option<mpsc::Receiver<ClassificationRequest>>>,
}

impl Socks5Server {
    pub fn new(
        addr: SocketAddr,
        ai: Arc<AiNegotiator>,
        p2p: P2PHandle,
        econ: Arc<EconLedger>,
        transports: Vec<ConfiguredTransport>,
        proxy_mode: String,
        discovery: Option<Arc<RouteDiscoveryService>>,
        credit: Option<Arc<dyn CreditController>>,
        provider_metrics: Option<Arc<ProviderMetricsLedger>>,
        usage_recorder: Option<Arc<dyn UsageRecorder>>,
        intelligence_config: &IntelligenceConfig,
    ) -> Result<Self> {
        let enrichment = EnrichmentService::new()?;
        let tracker_db = Arc::new(TrackerDatabase::new());
        let (classifier, llm_rx) = ConnectionClassifier::new(tracker_db, intelligence_config);
        let classification_log = Arc::new(ClassificationEventLog::new(
            crate::classification_log::default_base_dir(),
        )?);
        info!(
            "ConnectionClassifier initialized: auto_block={}, threshold={}",
            intelligence_config.auto_block_trackers,
            intelligence_config.block_confidence_threshold,
        );
        Ok(Self {
            addr,
            ai,
            p2p,
            econ,
            transports: Arc::new(RwLock::new(transports)),
            proxy_mode: Arc::new(RwLock::new(proxy_mode)),
            registry: Arc::new(ConnectionRegistry::new()),
            discovery,
            credit,
            provider_metrics,
            usage_recorder,
            enrichment: Arc::new(enrichment),
            classifier: Arc::new(classifier),
            classification_log,
            llm_rx: std::sync::Mutex::new(Some(llm_rx)),
        })
    }

    pub fn registry(&self) -> Arc<ConnectionRegistry> {
        self.registry.clone()
    }

    pub fn classifier(&self) -> Arc<ConnectionClassifier> {
        self.classifier.clone()
    }

    pub fn classification_log(&self) -> Arc<ClassificationEventLog> {
        self.classification_log.clone()
    }

    pub fn proxy_mode_handle(&self) -> Arc<RwLock<String>> {
        self.proxy_mode.clone()
    }

    pub fn transports_handle(&self) -> Arc<RwLock<Vec<ConfiguredTransport>>> {
        self.transports.clone()
    }

    pub fn set_proxy_mode(&self, mode: &str) {
        if let Ok(mut m) = self.proxy_mode.write() {
            *m = mode.to_string();
            tracing::info!("Proxy mode changed to: {}", mode);
        }
    }

    pub async fn run(&self) -> Result<()> {
        if let Some(discovery) = &self.discovery {
            discovery.start_polling();
        }

        // Spawn LLM classifier task (Tier 3: best-effort async classification)
        if let Some(llm_rx) = self.llm_rx.lock().unwrap().take() {
            let infer = self.ai.infer().clone();
            let registry = self.registry.clone();
            let verdict_cache = self.classifier.verdict_cache().clone();
            let llm_classifier = LlmClassifier::new(infer, llm_rx);
            tokio::spawn(async move {
                llm_classifier.run(registry, verdict_cache).await;
            });
            info!("LLM classifier task spawned for Tier 3 classification");
        }

        let listener = TcpListener::bind(self.addr).await?;
        info!("Socks5 server listening on {}", self.addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            debug!("Accepted connection from {}", peer_addr);

            let transports = self
                .transports
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let proxy_mode = self
                .proxy_mode
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let registry = self.registry.clone();
            let discovery = self.discovery.clone();
            let credit = self.credit.clone();
            let p2p = self.p2p.clone();
            let provider_metrics = self.provider_metrics.clone();
            let usage_recorder = self.usage_recorder.clone();
            let enrichment = self.enrichment.clone();
            let classifier = self.classifier.clone();
            let classification_log = self.classification_log.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(
                    stream,
                    transports,
                    proxy_mode,
                    registry,
                    discovery,
                    credit,
                    p2p,
                    provider_metrics,
                    usage_recorder,
                    enrichment,
                    classifier,
                    classification_log,
                )
                .await
                {
                    error!("Error handling connection from {}: {}", peer_addr, e);
                }
            });

            self.registry.gc(500);
        }
    }
}

/// Read USERNAME/PASSWORD auth subnegotiation (RFC 1929).
/// Returns parsed SourceInfo from the username field.
async fn read_userpass_auth(stream: &mut TcpStream) -> Result<socks::SourceInfo> {
    // VER (1) | ULEN (1) | UNAME (1-255) | PLEN (1) | PASSWD (1-255)
    let mut ver_ulen = [0u8; 2];
    stream.read_exact(&mut ver_ulen).await?;

    if ver_ulen[0] != 0x01 {
        return Err(anyhow::anyhow!(
            "Invalid auth subnegotiation version: expected 0x01, got 0x{:02x}",
            ver_ulen[0]
        ));
    }

    let ulen = ver_ulen[1] as usize;
    let mut username = vec![0u8; ulen];
    stream.read_exact(&mut username).await?;

    let plen = stream.read_u8().await? as usize;
    let mut _password = vec![0u8; plen];
    stream.read_exact(&mut _password).await?;

    // Send success response: VER (0x01) | STATUS (0x00 = success)
    stream.write_all(&[0x01, 0x00]).await?;

    let username_str = String::from_utf8_lossy(&username);
    let source_info = socks::SourceInfo::parse(&username_str);

    if source_info.has_source() {
        debug!(
            "Source info from tun2proxy: {}:{} ({})",
            source_info
                .src_ip
                .map(|ip| ip.to_string())
                .unwrap_or_default(),
            source_info.src_port.unwrap_or(0),
            source_info.protocol
        );
    }

    Ok(source_info)
}

async fn handle_connection(
    mut stream: TcpStream,
    transports: Vec<ConfiguredTransport>,
    proxy_mode: String,
    registry: Arc<ConnectionRegistry>,
    discovery: Option<Arc<RouteDiscoveryService>>,
    credit: Option<Arc<dyn CreditController>>,
    p2p: P2PHandle,
    provider_metrics: Option<Arc<ProviderMetricsLedger>>,
    usage_recorder: Option<Arc<dyn UsageRecorder>>,
    enrichment: Arc<EnrichmentService>,
    classifier: Arc<ConnectionClassifier>,
    classification_log: Arc<ClassificationEventLog>,
) -> Result<()> {
    // 1. Negotiation (Handshake)
    let mut buf = [0u8; 2];
    stream.read_exact(&mut buf).await?;

    if buf[0] != 0x05 {
        return Err(anyhow::anyhow!("Invalid SOCKS version"));
    }

    let n_methods = buf[1] as usize;
    let mut methods = vec![0u8; n_methods];
    stream.read_exact(&mut methods).await?;

    // Prefer USERNAME/PASSWORD (0x02) to get source info from tun2proxy,
    // fallback to NO AUTHENTICATION (0x00)
    let source_info = if methods.contains(&socks::AUTH_USER_PASS) {
        stream.write_all(&[0x05, socks::AUTH_USER_PASS]).await?;
        Some(read_userpass_auth(&mut stream).await?)
    } else if methods.contains(&socks::AUTH_NONE) {
        stream.write_all(&[0x05, socks::AUTH_NONE]).await?;
        None
    } else {
        stream.write_all(&[0x05, socks::AUTH_NO_ACCEPTABLE]).await?;
        return Err(anyhow::anyhow!("No supported auth methods"));
    };

    // 2. Request
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;

    if header[0] != 0x05 || header[1] != 0x01 {
        // SOCKS5 CMD: 0x01=CONNECT, 0x02=BIND, 0x03=UDP ASSOCIATE
        // We only support CONNECT. UDP ASSOCIATE requests from tun2proxy (DNS) are expected noise.
        debug!(
            "Ignoring non-CONNECT SOCKS5 request (cmd=0x{:02x})",
            header[1]
        );
        return Ok(());
    }

    let target_addr = match header[3] {
        0x01 => {
            // IPv4
            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).await?;
            let mut port_buf = [0u8; 2];
            stream.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            format!("{}.{}.{}.{}:{}", buf[0], buf[1], buf[2], buf[3], port)
        }
        0x03 => {
            // Domain Name
            let len = stream.read_u8().await? as usize;
            let mut buf = vec![0u8; len];
            stream.read_exact(&mut buf).await?;
            let mut port_buf = [0u8; 2];
            stream.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            format!("{}:{}", String::from_utf8_lossy(&buf), port)
        }
        0x04 => {
            // IPv6
            let mut buf = [0u8; 16];
            stream.read_exact(&mut buf).await?;
            let mut port_buf = [0u8; 2];
            stream.read_exact(&mut port_buf).await?;
            let port = u16::from_be_bytes(port_buf);
            format!(
                "[{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}:{:02x}{:02x}]:{}",
                buf[0],
                buf[1],
                buf[2],
                buf[3],
                buf[4],
                buf[5],
                buf[6],
                buf[7],
                buf[8],
                buf[9],
                buf[10],
                buf[11],
                buf[12],
                buf[13],
                buf[14],
                buf[15],
                port
            )
        }
        _ => return Err(anyhow::anyhow!("Unsupported address type")),
    };

    info!("Target requested: {}", target_addr);

    let is_telegram = socks::is_telegram_target(&target_addr);
    let is_relay_infra = socks::is_relay_infrastructure(&target_addr);
    let credit_status = if let Some(controller) = &credit {
        match controller.current_status().await {
            Ok(status) => status,
            Err(error) => {
                warn!(
                    "Failed to load credit status, using safe defaults: {}",
                    error
                );
                CreditRuntimeStatus::default()
            }
        }
    } else {
        CreditRuntimeStatus::default()
    };

    let dynamic_transports = if let Some(discovery) = &discovery {
        discovery.attach_p2p_handle(p2p.clone()).await;
        discovery
            .get_transports(&proxy_mode, is_telegram, credit_status.premium_allowed)
            .await
    } else {
        Vec::new()
    };
    let premium_better = if credit_status.premium_allowed {
        if let Some(discovery) = &discovery {
            discovery.premium_is_materially_better().await
        } else {
            false
        }
    } else {
        false
    };
    let all_transports = order_candidate_transports(
        transports,
        dynamic_transports,
        credit_status.clone(),
        premium_better,
    );
    let resolved_policy = registry.resolve_policy(&target_addr, None);
    let policy_transports = filter_transports_for_policy(&all_transports, &resolved_policy.action);
    let plan = select_transport_plan(
        &target_addr,
        is_telegram,
        is_relay_infra,
        &proxy_mode,
        &resolved_policy,
        &policy_transports,
    );

    let conn_id = registry.register(
        &target_addr,
        resolved_policy.group.clone(),
        resolved_policy.action.clone(),
        None, // App attribution resolved asynchronously via mobile bridge
        source_info.as_ref(),
    );
    info!(
        "Connection #{}: target={}, telegram={}, proxy_required={}, mode={}, policy={}",
        conn_id,
        target_addr,
        is_telegram,
        plan.requires_proxy(),
        proxy_mode,
        resolved_policy.action.as_label(),
    );

    // Fast classification (synchronous, <1ms): check cache then tracker DB
    let (target_host_str, target_port) = socks::split_target(&target_addr)
        .map(|(h, p)| (h.to_string(), p))
        .unwrap_or_else(|| (target_addr.clone(), 0));

    let fast_verdict = classifier.classify_fast(&target_host_str, target_port, None);
    if let ClassificationResult::Verdict(ref classification) = fast_verdict {
        registry.update_classification(conn_id, classification.clone());
        classification_log.log_event(crate::classification_log::make_event(
            target_host_str.clone(),
            target_port,
            None,
            None,
            None,
            None,
            None,
            None,
            classification.category,
            classification.confidence,
            classification.source,
            classification.explanation.clone(),
            0,
            0,
            None,
        ));

        // Block tracker connections only when auto_block is on AND user has no explicit policy
        if classifier.should_block(classification)
            && resolved_policy.action == RoutePolicyAction::Auto
        {
            info!(
                conn_id = conn_id,
                host = target_host_str.as_str(),
                category = classification.category.as_str(),
                confidence = classification.confidence,
                "Blocking tracker connection (auto_block)"
            );
            registry.update_route(
                conn_id,
                RouteType::Blocked,
                Some(format!(
                    "Blocked: {} (confidence {:.0}%)",
                    classification.category.as_str(),
                    classification.confidence * 100.0
                )),
                Some("Blocked".to_string()),
            );
            stream.write_all(&socks::failure_reply()).await?;
            registry.close(conn_id);
            return Ok(());
        }
    }

    // Spawn async enrichment + enriched classification task (non-blocking)
    {
        let enrichment = enrichment.clone();
        let registry = registry.clone();
        let classifier = classifier.clone();
        let target_host = target_host_str.clone();
        let is_pending = matches!(fast_verdict, ClassificationResult::Pending);
        tokio::spawn(async move {
            let result = enrichment.enrich_host(&target_host).await;
            debug!(
                "Connection #{}: enrichment complete - org={:?}, asn={:?}, country={:?}",
                conn_id, result.whois_org, result.asn, result.country
            );
            registry.update_enrichment(
                conn_id,
                result.reverse_dns.clone(),
                result.whois_org.clone(),
                result.asn,
                result.country.clone(),
            );

            // Run enriched classification if fast path didn't produce a verdict
            if is_pending {
                classifier
                    .classify_enriched(
                        conn_id,
                        &target_host,
                        target_port,
                        None,
                        None,
                        None,
                        &result,
                        &registry,
                        &classification_log,
                    )
                    .await;

                // Late-block verdict: log but do not kill active connection
                if let Some(cached) = classifier.verdict_cache().get(
                    &classifier::VerdictKey {
                        host_or_domain: target_host.to_lowercase(),
                        app_uid: None,
                    },
                ) {
                    if classifier.should_block(&cached) {
                        info!(
                            conn_id = conn_id,
                            host = target_host.as_str(),
                            category = cached.category.as_str(),
                            confidence = cached.confidence,
                            "Late classification: would block but connection already active"
                        );
                    }
                }
            }
        });
    }

    if plan.is_blocked() {
        registry.update_route(
            conn_id,
            RouteType::Blocked,
            Some("Blocked by saved policy".to_string()),
            Some("Blocked".to_string()),
        );
        stream.write_all(&socks::failure_reply()).await?;
        registry.close(conn_id);
        return Ok(());
    }

    if plan.is_direct() {
        registry.update_route(
            conn_id,
            RouteType::Direct,
            Some("Direct".to_string()),
            Some("Direct".to_string()),
        );
        return do_direct(stream, &target_addr, conn_id, &registry).await;
    }

    if plan.transports.is_empty() {
        warn!(
            "Connection #{}: proxied path required for {}, but no matching transports are configured",
            conn_id, target_addr
        );
        registry.update_route(
            conn_id,
            route_type_for_policy(&resolved_policy.action),
            Some("No matching transports configured".to_string()),
            Some(resolved_policy.action.as_label()),
        );
        stream.write_all(&socks::failure_reply()).await?;
        registry.close(conn_id);
        return Ok(());
    }

    let mut errors = Vec::new();
    for configured in plan.transports {
        let connect_started = Instant::now();
        info!(
            "Connection #{}: trying transport {} to {}",
            conn_id,
            configured.kind.as_str(),
            target_addr
        );
        registry.update_route(
            conn_id,
            route_type_for_transport(&configured),
            Some(format!("Using {}", configured.metadata.label)),
            Some(configured.metadata.label.clone()),
        );

        match configured.transport.connect(&target_addr).await {
            Ok(outbound) => {
                stream.write_all(&socks::success_reply()).await?;
                let (mut ri, mut wi) = stream.into_split();
                let (mut ro, mut wo) = tokio::io::split(outbound);
                let started_at = Instant::now();
                let connect_latency_ms = connect_started.elapsed().as_millis() as u64;
                let limit_bps =
                    rate_limit_bytes_per_sec(&configured, credit_status.throttle_factor);
                let c2t_registry = registry.clone();
                let c2t = copy_with_rate_limit(&mut ri, &mut wo, limit_bps, move |bytes| {
                    c2t_registry.update_bytes(conn_id, bytes, 0);
                });
                let t2c_registry = registry.clone();
                let t2c = copy_with_rate_limit(&mut ro, &mut wi, limit_bps, move |bytes| {
                    t2c_registry.update_bytes(conn_id, 0, bytes);
                });
                let (res_up, res_down) = tokio::join!(c2t, t2c);
                let up = res_up.unwrap_or(0);
                let down = res_down.unwrap_or(0);
                let duration = started_at.elapsed();
                if let Some(controller) = &credit {
                    if let Err(error) = controller
                        .record_usage(&configured, up.saturating_add(down), duration)
                        .await
                    {
                        warn!("Failed to persist credit usage: {}", error);
                    }
                }
                if let Some(recorder) = &usage_recorder {
                    if let Err(error) = recorder
                        .record_usage(&configured, up.saturating_add(down), duration)
                        .await
                    {
                        warn!("Failed to persist transport usage: {}", error);
                    }
                }
                if let (Some(metrics), Some(agent_id)) =
                    (&provider_metrics, configured.metadata.agent_id)
                {
                    if let Err(error) = metrics.record_success(
                        agent_id,
                        up.saturating_add(down),
                        duration,
                        connect_latency_ms,
                        configured.metadata.price_per_gb_micro_usdc,
                    ) {
                        warn!("Failed to persist provider metrics success: {}", error);
                    }
                }
                registry.close(conn_id);
                return Ok(());
            }
            Err(e) => {
                warn!(
                    "Connection #{}: transport {} failed: {}",
                    conn_id,
                    configured.kind.as_str(),
                    e
                );
                if let (Some(metrics), Some(agent_id)) =
                    (&provider_metrics, configured.metadata.agent_id)
                {
                    if let Err(error) = metrics.record_failure(agent_id) {
                        warn!("Failed to persist provider metrics failure: {}", error);
                    }
                }
                errors.push(format!("{}: {}", configured.kind.as_str(), e));
            }
        }
    }

    warn!(
        "Connection #{}: all matching transports failed for {}",
        conn_id, target_addr
    );
    registry.update_route(
        conn_id,
        route_type_for_policy(&resolved_policy.action),
        Some(format!("Transport failure: {}", errors.join("; "))),
        Some(resolved_policy.action.as_label()),
    );
    stream.write_all(&socks::failure_reply()).await?;
    registry.close(conn_id);
    Ok(())
}

async fn copy_with_rate_limit<R, W>(
    reader: &mut R,
    writer: &mut W,
    limit_bps: Option<u64>,
    mut on_progress: impl FnMut(u64),
) -> std::io::Result<u64>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut total = 0u64;
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = tokio::io::AsyncReadExt::read(reader, &mut buf).await?;
        if n == 0 {
            tokio::io::AsyncWriteExt::shutdown(writer).await?;
            return Ok(total);
        }

        tokio::io::AsyncWriteExt::write_all(writer, &buf[..n]).await?;
        let copied = n as u64;
        total = total.saturating_add(copied);
        on_progress(copied);

        if let Some(limit_bps) = limit_bps {
            let delay_secs = (n as f64 / limit_bps as f64).max(0.0);
            if delay_secs > 0.0 {
                tokio::time::sleep(Duration::from_secs_f64(delay_secs)).await;
            }
        }
    }
}

fn rate_limit_bytes_per_sec(transport: &ConfiguredTransport, throttle_factor: f64) -> Option<u64> {
    if !transport.metadata.is_premium() || throttle_factor >= 0.999 {
        return None;
    }

    let advertised = transport.metadata.bandwidth_mbps.unwrap_or(16).max(1);
    let base_bytes_per_sec = advertised.saturating_mul(125_000);
    Some(
        ((base_bytes_per_sec as f64) * throttle_factor)
            .round()
            .max(16_384.0) as u64,
    )
}

fn order_candidate_transports(
    static_transports: Vec<ConfiguredTransport>,
    dynamic_transports: Vec<ConfiguredTransport>,
    credit_status: CreditRuntimeStatus,
    premium_better: bool,
) -> Vec<ConfiguredTransport> {
    let mut static_free = Vec::new();
    let mut discovered_free = Vec::new();
    let mut premium = Vec::new();

    for transport in static_transports.into_iter().chain(dynamic_transports) {
        if transport.metadata.is_premium() {
            premium.push(transport);
        } else if matches!(
            transport.metadata.source,
            transport::TransportSource::StaticConfig
        ) {
            static_free.push(transport);
        } else {
            discovered_free.push(transport);
        }
    }

    discovered_free.sort_by(|a, b| {
        b.metadata
            .reputation_score
            .partial_cmp(&a.metadata.reputation_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.metadata.bandwidth_mbps.cmp(&a.metadata.bandwidth_mbps))
    });
    premium.sort_by(|a, b| {
        b.metadata
            .reputation_score
            .partial_cmp(&a.metadata.reputation_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.metadata.bandwidth_mbps.cmp(&a.metadata.bandwidth_mbps))
    });

    let mut ordered = Vec::new();
    if credit_status.premium_allowed && premium_better && !credit_status.fallback_to_free {
        ordered.extend(premium);
        ordered.extend(static_free);
        ordered.extend(discovered_free);
    } else {
        ordered.extend(static_free);
        ordered.extend(discovered_free);
        ordered.extend(premium);
    }
    ordered
}

async fn do_direct(
    mut stream: TcpStream,
    target_addr: &str,
    conn_id: u64,
    registry: &Arc<ConnectionRegistry>,
) -> Result<()> {
    match TcpStream::connect(target_addr).await {
        Ok(outbound) => {
            stream.write_all(&socks::success_reply()).await?;
            let (mut ri, mut wi) = stream.into_split();
            let (mut ro, mut wo) = outbound.into_split();
            let c2t_registry = registry.clone();
            let c2t = copy_with_rate_limit(&mut ri, &mut wo, None, move |bytes| {
                c2t_registry.update_bytes(conn_id, bytes, 0);
            });
            let t2c_registry = registry.clone();
            let t2c = copy_with_rate_limit(&mut ro, &mut wi, None, move |bytes| {
                t2c_registry.update_bytes(conn_id, 0, bytes);
            });
            let (res_up, res_down) = tokio::join!(c2t, t2c);
            let _ = (res_up.unwrap_or(0), res_down.unwrap_or(0));
            registry.close(conn_id);
            Ok(())
        }
        Err(e) => {
            error!(
                "Connection #{}: direct connect to {} failed: {}",
                conn_id, target_addr, e
            );
            stream.write_all(&socks::failure_reply()).await?;
            registry.close(conn_id);
            Ok(())
        }
    }
}

#[derive(Clone)]
struct TransportPlan {
    transports: Vec<ConfiguredTransport>,
    proxy_required: bool,
    blocked: bool,
}

impl TransportPlan {
    fn direct() -> Self {
        Self {
            transports: Vec::new(),
            proxy_required: false,
            blocked: false,
        }
    }

    fn proxied(transports: Vec<ConfiguredTransport>) -> Self {
        Self {
            proxy_required: true,
            transports,
            blocked: false,
        }
    }

    fn blocked() -> Self {
        Self {
            transports: Vec::new(),
            proxy_required: false,
            blocked: true,
        }
    }

    fn requires_proxy(&self) -> bool {
        self.proxy_required
    }

    fn is_direct(&self) -> bool {
        !self.proxy_required && !self.blocked
    }

    fn is_blocked(&self) -> bool {
        self.blocked
    }
}

fn filter_transports_for_policy(
    transports: &[ConfiguredTransport],
    policy: &RoutePolicyAction,
) -> Vec<ConfiguredTransport> {
    match policy {
        RoutePolicyAction::Wss { profile_id } => transports
            .iter()
            .filter(|transport| {
                transport.kind == TransportKind::Wss
                    && transport.metadata.profile_id.as_deref() == Some(profile_id.as_str())
            })
            .cloned()
            .collect(),
        RoutePolicyAction::Vless { profile_id } => transports
            .iter()
            .filter(|transport| {
                transport.kind == TransportKind::Vless
                    && transport.metadata.profile_id.as_deref() == Some(profile_id.as_str())
            })
            .cloned()
            .collect(),
        _ => transports.to_vec(),
    }
}

fn route_type_for_transport(transport: &ConfiguredTransport) -> RouteType {
    match transport.kind {
        TransportKind::Wss => RouteType::Wss,
        TransportKind::Vless => RouteType::Vless,
    }
}

fn route_type_for_policy(policy: &RoutePolicyAction) -> RouteType {
    match policy {
        RoutePolicyAction::Vless { .. } => RouteType::Vless,
        RoutePolicyAction::Wss { .. } => RouteType::Wss,
        RoutePolicyAction::Block => RouteType::Blocked,
        _ => RouteType::Wss,
    }
}

fn select_transport_plan(
    _target: &str,
    is_telegram: bool,
    is_relay_infra: bool,
    proxy_mode: &str,
    resolved_policy: &ResolvedRoutePolicy,
    transports: &[ConfiguredTransport],
) -> TransportPlan {
    if matches!(resolved_policy.action, RoutePolicyAction::Block) {
        return TransportPlan::blocked();
    }

    if matches!(resolved_policy.action, RoutePolicyAction::Direct) {
        return TransportPlan::direct();
    }

    if matches!(
        resolved_policy.action,
        RoutePolicyAction::Wss { .. } | RoutePolicyAction::Vless { .. }
    ) {
        return TransportPlan::proxied(transports.to_vec());
    }

    if is_relay_infra || proxy_mode == "off" {
        return TransportPlan::direct();
    }

    let matches: Vec<ConfiguredTransport> = transports
        .iter()
        .filter(|transport| transport.mode_matches_target(is_telegram, proxy_mode))
        .cloned()
        .collect();

    match proxy_mode {
        "telegram" => {
            if is_telegram {
                TransportPlan::proxied(matches)
            } else {
                TransportPlan::direct()
            }
        }
        "full" => TransportPlan::proxied(matches),
        _ => TransportPlan::direct(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connections::{ConnectionGroup, ConnectionGroupKind};
    use crate::transport::{
        Transport, TransportKind, TransportMetadata, TransportSource, TransportStream,
    };
    use async_trait::async_trait;
    use hydra_config::TransportMode;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct DummyTransport;

    #[async_trait]
    impl Transport for DummyTransport {
        async fn connect(&self, _target: &str) -> Result<TransportStream> {
            Err(anyhow::anyhow!("unused"))
        }

        fn name(&self) -> &str {
            "dummy"
        }

        fn supports_udp(&self) -> bool {
            false
        }
    }

    fn configured(mode: TransportMode) -> ConfiguredTransport {
        ConfiguredTransport {
            kind: TransportKind::Wss,
            mode,
            transport: Arc::new(DummyTransport),
            metadata: TransportMetadata {
                source: TransportSource::StaticConfig,
                offer_id: None,
                agent_id: None,
                price_per_gb_micro_usdc: 0,
                stake_amount_micro_usdc: 0,
                bandwidth_mbps: None,
                reputation_score: 0.0,
                feedback_count: 0,
                created_at: None,
                endpoint_host: None,
                label: "dummy".to_string(),
                profile_id: None,
            },
        }
    }

    fn auto_policy() -> ResolvedRoutePolicy {
        ResolvedRoutePolicy {
            group: ConnectionGroup {
                group_kind: ConnectionGroupKind::Domain,
                group_key: "example.com".to_string(),
                app_label: None,
                package_name: None,
            },
            action: RoutePolicyAction::Auto,
        }
    }

    #[test]
    fn route_plan_off_is_direct() {
        let plan = select_transport_plan(
            "149.154.167.50:443",
            true,
            false,
            "off",
            &auto_policy(),
            &[configured(TransportMode::Telegram)],
        );
        assert!(plan.is_direct());
    }

    #[test]
    fn route_plan_telegram_target_uses_telegram_and_all() {
        let plan = select_transport_plan(
            "149.154.167.50:443",
            true,
            false,
            "telegram",
            &auto_policy(),
            &[
                configured(TransportMode::Telegram),
                configured(TransportMode::All),
            ],
        );
        assert!(plan.requires_proxy());
        assert_eq!(plan.transports.len(), 2);
    }

    #[test]
    fn route_plan_non_telegram_in_telegram_mode_is_direct() {
        let plan = select_transport_plan(
            "8.8.8.8:53",
            false,
            false,
            "telegram",
            &auto_policy(),
            &[configured(TransportMode::All)],
        );
        assert!(plan.is_direct());
    }

    #[test]
    fn route_plan_full_uses_only_all_for_non_telegram() {
        let plan = select_transport_plan(
            "8.8.8.8:53",
            false,
            false,
            "full",
            &auto_policy(),
            &[
                configured(TransportMode::Telegram),
                configured(TransportMode::All),
            ],
        );
        assert!(plan.requires_proxy());
        assert_eq!(plan.transports.len(), 1);
        assert_eq!(plan.transports[0].mode, TransportMode::All);
    }

    #[test]
    fn route_plan_relay_infra_is_always_direct() {
        let plan = select_transport_plan(
            "relay.hydra-net.work:443",
            false,
            true,
            "full",
            &auto_policy(),
            &[configured(TransportMode::All)],
        );
        assert!(plan.is_direct());
    }

    #[test]
    fn route_plan_full_without_matching_transports_is_fail_closed() {
        let plan = select_transport_plan(
            "8.8.8.8:53",
            false,
            false,
            "full",
            &auto_policy(),
            &[configured(TransportMode::Telegram)],
        );
        assert!(plan.requires_proxy());
        assert!(plan.transports.is_empty());
    }

    #[test]
    fn premium_routes_stay_last_without_credit_signal() {
        let static_transport = configured(TransportMode::Telegram);
        let mut premium = configured(TransportMode::All);
        premium.metadata.source = TransportSource::DiscoveredPremium;
        premium.metadata.price_per_gb_micro_usdc = 1_000_000;
        let ordered = order_candidate_transports(
            vec![static_transport.clone()],
            vec![premium.clone()],
            CreditRuntimeStatus {
                premium_allowed: false,
                throttle_factor: 1.0,
                fallback_to_free: false,
            },
            false,
        );
        assert_eq!(
            ordered.first().unwrap().metadata.source,
            TransportSource::StaticConfig
        );
        assert!(ordered.last().unwrap().metadata.is_premium());
    }

    #[tokio::test]
    async fn copy_with_rate_limit_reports_progress_before_close() {
        let payload = b"hello over live accounting";
        let (mut writer, mut reader) = tokio::io::duplex(128);
        let (mut sink_reader, mut sink_writer) = tokio::io::duplex(128);

        tokio::spawn(async move {
            writer.write_all(payload).await.unwrap();
            writer.shutdown().await.unwrap();
        });

        let mut seen = 0u64;
        let copied = copy_with_rate_limit(&mut reader, &mut sink_writer, None, |bytes| {
            seen += bytes;
        })
        .await
        .unwrap();

        let mut received = Vec::new();
        sink_reader.read_to_end(&mut received).await.unwrap();

        assert_eq!(copied, payload.len() as u64);
        assert_eq!(seen, payload.len() as u64);
        assert_eq!(received, payload);
    }
}
