use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_tungstenite::{connect_async, tungstenite};
use futures_util::{SinkExt, StreamExt};
use tracing::{debug, info, warn};

/// A relay connection through a Cloudflare Worker WSS endpoint.
/// The Worker accepts WebSocket, reads X-Hydra-Target header, and bridges to TCP.
pub struct RelayConnection {
    endpoints: Vec<String>,
    device_id: String,
}

impl RelayConnection {
    pub fn new(endpoints: Vec<String>, device_id: String) -> Self {
        Self { endpoints, device_id }
    }

    /// Connect to target through the first available relay endpoint.
    /// Returns a bidirectional stream (read half, write half) on success.
    pub async fn connect_to_target(
        &self,
        target: &str,
    ) -> Result<TcpStream> {
        for endpoint in &self.endpoints {
            match self.try_endpoint(endpoint, target).await {
                Ok(stream) => return Ok(stream),
                Err(e) => {
                    warn!("Relay endpoint {} failed: {}", endpoint, e);
                    continue;
                }
            }
        }
        Err(anyhow::anyhow!(
            "All relay endpoints exhausted for target {}",
            target
        ))
    }

    /// Connect through a specific relay endpoint using WebSocket.
    /// Returns a bridge TCP stream that pipes data through the WSS tunnel.
    async fn try_endpoint(&self, endpoint: &str, target: &str) -> Result<TcpStream> {
        info!("Connecting to relay {} for target {}", endpoint, target);

        let request = tungstenite::http::Request::builder()
            .uri(endpoint)
            .header("Upgrade", "websocket")
            .header("Connection", "Upgrade")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", tungstenite::handshake::client::generate_key())
            .header("Host", extract_host(endpoint))
            .header("X-Hydra-Target", target)
            .header("X-Hydra-Device", &self.device_id)
            .body(())
            .context("Failed to build WSS request")?;

        let (ws_stream, _response) = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            connect_async(request),
        )
        .await
        .map_err(|_| anyhow::anyhow!("WebSocket handshake timed out (10s)"))?
        .context("WebSocket handshake failed")?;

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
        let relay = RelayConnection::new(
            vec!["wss://nonexistent.invalid:9999".to_string()],
            "test".to_string(),
        );
        let result = relay.connect_to_target("127.0.0.1:80").await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("All relay endpoints exhausted"));
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
