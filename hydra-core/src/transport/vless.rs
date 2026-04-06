use super::{Transport, TransportStream};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use leaf::app::{SyncDnsClient, dns_client::DnsClient};
use leaf::proxy::outbound::HandlerBuilder;
use leaf::proxy::{self, AnyOutboundHandler, AnyStream, OutboundConnect, OutboundStreamHandler};
use leaf::session::{Network, Session, SocksAddr};
use protobuf::MessageField;
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::sync::RwLock;
use uuid::Uuid;
use vpn_link_serde::{Protocol, VLess};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlessConfig {
    pub uuid: String,
    pub address: String,
    pub port: u16,
    pub flow: Option<String>,
    pub security: String,
    pub server_name: String,
    pub public_key: String,
    pub short_id: String,
    pub fingerprint: String,
    pub network: String,
    pub host: Option<String>,
    pub path: Option<String>,
}

impl VlessConfig {
    fn requires_reality(&self) -> bool {
        self.security == "reality"
    }

    fn uses_vision(&self) -> bool {
        self.flow.as_deref() == Some("xtls-rprx-vision")
    }
}

impl TryFrom<&str> for VlessConfig {
    type Error = anyhow::Error;

    fn try_from(url: &str) -> Result<Self> {
        let protocol = Protocol::parse(url).map_err(|e| anyhow!("invalid vless url: {}", e))?;
        let vless = match protocol {
            Protocol::VLess(vless) => vless,
            _ => return Err(anyhow!("expected vless:// URL")),
        };
        Self::try_from(vless)
    }
}

impl TryFrom<VLess> for VlessConfig {
    type Error = anyhow::Error;

    fn try_from(vless: VLess) -> Result<Self> {
        let cfg = vless.config;

        anyhow::ensure!(!cfg.id.trim().is_empty(), "vless url is missing uuid");
        anyhow::ensure!(
            !cfg.address.trim().is_empty(),
            "vless url is missing address"
        );
        anyhow::ensure!(cfg.port > 0, "vless url is missing port");

        let security = cfg.security.unwrap_or_else(|| "none".to_string());
        anyhow::ensure!(
            security == "reality" || security == "none",
            "unsupported vless security: {}",
            security
        );

        let network = cfg.r#type.unwrap_or_else(|| "tcp".to_string());
        anyhow::ensure!(
            matches!(network.as_str(), "tcp" | "ws" | "grpc"),
            "unsupported vless network type: {}",
            network
        );

        if let Some(flow) = cfg.flow.as_ref() {
            anyhow::ensure!(
                flow == "xtls-rprx-vision",
                "unsupported vless flow: {}",
                flow
            );
        }

        let server_name = cfg
            .sni
            .clone()
            .or_else(|| cfg.host.clone())
            .unwrap_or_else(|| cfg.address.clone());
        let public_key = cfg.pbk.clone().unwrap_or_default();
        let short_id = cfg.sid.clone().unwrap_or_default();
        let fingerprint = cfg.fp.clone().unwrap_or_default();

        if security == "reality" {
            anyhow::ensure!(
                !server_name.trim().is_empty(),
                "reality vless url is missing sni/host"
            );
            anyhow::ensure!(
                !public_key.trim().is_empty(),
                "reality vless url is missing pbk"
            );
        }

        Ok(Self {
            uuid: cfg.id,
            address: cfg.address,
            port: cfg.port,
            flow: cfg.flow,
            security,
            server_name,
            public_key,
            short_id,
            fingerprint,
            network,
            host: cfg.host,
            path: cfg.path,
        })
    }
}

pub struct VlessTransport {
    config: VlessConfig,
    dns_client: SyncDnsClient,
    handler: Option<AnyOutboundHandler>,
}

struct HydraVlessStreamHandler {
    address: String,
    port: u16,
    uuid: String,
    uses_vision: bool,
}

#[async_trait]
impl OutboundStreamHandler for HydraVlessStreamHandler {
    fn connect_addr(&self) -> OutboundConnect {
        OutboundConnect::Proxy(Network::Tcp, self.address.clone(), self.port)
    }

    async fn handle<'a>(
        &'a self,
        sess: &'a Session,
        _lhs: Option<&mut AnyStream>,
        stream: Option<AnyStream>,
    ) -> io::Result<AnyStream> {
        let uuid = Uuid::parse_str(&self.uuid)
            .map_err(|error| io::Error::other(format!("parse uuid failed: {error}")))?;
        let uuid_bytes = *uuid.as_bytes();

        let addr_type = match sess.destination.ip() {
            Some(ip) if ip.is_ipv4() => 1,
            Some(_) => 3,
            None => 2,
        };
        let host = sess.destination.host();
        let port = sess.destination.port();

        let header = build_vless_tcp_header(&uuid_bytes, &host, port, addr_type, self.uses_vision);

        let mut stream = stream.ok_or_else(|| io::Error::other("invalid input"))?;
        stream.write_all(&header).await?;

        if self.uses_vision {
            Ok(Box::new(leaf::proxy::vless::VlessStream::new(
                stream,
                uuid_bytes,
                Some(sess.vision_read_raw.clone()),
            )))
        } else {
            Ok(Box::new(StandardVlessStream::new(stream)))
        }
    }
}

struct StandardVlessStream<S> {
    stream: S,
    response_prefix: Vec<u8>,
    plaintext_buffer: Vec<u8>,
    response_header_complete: bool,
}

impl<S: AsyncRead + AsyncWrite + Unpin> StandardVlessStream<S> {
    fn new(stream: S) -> Self {
        Self {
            stream,
            response_prefix: Vec::new(),
            plaintext_buffer: Vec::new(),
            response_header_complete: false,
        }
    }

    fn strip_response_header(&mut self, data: &[u8]) -> Vec<u8> {
        if self.response_header_complete {
            return data.to_vec();
        }

        self.response_prefix.extend_from_slice(data);
        if self.response_prefix.len() < 2 {
            return Vec::new();
        }

        let addons_len = self.response_prefix[1] as usize;
        let header_len = 2 + addons_len;
        if self.response_prefix.len() < header_len {
            return Vec::new();
        }

        self.response_header_complete = true;
        let payload = self.response_prefix.split_off(header_len);
        self.response_prefix.clear();
        payload
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncRead for StandardVlessStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();

        loop {
            if !this.plaintext_buffer.is_empty() {
                let len = std::cmp::min(buf.remaining(), this.plaintext_buffer.len());
                buf.put_slice(&this.plaintext_buffer[..len]);
                this.plaintext_buffer.drain(..len);
                return Poll::Ready(Ok(()));
            }

            let mut temp = [0u8; 8192];
            let mut inner = ReadBuf::new(&mut temp);
            match Pin::new(&mut this.stream).poll_read(cx, &mut inner) {
                Poll::Ready(Ok(())) => {
                    let bytes_read = inner.filled().len();
                    if bytes_read == 0 {
                        return Poll::Ready(Ok(()));
                    }

                    let payload = this.strip_response_header(&temp[..bytes_read]);
                    if payload.is_empty() {
                        continue;
                    }

                    let len = std::cmp::min(buf.remaining(), payload.len());
                    buf.put_slice(&payload[..len]);
                    if payload.len() > len {
                        this.plaintext_buffer.extend_from_slice(&payload[len..]);
                    }
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl<S: AsyncRead + AsyncWrite + Unpin> AsyncWrite for StandardVlessStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

impl VlessTransport {
    pub fn from_url(url: &str) -> Result<Self> {
        let config = VlessConfig::try_from(url)?;
        Self::new(config)
    }

    pub fn new(config: VlessConfig) -> Result<Self> {
        let dns_client = Arc::new(RwLock::new(new_dns_client()?));
        let handler = build_leaf_handler(&config, dns_client.clone())?;
        Ok(Self {
            config,
            dns_client,
            handler,
        })
    }
}

#[async_trait]
impl Transport for VlessTransport {
    async fn connect(&self, target: &str) -> Result<TransportStream> {
        let handler = self
            .handler
            .as_ref()
            .ok_or_else(|| anyhow!("vless network {} is not supported yet", self.config.network))?;

        let session = Session {
            network: Network::Tcp,
            destination: target_to_socks_addr(target)?,
            inbound_tag: "hydra".to_string(),
            outbound_tag: "hydra-vless".to_string(),
            ..Default::default()
        };

        let stream = proxy::connect_stream_outbound(&session, self.dns_client.clone(), handler)
            .await
            .map_err(|e| anyhow!("leaf outbound connect failed: {}", e))?
            .ok_or_else(|| anyhow!("leaf outbound did not return a stream"))?;

        let stream = handler
            .stream()
            .map_err(|e| anyhow!("leaf outbound has no stream handler: {}", e))?
            .handle(&session, None, Some(stream))
            .await
            .map_err(|e| anyhow!("leaf outbound handler failed: {}", e))?;

        Ok(stream)
    }

    fn name(&self) -> &str {
        "vless"
    }

    fn supports_udp(&self) -> bool {
        false
    }
}

fn build_leaf_handler(
    config: &VlessConfig,
    _dns_client: SyncDnsClient,
) -> Result<Option<AnyOutboundHandler>> {
    let mut actors: Vec<AnyOutboundHandler> = Vec::new();

    if config.requires_reality() {
        let reality = Arc::new(leaf::proxy::reality::outbound::StreamHandler {
            server_name: config.server_name.clone(),
            public_key: config.public_key.clone(),
            short_id: config.short_id.clone(),
        });
        actors.push(
            HandlerBuilder::default()
                .tag("hydra-reality".to_string())
                .stream_handler(reality)
                .build(),
        );
    }

    match config.network.as_str() {
        "tcp" => {}
        "ws" => {
            let mut headers = HashMap::new();
            if let Some(host) = config.host.as_ref() {
                headers.insert("Host".to_string(), host.clone());
            }
            let ws = Arc::new(leaf::proxy::ws::outbound::StreamHandler {
                path: config.path.clone().unwrap_or_else(|| "/".to_string()),
                headers,
            });
            actors.push(
                HandlerBuilder::default()
                    .tag("hydra-ws".to_string())
                    .stream_handler(ws)
                    .build(),
            );
        }
        "grpc" => return Ok(None),
        other => return Err(anyhow!("unsupported vless network type: {}", other)),
    }

    let vless = Arc::new(HydraVlessStreamHandler {
        address: config.address.clone(),
        port: config.port,
        uuid: config.uuid.clone(),
        uses_vision: config.uses_vision(),
    });
    actors.push(
        HandlerBuilder::default()
            .tag("hydra-vless".to_string())
            .stream_handler(vless)
            .build(),
    );

    if actors.len() == 1 {
        return Ok(Some(actors.remove(0)));
    }

    let chain = Arc::new(leaf::proxy::chain::outbound::StreamHandler { actors });
    Ok(Some(
        HandlerBuilder::default()
            .tag("hydra-vless-chain".to_string())
            .stream_handler(chain)
            .build(),
    ))
}

fn new_dns_client() -> Result<DnsClient> {
    let mut dns = leaf::config::Dns::new();
    dns.servers.push("system".to_string());
    DnsClient::new(&MessageField::some(dns))
        .map_err(|e| anyhow!("failed to initialize leaf dns client: {}", e))
}

fn target_to_socks_addr(target: &str) -> Result<SocksAddr> {
    let (host, port) = crate::socks::split_target(target)
        .ok_or_else(|| anyhow!("invalid target address: {}", target))?;
    let host = if host.starts_with('[') && host.ends_with(']') {
        host[1..host.len() - 1].to_string()
    } else {
        host.to_string()
    };
    SocksAddr::try_from((host, port))
        .map_err(|e: io::Error| anyhow!("invalid target address {}: {}", target, e))
}

fn build_vless_tcp_header(
    uuid_bytes: &[u8; 16],
    dst_addr: &str,
    dst_port: u16,
    addr_type: u8,
    uses_vision: bool,
) -> Vec<u8> {
    let mut header = Vec::new();
    header.push(0x00);
    header.extend_from_slice(uuid_bytes);

    if uses_vision {
        header.push(18);
        header.push(0x0a);
        header.push(16);
        header.extend_from_slice(b"xtls-rprx-vision");
    } else {
        header.push(0x00);
    }

    header.push(0x01);
    header.push((dst_port >> 8) as u8);
    header.push((dst_port & 0xFF) as u8);
    header.push(addr_type);

    match addr_type {
        1 => {
            let parts: Vec<u8> = dst_addr.split('.').map(|segment| segment.parse().unwrap()).collect();
            header.extend_from_slice(&parts);
        }
        2 => {
            header.push(dst_addr.len() as u8);
            header.extend_from_slice(dst_addr.as_bytes());
        }
        3 => {
            let addr: std::net::Ipv6Addr = dst_addr.parse().unwrap();
            header.extend_from_slice(&addr.octets());
        }
        _ => unreachable!(),
    }

    header
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use tokio::time::{Duration, timeout};

    #[test]
    fn parses_tcp_reality_vless_url() {
        let cfg = VlessConfig::try_from(
            "vless://118c2bd4-4d09-44c1-8b72-c879922d4a45@132.243.172.102:443?security=reality&type=tcp&sni=github.com&fp=chrome&pbk=K5mJc5Yb-7EtatgE4lK3smUel4r0VYYr7AVQRnJdGHM&sid=267d9234db73217e",
        )
        .unwrap();

        assert_eq!(cfg.uuid, "118c2bd4-4d09-44c1-8b72-c879922d4a45");
        assert_eq!(cfg.address, "132.243.172.102");
        assert_eq!(cfg.port, 443);
        assert_eq!(cfg.security, "reality");
        assert_eq!(cfg.server_name, "github.com");
        assert_eq!(cfg.network, "tcp");
    }

    #[test]
    fn parses_grpc_reality_vless_url() {
        let cfg = VlessConfig::try_from(
            "vless://f5241b24-a5e2-3577-94f3-4fbc23ea6d32@213.176.92.19:36931?security=reality&type=grpc&path=festiveecclesia&sni=cloudflare.com&fp=chrome&pbk=sZ05YkXN0R1zve7XcBtR20xfSt7OrAhyEjOqJ6TpXEw&sid=0072ded4af",
        )
        .unwrap();

        assert_eq!(cfg.network, "grpc");
        assert_eq!(cfg.server_name, "cloudflare.com");
        assert_eq!(cfg.path.as_deref(), Some("festiveecclesia"));
    }

    #[test]
    fn rejects_invalid_protocol() {
        let err = VlessConfig::try_from("https://example.com").unwrap_err();
        assert!(err.to_string().contains("invalid vless url"));
    }

    #[test]
    fn rejects_missing_required_fields() {
        let err = VlessConfig::try_from(
            "vless://uuid@example.com:443?security=reality&type=tcp&sni=github.com",
        )
        .unwrap_err();
        assert!(err.to_string().contains("missing pbk"));
    }

    #[test]
    fn plain_vless_header_omits_vision_extension() {
        let header = build_vless_tcp_header(&[0x11; 16], "example.com", 443, 2, false);

        assert_eq!(header[0], 0x00);
        assert_eq!(&header[1..17], &[0x11; 16]);
        assert_eq!(header[17], 0x00);
        assert_eq!(header[18], 0x01);
        assert_eq!(header[19], 0x01);
        assert_eq!(header[20], 0xbb);
        assert_eq!(header[21], 0x02);
        assert_eq!(header[22], 11);
        assert_eq!(&header[23..34], b"example.com");
    }

    #[test]
    fn vision_vless_header_includes_flow_extension() {
        let header = build_vless_tcp_header(&[0x22; 16], "1.2.3.4", 80, 1, true);

        assert_eq!(header[17], 18);
        assert_eq!(header[18], 0x0a);
        assert_eq!(header[19], 16);
        assert_eq!(&header[20..36], b"xtls-rprx-vision");
        assert_eq!(header[36], 0x01);
    }

    #[tokio::test]
    async fn plain_tcp_vless_transport_uses_standard_header_and_reads_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 512];
            let bytes_read = socket.read(&mut request).await.unwrap();
            let request = &request[..bytes_read];

            assert!(request.len() >= 34, "request too short: {}", request.len());
            assert_eq!(request[0], 0x00);
            assert_eq!(request[17], 0x00);
            assert_eq!(request[18], 0x01);
            assert_eq!(request[21], 0x02);
            assert_eq!(request[22], 11);
            assert_eq!(&request[23..34], b"example.com");

            socket.write_all(&[0x00, 0x00]).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .await
                .unwrap();
        });

        let transport = VlessTransport::from_url(&format!(
            "vless://118c2bd4-4d09-44c1-8b72-c879922d4a45@127.0.0.1:{port}?security=none&type=tcp"
        ))
        .unwrap();

        let mut stream = transport.connect("example.com:80").await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n")
            .await
            .unwrap();

        let mut response = [0u8; 256];
        let bytes_read = timeout(Duration::from_secs(2), stream.read(&mut response))
            .await
            .unwrap()
            .unwrap();
        let response = String::from_utf8_lossy(&response[..bytes_read]);

        assert!(response.contains("HTTP/1.1 200 OK"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn grpc_config_builds_but_connect_errors() {
        let transport = VlessTransport::from_url(
            "vless://f5241b24-a5e2-3577-94f3-4fbc23ea6d32@213.176.92.19:36931?security=reality&type=grpc&path=festiveecclesia&sni=cloudflare.com&fp=chrome&pbk=sZ05YkXN0R1zve7XcBtR20xfSt7OrAhyEjOqJ6TpXEw&sid=0072ded4af",
        )
        .unwrap();

        match transport.connect("example.com:443").await {
            Ok(_) => panic!("grpc VLESS should not connect until grpc transport is implemented"),
            Err(err) => assert!(err.to_string().contains("not supported yet")),
        }
    }
}
