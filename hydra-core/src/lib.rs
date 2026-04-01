pub mod connections;
pub mod onion;
pub mod socks;
pub mod transport;
use anyhow::Result;
use connections::{ConnectionRegistry, RouteType};
use hydra_ai::AiNegotiator;
use hydra_econ::EconLedger;
use hydra_p2p::P2PHandle;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info, warn};
use transport::ConfiguredTransport;

use std::sync::RwLock;

pub struct Socks5Server {
    addr: SocketAddr,
    #[allow(dead_code)]
    ai: Arc<AiNegotiator>,
    #[allow(dead_code)]
    p2p: P2PHandle,
    #[allow(dead_code)]
    econ: Arc<EconLedger>,
    transports: Vec<ConfiguredTransport>,
    proxy_mode: Arc<RwLock<String>>,
    registry: Arc<ConnectionRegistry>,
}

impl Socks5Server {
    pub fn new(
        addr: SocketAddr,
        ai: Arc<AiNegotiator>,
        p2p: P2PHandle,
        econ: Arc<EconLedger>,
        transports: Vec<ConfiguredTransport>,
        proxy_mode: String,
    ) -> Self {
        Self {
            addr,
            ai,
            p2p,
            econ,
            transports,
            proxy_mode: Arc::new(RwLock::new(proxy_mode)),
            registry: Arc::new(ConnectionRegistry::new()),
        }
    }

    pub fn registry(&self) -> Arc<ConnectionRegistry> {
        self.registry.clone()
    }

    pub fn proxy_mode_handle(&self) -> Arc<RwLock<String>> {
        self.proxy_mode.clone()
    }

    pub fn set_proxy_mode(&self, mode: &str) {
        if let Ok(mut m) = self.proxy_mode.write() {
            *m = mode.to_string();
            tracing::info!("Proxy mode changed to: {}", mode);
        }
    }

    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("Socks5 server listening on {}", self.addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            debug!("Accepted connection from {}", peer_addr);

            let transports = self.transports.clone();
            let proxy_mode = self
                .proxy_mode
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let registry = self.registry.clone();

            tokio::spawn(async move {
                if let Err(e) = handle_connection(stream, transports, proxy_mode, registry).await {
                    error!("Error handling connection from {}: {}", peer_addr, e);
                }
            });

            self.registry.gc(500);
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    transports: Vec<ConfiguredTransport>,
    proxy_mode: String,
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

    let is_telegram = socks::is_telegram_target(&target_addr);
    let is_relay_infra = socks::is_relay_infrastructure(&target_addr);
    let plan = select_transport_plan(&target_addr, is_telegram, is_relay_infra, &proxy_mode, &transports);

    let conn_id = registry.register(&target_addr, plan.requires_proxy());
    info!(
        "Connection #{}: target={}, telegram={}, proxy_required={}, mode={}",
        conn_id,
        target_addr,
        is_telegram,
        plan.requires_proxy(),
        proxy_mode
    );

    if plan.is_direct() {
        registry.update_route(conn_id, RouteType::Direct, Some("Direct".to_string()));
        return do_direct(stream, &target_addr, conn_id, &registry).await;
    }

    if plan.transports.is_empty() {
        warn!(
            "Connection #{}: proxied path required for {}, but no matching transports are configured",
            conn_id, target_addr
        );
        registry.update_route(
            conn_id,
            RouteType::Relay,
            Some("No matching transports configured".to_string()),
        );
        stream.write_all(&socks::failure_reply()).await?;
        registry.close(conn_id);
        return Ok(());
    }

    let mut errors = Vec::new();
    for configured in plan.transports {
        info!(
            "Connection #{}: trying transport {} to {}",
            conn_id,
            configured.kind.as_str(),
            target_addr
        );
        registry.update_route(
            conn_id,
            RouteType::Relay,
            Some(format!("Via {}", configured.kind.as_str())),
        );

        match configured.transport.connect(&target_addr).await {
            Ok(outbound) => {
                stream.write_all(&socks::success_reply()).await?;
                let (mut ri, mut wi) = stream.into_split();
                let (mut ro, mut wo) = tokio::io::split(outbound);
                let c2t = tokio::io::copy(&mut ri, &mut wo);
                let t2c = tokio::io::copy(&mut ro, &mut wi);
                let (res_up, res_down) = tokio::join!(c2t, t2c);
                registry.update_bytes(conn_id, res_up.unwrap_or(0), res_down.unwrap_or(0));
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
        RouteType::Relay,
        Some(format!("Transport failure: {}", errors.join("; "))),
    );
    stream.write_all(&socks::failure_reply()).await?;
    registry.close(conn_id);
    Ok(())
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

#[derive(Clone)]
struct TransportPlan {
    transports: Vec<ConfiguredTransport>,
    proxy_required: bool,
}

impl TransportPlan {
    fn direct() -> Self {
        Self {
            transports: Vec::new(),
            proxy_required: false,
        }
    }

    fn proxied(transports: Vec<ConfiguredTransport>) -> Self {
        Self {
            proxy_required: true,
            transports,
        }
    }

    fn requires_proxy(&self) -> bool {
        self.proxy_required
    }

    fn is_direct(&self) -> bool {
        !self.proxy_required
    }
}

fn select_transport_plan(
    _target: &str,
    is_telegram: bool,
    is_relay_infra: bool,
    proxy_mode: &str,
    transports: &[ConfiguredTransport],
) -> TransportPlan {
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
    use crate::transport::{Transport, TransportKind, TransportStream};
    use async_trait::async_trait;
    use hydra_config::TransportMode;

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
        }
    }

    #[test]
    fn route_plan_off_is_direct() {
        let plan = select_transport_plan(
            "149.154.167.50:443",
            true,
            false,
            "off",
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
            &[configured(TransportMode::Telegram)],
        );
        assert!(plan.requires_proxy());
        assert!(plan.transports.is_empty());
    }
}
