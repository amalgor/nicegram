pub mod connections;
pub mod onion;
pub mod relay;
pub mod socks;
use anyhow::Result;
use connections::{ConnectionRegistry, RouteType};
use hydra_ai::AiNegotiator;
use hydra_config::RelayConfig;
use hydra_econ::EconLedger;
use hydra_p2p::P2PHandle;
use relay::RelayConnection;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

use std::sync::RwLock;

pub struct Socks5Server {
    addr: SocketAddr,
    #[allow(dead_code)]
    ai: Arc<AiNegotiator>,
    #[allow(dead_code)]
    p2p: P2PHandle,
    #[allow(dead_code)]
    econ: Arc<EconLedger>,
    relay: Option<Arc<RelayConnection>>,
    relay_mode: Arc<RwLock<String>>,
    registry: Arc<ConnectionRegistry>,
}

impl Socks5Server {
    pub fn new(
        addr: SocketAddr,
        ai: Arc<AiNegotiator>,
        p2p: P2PHandle,
        econ: Arc<EconLedger>,
        relay_config: &RelayConfig,
    ) -> Self {
        let relay = if !relay_config.endpoints.is_empty() && relay_config.mode != "never" {
            Some(Arc::new(RelayConnection::new(
                relay_config.endpoints.clone(),
                relay_config.device_id.clone(),
            )))
        } else {
            None
        };
        Self {
            addr,
            ai,
            p2p,
            econ,
            relay,
            relay_mode: Arc::new(RwLock::new(relay_config.mode.clone())),
            registry: Arc::new(ConnectionRegistry::new()),
        }
    }

    pub fn registry(&self) -> Arc<ConnectionRegistry> {
        self.registry.clone()
    }

    pub fn relay_mode_handle(&self) -> Arc<RwLock<String>> {
        self.relay_mode.clone()
    }

    pub fn set_relay_mode(&self, mode: &str) {
        if let Ok(mut m) = self.relay_mode.write() {
            *m = mode.to_string();
            tracing::info!("Relay mode changed to: {}", mode);
        }
    }

    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("Socks5 server listening on {}", self.addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            debug!("Accepted connection from {}", peer_addr);

            let relay = self.relay.clone();
            let relay_mode = self.relay_mode.read().unwrap_or_else(|e| e.into_inner()).clone();
            let registry = self.registry.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    handle_connection(stream, relay, relay_mode, registry).await
                {
                    error!("Error handling connection from {}: {}", peer_addr, e);
                }
            });

            self.registry.gc(500);
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    relay: Option<Arc<RelayConnection>>,
    relay_mode: String,
    registry: Arc<ConnectionRegistry>,
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

    // We only support 'NO AUTHENTICATION REQUIRED' (0x00)
    if !methods.contains(&0x00) {
        stream.write_all(&[0x05, 0xFF]).await?;
        return Err(anyhow::anyhow!("No supported auth methods"));
    }

    stream.write_all(&[0x05, 0x00]).await?;

    // 2. Request
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;

    if header[0] != 0x05 || header[1] != 0x01 {
        // SOCKS5 CMD: 0x01=CONNECT, 0x02=BIND, 0x03=UDP ASSOCIATE
        // We only support CONNECT. UDP ASSOCIATE requests from tun2proxy (DNS) are expected noise.
        debug!("Ignoring non-CONNECT SOCKS5 request (cmd=0x{:02x})", header[1]);
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

    // Register connection in the tracking registry
    let is_telegram = socks::is_telegram_target(&target_addr);
    let is_relay_infra = socks::is_relay_infrastructure(&target_addr);

    // Relay (WSS via Cloudflare) is only for anti-censorship: Telegram traffic.
    // In "full" VPN mode, all traffic flows through the VPN tunnel -> SOCKS5,
    // but only Telegram uses the WSS relay; everything else connects directly
    // from the SOCKS5 proxy (still within VPN for DNS/routing protection).
    let use_relay = if is_relay_infra {
        false
    } else {
        match relay_mode.as_str() {
            "off" | "never" => false,
            _ => is_telegram,
        }
    };
    let is_vpn_routed = matches!(relay_mode.as_str(), "full" | "always");
    let conn_id = registry.register(&target_addr, use_relay || is_vpn_routed);
    info!("Connection #{}: target={}, telegram={}, relay={}, vpn={}, mode={}", conn_id, target_addr, is_telegram, use_relay, is_vpn_routed, relay_mode);

    if !use_relay {
        let route_desc = if is_vpn_routed { "Direct (VPN routed)" } else { "Direct" };
        registry.update_route(conn_id, RouteType::Direct, Some(route_desc.to_string()));
        return do_direct(stream, &target_addr, conn_id, &registry).await;
    }

    // Telegram traffic: relay-first with direct fallback
    if let Some(ref relay_conn) = relay {
        info!("Connection #{}: relay-first to {}", conn_id, target_addr);
        registry.update_route(conn_id, RouteType::Relay, Some("Via Cloudflare relay".to_string()));
        match relay_conn.connect_to_target(&target_addr).await {
            Ok(outbound) => {
                stream.write_all(&socks::success_reply()).await?;
                let (mut ri, mut wi) = stream.into_split();
                let (mut ro, mut wo) = outbound.into_split();
                let c2t = tokio::io::copy(&mut ri, &mut wo);
                let t2c = tokio::io::copy(&mut ro, &mut wi);
                let (res_up, res_down) = tokio::join!(c2t, t2c);
                registry.update_bytes(conn_id, res_up.unwrap_or(0), res_down.unwrap_or(0));
                registry.close(conn_id);
                return Ok(());
            }
            Err(e) => {
                info!("Connection #{}: relay failed ({}), falling back to direct", conn_id, e);
            }
        }
    }

    // Relay unavailable or failed — fall back to direct
    info!("Connection #{}: direct fallback to {}", conn_id, target_addr);
    registry.update_route(conn_id, RouteType::Direct, Some("Direct fallback (relay unavailable)".to_string()));
    do_direct(stream, &target_addr, conn_id, &registry).await
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
            let c2t = tokio::io::copy(&mut ri, &mut wo);
            let t2c = tokio::io::copy(&mut ro, &mut wi);
            let (res_up, res_down) = tokio::join!(c2t, t2c);
            registry.update_bytes(conn_id, res_up.unwrap_or(0), res_down.unwrap_or(0));
            registry.close(conn_id);
            Ok(())
        }
        Err(e) => {
            error!("Connection #{}: direct connect to {} failed: {}", conn_id, target_addr, e);
            stream.write_all(&socks::failure_reply()).await?;
            registry.close(conn_id);
            Ok(())
        }
    }
}
