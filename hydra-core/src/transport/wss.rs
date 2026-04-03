use super::{Transport, TransportStream};
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite;
use tracing::{debug, info, warn};
use url::Url;

fn build_tls_connector() -> Result<tokio_rustls::TlsConnector> {
    let provider = rustls::crypto::ring::default_provider();
    let mut root_store = rustls::RootCertStore::empty();
    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(provider))
        .with_safe_default_protocol_versions()
        .context("Failed to set TLS protocol versions")?
        .with_root_certificates(root_store)
        .with_no_client_auth();
    Ok(tokio_rustls::TlsConnector::from(Arc::new(config)))
}

const MAX_CONCURRENT_RELAY: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedEndpoint {
    host: String,
    port: u16,
    target_agent: Option<String>,
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
        let _permit = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.semaphore.acquire(),
        )
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

        let tcp_addr = format!("{}:{}", parsed.host, parsed.port);
        let tcp_stream = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            TcpStream::connect(&tcp_addr),
        )
        .await
        .map_err(|_| {
            warn!("Relay: [{}] TCP timeout (5s) to {}", target, tcp_addr);
            anyhow::anyhow!("TCP connect timeout")
        })?
        .context("TCP connect failed")?;

        let tls_connector = build_tls_connector()?;
        let server_name = rustls::pki_types::ServerName::try_from(parsed.host.as_str())
            .map_err(|error| anyhow::anyhow!("Invalid server name '{}': {}", parsed.host, error))?
            .to_owned();

        let tls_stream = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tls_connector.connect(server_name, tcp_stream),
        )
        .await
        .map_err(|_| {
            warn!("Relay: [{}] TLS timeout (5s)", target);
            anyhow::anyhow!("TLS handshake timeout")
        })?
        .context("TLS handshake failed")?;

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

        let ws_result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio_tungstenite::client_async(request, tls_stream),
        )
        .await
        .map_err(|_| {
            warn!("Relay: [{}] WS upgrade timeout (5s)", target);
            anyhow::anyhow!("WebSocket upgrade timeout")
        })?
        .context("WebSocket handshake failed")?;

        let (ws_stream, _response) = ws_result;
        info!("WSS relay connected to {} via {}", target, endpoint);

        let (local_stream, bridge_stream) = create_tcp_pair().await?;

        tokio::spawn(async move {
            let (mut ws_sink, mut ws_source) = ws_stream.split();
            let (mut bridge_read, mut bridge_write) = bridge_stream.into_split();

            let ws_to_bridge = async {
                while let Some(msg) = ws_source.next().await {
                    match msg {
                        Ok(tungstenite::Message::Binary(data)) => {
                            if bridge_write.write_all(&data).await.is_err() {
                                break;
                            }
                        }
                        Ok(tungstenite::Message::Text(text)) => {
                            if text.contains("\"type\":\"error\"")
                                || text.contains("\"type\":\"closed\"")
                            {
                                break;
                            }
                        }
                        Ok(tungstenite::Message::Close(_)) | Err(_) => break,
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
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            };

            tokio::select! {
                _ = ws_to_bridge => debug!("WS->bridge finished"),
                _ = bridge_to_ws => debug!("bridge->WS finished"),
            }
        });

        Ok(Box::new(local_stream))
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

    #[test]
    fn test_extract_host_wss() {
        assert_eq!(extract_host("wss://relay.hydra-net.work"), "relay.hydra-net.work");
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
    fn test_wss_transport_new() {
        let relay = WssTransport::new(
            vec!["wss://a.example.com".to_string(), "wss://b.example.com".to_string()],
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
