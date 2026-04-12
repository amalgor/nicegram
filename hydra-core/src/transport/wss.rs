use super::{Transport, TransportStream};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use hickory_resolver::TokioResolver;
use hickory_resolver::config::ResolverConfig;
use hickory_resolver::name_server::TokioConnectionProvider;
use hickory_resolver::proto::rr::rdata::svcb::{SvcParamKey, SvcParamValue};
use hickory_resolver::proto::rr::{RData, RecordType};
use rustls::client::{EchConfig, EchMode};
use rustls::pki_types::{EchConfigListBytes, ServerName};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio_rustls::TlsConnector;
use tokio_tungstenite::tungstenite;
use tokio_tungstenite::{WebSocketStream, client_async};
use tracing::{debug, info, warn};
use url::Url;

const MAX_CONCURRENT_RELAY: usize = 4;
const DNS_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(4);
const TCP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const TLS_HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const WS_UPGRADE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
const ECH_RETRY_COOLDOWN: Duration = Duration::from_secs(600);

static RELAY_DNS_RESOLVER: OnceLock<TokioResolver> = OnceLock::new();
static ECH_DISABLED_UNTIL: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedEndpoint {
    host: String,
    port: u16,
    target_agent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayTlsReport {
    pub ech_configured: bool,
    pub ech_status: Option<String>,
}

impl RelayTlsReport {
    pub fn summary(&self) -> String {
        match (self.ech_configured, self.ech_status.as_deref()) {
            (true, Some("Accepted")) => "ECH accepted".to_string(),
            (true, Some(status)) => format!("ECH configured ({status})"),
            (true, None) => "ECH configured".to_string(),
            (false, _) => "standard TLS".to_string(),
        }
    }
}

pub struct WssTransport {
    endpoints: Vec<String>,
    device_id: String,
    semaphore: Arc<Semaphore>,
}

impl WssTransport {
    pub fn new(endpoints: Vec<String>, device_id: String) -> Self {
        Self {
            endpoints,
            device_id,
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_RELAY)),
        }
    }

    async fn connect_to_target(&self, target: &str) -> Result<TransportStream> {
        let _permit =
            tokio::time::timeout(std::time::Duration::from_secs(15), self.semaphore.acquire())
                .await
                .map_err(|_| anyhow::anyhow!("Relay queue full, timed out waiting for slot"))?
                .map_err(|_| anyhow::anyhow!("Relay semaphore closed"))?;

        let endpoints = self.endpoints.clone();
        let device_id = self.device_id.clone();
        let target_owned = target.to_string();

        let handle = tokio::spawn(async move {
            let transport = WssTransport {
                endpoints,
                device_id,
                semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_RELAY)),
            };
            for endpoint in &transport.endpoints {
                match transport.try_endpoint(endpoint, &target_owned).await {
                    Ok(stream) => return Ok(stream),
                    Err(error) => {
                        warn!("Relay endpoint {} failed: {}", endpoint, error);
                    }
                }
            }
            Err(anyhow::anyhow!(
                "All relay endpoints exhausted for target {}",
                target_owned
            ))
        });

        handle
            .await
            .map_err(|error| anyhow::anyhow!("Relay task panicked: {}", error))?
    }

    async fn try_endpoint(&self, endpoint: &str, target: &str) -> Result<TransportStream> {
        let parsed = parse_endpoint(endpoint)?;
        info!("Relay: [{}] connecting to {} ...", target, parsed.host);

        let mut request = tungstenite::http::Request::builder()
            .uri(endpoint)
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tungstenite::handshake::client::generate_key(),
            )
            .header("Host", &parsed.host)
            .header("X-Hydra-Target", target)
            .header("X-Hydra-Device", &self.device_id);

        if let Some(agent_id) = &parsed.target_agent {
            request = request
                .header("X-Hydra-Mode", "consumer")
                .header("X-Hydra-Target-Agent", agent_id);
        }

        let request = request.body(()).context("Failed to build WSS request")?;
        let (ws_stream, tls_report) = connect_relay_websocket(endpoint, request, target).await?;
        info!(
            "WSS relay connected to {} via {} using {}",
            target,
            endpoint,
            tls_report.summary()
        );

        let (local_stream, bridge_stream) = create_tcp_pair().await?;

        let target = target.to_string();
        tokio::spawn(async move {
            let (mut ws_sink, mut ws_source) = ws_stream.split();
            let (mut bridge_read, mut bridge_write) = bridge_stream.into_split();

            let ws_to_bridge = async {
                while let Some(msg) = ws_source.next().await {
                    match msg {
                        Ok(tungstenite::Message::Binary(data)) => {
                            if bridge_write.write_all(&data).await.is_err() {
                                warn!(
                                    "WSS relay [{}]: failed to write binary frame to bridge",
                                    target
                                );
                                break;
                            }
                        }
                        Ok(tungstenite::Message::Text(text)) => {
                            if text.contains("\"type\":\"error\"")
                                || text.contains("\"type\":\"closed\"")
                            {
                                warn!(
                                    "WSS relay [{}]: control frame from worker: {}",
                                    target, text
                                );
                                break;
                            }
                        }
                        Ok(tungstenite::Message::Close(frame)) => {
                            warn!(
                                "WSS relay [{}]: websocket closed by worker: {:?}",
                                target, frame
                            );
                            break;
                        }
                        Err(error) => {
                            warn!("WSS relay [{}]: websocket receive error: {}", target, error);
                            break;
                        }
                        _ => {}
                    }
                }
            };

            let bridge_to_ws = async {
                let mut buf = vec![0u8; 16384];
                loop {
                    match bridge_read.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if ws_sink
                                .send(tungstenite::Message::Binary(buf[..n].to_vec().into()))
                                .await
                                .is_err()
                            {
                                warn!("WSS relay [{}]: failed to send frame to worker", target);
                                break;
                            }
                        }
                        Err(error) => {
                            warn!("WSS relay [{}]: bridge read error: {}", target, error);
                            break;
                        }
                    }
                }
            };

            let finished = tokio::select! {
                _ = ws_to_bridge => "ws_to_bridge",
                _ = bridge_to_ws => "bridge_to_ws",
            };
            debug!("WSS relay [{}]: {} finished", target, finished);
            let _ = bridge_write.shutdown().await;
            let _ = ws_sink.close().await;
            debug!("WSS relay [{}]: bridge task shutdown complete", target);
        });

        Ok(Box::new(local_stream))
    }
}

pub async fn connect_relay_websocket(
    endpoint: &str,
    request: tungstenite::http::Request<()>,
    log_context: &str,
) -> Result<(
    WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>,
    RelayTlsReport,
)> {
    let parsed = parse_endpoint(endpoint)?;
    let (preferred_tls_config, ech_configured) =
        build_tls_client_config(&parsed.host, parsed.port).await?;
    let (tls_stream, tls_report) = match attempt_tls_handshake(
        &parsed.host,
        parsed.port,
        log_context,
        preferred_tls_config,
        ech_configured,
    )
    .await
    {
        Ok(result) => result,
        Err(error) if ech_configured => {
            record_ech_handshake_failure();
            warn!(
                "Relay: [{}] ECH-enabled TLS handshake failed for {}:{}: {}. Retrying with standard TLS and disabling ECH for {:?}.",
                log_context, parsed.host, parsed.port, error, ECH_RETRY_COOLDOWN
            );
            attempt_tls_handshake(
                &parsed.host,
                parsed.port,
                log_context,
                Arc::new(build_standard_tls_client_config()?),
                false,
            )
            .await?
        }
        Err(error) => return Err(error),
    };

    let ws_result = tokio::time::timeout(WS_UPGRADE_TIMEOUT, client_async(request, tls_stream))
        .await
        .map_err(|_| {
            warn!(
                "Relay: [{}] WS upgrade timeout ({:?})",
                log_context, WS_UPGRADE_TIMEOUT
            );
            anyhow::anyhow!("WebSocket upgrade timeout")
        })?
        .context("WebSocket handshake failed")?;

    let (ws_stream, _response) = ws_result;

    Ok((ws_stream, tls_report))
}

fn relay_dns_resolver() -> &'static TokioResolver {
    RELAY_DNS_RESOLVER.get_or_init(|| {
        TokioResolver::builder_with_config(
            ResolverConfig::cloudflare_https(),
            TokioConnectionProvider::default(),
        )
        .build()
    })
}

fn ech_disabled_until() -> &'static Mutex<Option<Instant>> {
    ECH_DISABLED_UNTIL.get_or_init(|| Mutex::new(None))
}

fn should_attempt_ech() -> bool {
    let mut disabled_until = ech_disabled_until()
        .lock()
        .expect("ECH cooldown mutex poisoned");
    match *disabled_until {
        Some(until) if until > Instant::now() => false,
        Some(_) => {
            *disabled_until = None;
            true
        }
        None => true,
    }
}

fn record_ech_handshake_failure() {
    let mut disabled_until = ech_disabled_until()
        .lock()
        .expect("ECH cooldown mutex poisoned");
    *disabled_until = Some(Instant::now() + ECH_RETRY_COOLDOWN);
}

fn clear_ech_cooldown() {
    let mut disabled_until = ech_disabled_until()
        .lock()
        .expect("ECH cooldown mutex poisoned");
    *disabled_until = None;
}

async fn connect_relay_tcp_stream(host: &str, port: u16, log_context: &str) -> Result<TcpStream> {
    let resolver = relay_dns_resolver();
    let mut last_error = None;

    match tokio::time::timeout(DNS_LOOKUP_TIMEOUT, resolver.lookup_ip(host.to_string())).await {
        Ok(Ok(lookup)) => {
            for ip in lookup.iter() {
                let socket_addr = SocketAddr::new(ip, port);
                match tokio::time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(socket_addr))
                    .await
                {
                    Ok(Ok(stream)) => {
                        debug!(
                            "Relay: [{}] connected to {} via Cloudflare DoH IP {}",
                            log_context, host, socket_addr
                        );
                        return Ok(stream);
                    }
                    Ok(Err(error)) => {
                        last_error = Some(anyhow::anyhow!(
                            "DoH TCP connect to {} failed: {}",
                            socket_addr,
                            error
                        ));
                    }
                    Err(_) => {
                        last_error = Some(anyhow::anyhow!(
                            "DoH TCP connect timeout to {} after {:?}",
                            socket_addr,
                            TCP_CONNECT_TIMEOUT
                        ));
                    }
                }
            }
        }
        Ok(Err(error)) => {
            warn!(
                "Relay: [{}] Cloudflare DoH IP lookup failed for {}: {}. Falling back to system DNS.",
                log_context, host, error
            );
        }
        Err(_) => {
            warn!(
                "Relay: [{}] Cloudflare DoH IP lookup timed out for {} after {:?}. Falling back to system DNS.",
                log_context, host, DNS_LOOKUP_TIMEOUT
            );
        }
    }

    let tcp_addr = format!("{host}:{port}");
    tokio::time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(&tcp_addr))
        .await
        .map_err(|_| {
            warn!(
                "Relay: [{}] TCP timeout ({:?}) to {}",
                log_context, TCP_CONNECT_TIMEOUT, tcp_addr
            );
            anyhow::anyhow!("TCP connect timeout")
        })?
        .with_context(|| {
            if let Some(error) = last_error {
                format!("TCP connect failed after DoH attempts also failed: {error}")
            } else {
                format!("TCP connect failed to {tcp_addr}")
            }
        })
}

async fn build_tls_client_config(
    host: &str,
    port: u16,
) -> Result<(Arc<rustls::ClientConfig>, bool)> {
    if !should_attempt_ech() {
        debug!(
            "Relay: ECH temporarily disabled for {}:{} after recent handshake failures; using standard TLS.",
            host, port
        );
        return Ok((Arc::new(build_standard_tls_client_config()?), false));
    }

    match lookup_ech_config_lists(host, port).await {
        Ok(config_lists) => {
            if let Some(ech_mode) = select_ech_mode(config_lists) {
                let config = rustls::ClientConfig::builder_with_provider(Arc::new(
                    rustls::crypto::aws_lc_rs::default_provider(),
                ))
                .with_ech(ech_mode)
                .context("Failed to enable ECH for relay TLS")?
                .with_root_certificates(default_root_store())
                .with_no_client_auth();
                return Ok((Arc::new(config), true));
            }
            debug!("Relay: no compatible ECH config found for {}", host);
        }
        Err(error) => {
            warn!(
                "Relay: failed to resolve ECH config for {}:{}: {}. Falling back to standard TLS.",
                host, port, error
            );
        }
    }

    Ok((Arc::new(build_standard_tls_client_config()?), false))
}

fn default_root_store() -> rustls::RootCertStore {
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    root_store
}

fn build_standard_tls_client_config() -> Result<rustls::ClientConfig> {
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .context("Failed to set TLS protocol versions")?
    .with_root_certificates(default_root_store())
    .with_no_client_auth();
    Ok(config)
}

async fn attempt_tls_handshake(
    host: &str,
    port: u16,
    log_context: &str,
    tls_config: Arc<rustls::ClientConfig>,
    ech_configured: bool,
) -> Result<(tokio_rustls::client::TlsStream<TcpStream>, RelayTlsReport)> {
    let tcp_stream = connect_relay_tcp_stream(host, port, log_context).await?;
    let server_name = ServerName::try_from(host.to_string())
        .map_err(|error| anyhow::anyhow!("Invalid server name '{}': {}", host, error))?;
    let tls_connector = TlsConnector::from(tls_config);

    let tls_stream = tokio::time::timeout(
        TLS_HANDSHAKE_TIMEOUT,
        tls_connector.connect(server_name, tcp_stream),
    )
    .await
    .map_err(|_| {
        warn!(
            "Relay: [{}] TLS handshake timeout ({:?})",
            log_context, TLS_HANDSHAKE_TIMEOUT
        );
        anyhow::anyhow!("TLS handshake timeout")
    })?
    .context("TLS handshake failed")?;

    let ech_status = if ech_configured {
        Some(format!("{:?}", tls_stream.get_ref().1.ech_status()))
    } else {
        Some("Disabled".to_string())
    };
    if matches!(ech_status.as_deref(), Some("Accepted")) {
        clear_ech_cooldown();
    }
    debug!(
        "Relay: [{}] TLS handshake completed for {} with {}",
        log_context,
        host,
        match ech_status.as_deref() {
            Some(status) if ech_configured => format!("ECH status {status}"),
            _ => "standard TLS".to_string(),
        }
    );

    Ok((
        tls_stream,
        RelayTlsReport {
            ech_configured,
            ech_status,
        },
    ))
}

fn select_ech_mode(config_lists: Vec<Vec<u8>>) -> Option<EchMode> {
    config_lists.into_iter().find_map(|config_list| {
        EchConfig::new(
            EchConfigListBytes::from(config_list),
            rustls::crypto::aws_lc_rs::hpke::ALL_SUPPORTED_SUITES,
        )
        .map(EchMode::from)
        .ok()
    })
}

async fn lookup_ech_config_lists(host: &str, port: u16) -> Result<Vec<Vec<u8>>> {
    let resolver = relay_dns_resolver();
    let lookup_name = https_record_lookup_name(host, port);
    let lookup = tokio::time::timeout(
        DNS_LOOKUP_TIMEOUT,
        resolver.lookup(lookup_name.clone(), RecordType::HTTPS),
    )
    .await
    .map_err(|_| anyhow::anyhow!("ECH DNS lookup timed out after {:?}", DNS_LOOKUP_TIMEOUT))?
    .with_context(|| format!("ECH DNS lookup failed for {lookup_name}"))?;

    let mut config_lists = Vec::new();
    for record in lookup.record_iter() {
        let RData::HTTPS(svcb) = record.data() else {
            continue;
        };

        config_lists.extend(svcb.svc_params().iter().filter_map(|param| match param {
            (SvcParamKey::EchConfigList, SvcParamValue::EchConfigList(config_list)) => {
                Some(config_list.clone().0)
            }
            _ => None,
        }));
    }

    Ok(config_lists)
}

fn https_record_lookup_name(host: &str, port: u16) -> String {
    match port {
        443 => host.to_string(),
        port => format!("_{port}._https.{host}"),
    }
}

#[async_trait]
impl Transport for WssTransport {
    async fn connect(&self, target: &str) -> Result<TransportStream> {
        self.connect_to_target(target).await
    }

    fn name(&self) -> &str {
        "wss"
    }

    fn supports_udp(&self) -> bool {
        false
    }
}

pub fn extract_host(url: &str) -> String {
    Url::parse(url)
        .ok()
        .and_then(|parsed| {
            let host = parsed.host_str()?.to_string();
            Some(match parsed.port() {
                Some(port) => format!("{host}:{port}"),
                None => host,
            })
        })
        .unwrap_or_else(|| {
            url.replace("wss://", "")
                .replace("ws://", "")
                .split('/')
                .next()
                .unwrap_or("localhost")
                .split('?')
                .next()
                .unwrap_or("localhost")
                .to_string()
        })
}

fn parse_endpoint(url: &str) -> Result<ParsedEndpoint> {
    let parsed = Url::parse(url).context("Invalid WSS endpoint URL")?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing host in WSS endpoint"))?
        .to_string();
    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| anyhow::anyhow!("Missing port in WSS endpoint"))?;
    let target_agent = parsed
        .query_pairs()
        .find(|(key, _)| key == "agent")
        .map(|(_, value)| value.trim().to_string())
        .filter(|value| !value.is_empty());

    Ok(ParsedEndpoint {
        host,
        port,
        target_agent,
    })
}

async fn create_tcp_pair() -> Result<(TcpStream, TcpStream)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let connect_fut = TcpStream::connect(addr);
    let accept_fut = listener.accept();
    let (client, (server, _)) = tokio::try_join!(connect_fut, accept_fut)?;
    Ok((client, server))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn test_extract_host_wss() {
        assert_eq!(
            extract_host("wss://relay.hydra-net.work"),
            "relay.hydra-net.work"
        );
        assert_eq!(
            extract_host("wss://relay.hydra-net.work/"),
            "relay.hydra-net.work"
        );
        assert_eq!(
            extract_host("wss://relay.hydra-net.work/path"),
            "relay.hydra-net.work"
        );
        assert_eq!(
            extract_host("wss://relay.hydra-net.work?agent=3377"),
            "relay.hydra-net.work"
        );
    }

    #[test]
    fn test_extract_host_ws() {
        assert_eq!(extract_host("ws://localhost:8080"), "localhost:8080");
        assert_eq!(extract_host("ws://127.0.0.1:9000/ws"), "127.0.0.1:9000");
    }

    #[test]
    fn test_extract_host_no_scheme() {
        assert_eq!(extract_host("example.com"), "example.com");
        assert_eq!(extract_host("example.com/path"), "example.com");
    }

    #[test]
    fn test_extract_host_empty() {
        assert_eq!(extract_host(""), "");
    }

    #[test]
    fn test_parse_endpoint_keeps_agent_and_port() {
        let parsed = parse_endpoint("wss://relay.hydra-net.work:7443/ws?agent=12").unwrap();
        assert_eq!(parsed.host, "relay.hydra-net.work");
        assert_eq!(parsed.port, 7443);
        assert_eq!(parsed.target_agent.as_deref(), Some("12"));
    }

    #[test]
    fn test_https_record_lookup_name_uses_port_prefix_for_non_default_port() {
        assert_eq!(
            https_record_lookup_name("relay.hydra-net.work", 7443),
            "_7443._https.relay.hydra-net.work"
        );
        assert_eq!(
            https_record_lookup_name("relay.hydra-net.work", 443),
            "relay.hydra-net.work"
        );
    }

    #[test]
    fn test_select_ech_mode_from_cloudflare_sample() {
        let encoded = "AEX+DQBBfgAgACCbfSbeknM4CVoitD1Dka1jBzH/rqXyFJ7kJWvR3CgoTAAEAAEAAQASY2xvdWRmbGFyZS1lY2guY29tAAA=";
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .expect("ECH config sample must decode");
        assert!(select_ech_mode(vec![decoded]).is_some());
    }

    #[test]
    fn test_wss_transport_new() {
        let relay = WssTransport::new(
            vec![
                "wss://a.example.com".to_string(),
                "wss://b.example.com".to_string(),
            ],
            "test-device".to_string(),
        );
        assert_eq!(relay.endpoints.len(), 2);
        assert_eq!(relay.device_id, "test-device");
    }

    #[test]
    fn test_wss_transport_empty_endpoints() {
        let relay = WssTransport::new(vec![], "dev".to_string());
        assert!(relay.endpoints.is_empty());
    }

    #[tokio::test]
    async fn test_create_tcp_pair() {
        let (mut a, mut b) = create_tcp_pair().await.expect("tcp pair creation failed");

        a.write_all(b"hello").await.unwrap();
        let mut buf = [0u8; 5];
        b.read_exact(&mut buf).await.unwrap();
        assert_eq!(&buf, b"hello");

        b.write_all(b"world").await.unwrap();
        let mut buf2 = [0u8; 5];
        a.read_exact(&mut buf2).await.unwrap();
        assert_eq!(&buf2, b"world");
    }

    #[tokio::test]
    async fn test_relay_all_endpoints_exhausted() {
        let relay = Arc::new(WssTransport::new(
            vec!["wss://nonexistent.invalid:9999".to_string()],
            "test".to_string(),
        ));
        let result = relay.connect_to_target("127.0.0.1:80").await;
        assert!(result.is_err());
    }
}
