use anyhow::{Context, Result};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Semaphore;
use tokio_tungstenite::tungstenite;
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, info, warn};

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

/// Max concurrent WSS relay handshakes to prevent tokio thread starvation.
/// DNS resolution via getaddrinfo is blocking and can exhaust the thread pool.
const MAX_CONCURRENT_RELAY: usize = 4;

/// A relay connection through a Cloudflare Worker WSS endpoint.
/// The Worker accepts WebSocket, reads X-Hydra-Target header, and bridges to TCP.
pub struct RelayConnection {
    endpoints: Vec<String>,
    device_id: String,
    semaphore: Semaphore,
}

impl RelayConnection {
    pub fn new(endpoints: Vec<String>, device_id: String) -> Self {
        Self {
            endpoints,
            device_id,
            semaphore: Semaphore::new(MAX_CONCURRENT_RELAY),
        }
    }

    /// Connect to target through the first available relay endpoint.
    /// Uses a semaphore to limit concurrent handshakes.
    /// Runs in a spawned task to ensure timeouts fire even under load.
    pub async fn connect_to_target(
        self: &Arc<Self>,
        target: &str,
    ) -> Result<TcpStream> {
        let _permit = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            self.semaphore.acquire(),
        )
        .await
        .map_err(|_| anyhow::anyhow!("Relay queue full, timed out waiting for slot"))?
        .map_err(|_| anyhow::anyhow!("Relay semaphore closed"))?;

        let this = Arc::clone(self);
        let target_owned = target.to_string();

        // Spawn in a separate task so tokio timers can fire independently
        let handle = tokio::spawn(async move {
            for endpoint in &this.endpoints {
                match this.try_endpoint(endpoint, &target_owned).await {
                    Ok(stream) => return Ok(stream),
                    Err(e) => {
                        warn!("Relay endpoint {} failed: {}", endpoint, e);
                        continue;
                    }
                }
            }
            Err(anyhow::anyhow!(
                "All relay endpoints exhausted for target {}",
                target_owned
            ))
        });

        handle.await.map_err(|e| anyhow::anyhow!("Relay task panicked: {}", e))?
    }

    /// Connect through a specific relay endpoint using WebSocket.
    /// Does manual DNS -> TCP -> TLS -> WS handshake for full timeout control on Android.
    async fn try_endpoint(&self, endpoint: &str, target: &str) -> Result<TcpStream> {
        let host = extract_host(endpoint);
        info!("Relay: [{}] connecting to {} ...", target, host);

        // Step 1: TCP connect with explicit timeout
        let tcp_addr = format!("{}:443", host);
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

        debug!("Relay: [{}] TCP connected to {}", target, tcp_addr);

        // Step 2: TLS handshake
        let tls_connector = build_tls_connector()?;
        let server_name = rustls::pki_types::ServerName::try_from(host.as_str())
            .map_err(|e| anyhow::anyhow!("Invalid server name '{}': {}", host, e))?
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

        debug!("Relay: [{}] TLS established", target);

        // Step 3: WebSocket upgrade over TLS stream
        let request = tungstenite::http::Request::builder()
            .uri(endpoint)
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
            .header("Host", &host)
            .header("X-Hydra-Target", target)
            .header("X-Hydra-Device", &self.device_id)
            .body(())
            .context("Failed to build WSS request")?;

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

        // Create a local TCP pair to bridge: one end for the SOCKS5 handler,
        // the other end pumps data through the WebSocket.
        let (local_stream, bridge_stream) = create_tcp_pair().await?;

        // Spawn bidirectional pump: bridge_stream <-> ws_stream
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

        Ok(local_stream)
    }
}

pub fn extract_host(url: &str) -> String {
    url.replace("wss://", "")
        .replace("ws://", "")
        .split('/')
        .next()
        .unwrap_or("localhost")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_host_wss() {
        assert_eq!(extract_host("wss://relay.hydra-net.work"), "relay.hydra-net.work");
        assert_eq!(extract_host("wss://relay.hydra-net.work/"), "relay.hydra-net.work");
        assert_eq!(extract_host("wss://relay.hydra-net.work/path"), "relay.hydra-net.work");
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
    fn test_relay_connection_new() {
        let relay = RelayConnection::new(
            vec!["wss://a.example.com".to_string(), "wss://b.example.com".to_string()],
            "test-device".to_string(),
        );
        assert_eq!(relay.endpoints.len(), 2);
        assert_eq!(relay.device_id, "test-device");
    }

    #[test]
    fn test_relay_connection_empty_endpoints() {
        let relay = RelayConnection::new(vec![], "dev".to_string());
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
        let relay = Arc::new(RelayConnection::new(
            vec!["wss://nonexistent.invalid:9999".to_string()],
            "test".to_string(),
        ));
        let result = relay.connect_to_target("127.0.0.1:80").await;
        assert!(result.is_err());
    }
}

/// Create a connected TCP pair using a loopback listener.
async fn create_tcp_pair() -> Result<(TcpStream, TcpStream)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let connect_fut = TcpStream::connect(addr);
    let accept_fut = listener.accept();
    let (client, (server, _)) = tokio::try_join!(connect_fut, accept_fut)?;
    Ok((client, server))
}
