pub mod connections;
pub mod onion;
pub mod relay;
pub mod socks;
use anyhow::Result;
use connections::{ConnectionRegistry, RouteType};
use hydra_ai::{AiNegotiator, RouteRequest};
use hydra_config::RelayConfig;
use hydra_econ::EconLedger;
use hydra_p2p::P2PHandle;
use libp2p::PeerId;
use relay::RelayConnection;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

pub struct Socks5Server {
    addr: SocketAddr,
    ai: Arc<AiNegotiator>,
    p2p: P2PHandle,
    econ: Arc<EconLedger>,
    relay: Option<Arc<RelayConnection>>,
    relay_mode: String,
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
            relay_mode: relay_config.mode.clone(),
            registry: Arc::new(ConnectionRegistry::new()),
        }
    }

    pub fn registry(&self) -> Arc<ConnectionRegistry> {
        self.registry.clone()
    }

    pub async fn run(&self) -> Result<()> {
        let listener = TcpListener::bind(self.addr).await?;
        info!("Socks5 server listening on {}", self.addr);

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            debug!("Accepted connection from {}", peer_addr);

            let ai = self.ai.clone();
            let p2p = self.p2p.clone();
            let econ = self.econ.clone();
            let relay = self.relay.clone();
            let relay_mode = self.relay_mode.clone();
            let registry = self.registry.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    handle_connection(stream, ai, p2p, econ, relay, relay_mode, registry).await
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
    ai: Arc<AiNegotiator>,
    p2p: P2PHandle,
    econ: Arc<EconLedger>,
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
        // Only CONNECT command (0x01)
        return Err(anyhow::anyhow!("Unsupported command or version"));
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
    let should_proxy = is_telegram && relay_mode != "never";
    let conn_id = registry.register(&target_addr, should_proxy);
    info!("Connection #{}: target={}, telegram={}, proxy={}", conn_id, target_addr, is_telegram, should_proxy);

    // Selective routing: Telegram traffic goes through relay by default,
    // other traffic goes direct unless relay_mode == "always"
    if !should_proxy && relay_mode != "always" {
        info!("Connection #{}: direct passthrough (non-Telegram)", conn_id);
        registry.update_route(conn_id, RouteType::Direct, Some("Direct: non-Telegram traffic".to_string()));

        match TcpStream::connect(&target_addr).await {
            Ok(outbound) => {
                stream.write_all(&socks::success_reply()).await?;
                let (mut ri, mut wi) = stream.into_split();
                let (mut ro, mut wo) = outbound.into_split();

                let c2t = tokio::io::copy(&mut ri, &mut wo);
                let t2c = tokio::io::copy(&mut ro, &mut wi);

                let (res_up, res_down) = tokio::join!(c2t, t2c);
                let up = res_up.unwrap_or(0);
                let down = res_down.unwrap_or(0);
                registry.update_bytes(conn_id, up, down);
                registry.close(conn_id);
                return Ok(());
            }
            Err(e) => {
                error!("Connection #{}: direct connect to {} failed: {}", conn_id, target_addr, e);
                stream.write_all(&socks::failure_reply()).await?;
                registry.close(conn_id);
                return Ok(());
            }
        }
    }

    // 3. AI Routing Decision (for proxied connections)
    let peers = p2p.get_peers().await?;
    let mut peer_infos = Vec::new();
    for peer_id in peers {
        let peer_id_str = peer_id.to_string();
        let balance = econ.get_balance(&peer_id_str)?;
        let telemetry = p2p.get_telemetry(&peer_id).await.unwrap_or_default();

        peer_infos.push(hydra_ai::PeerInfo {
            peer_id: peer_id_str,
            trust_score: balance.trust_score,
            current_debt: balance.debt,
            rtt_ms: telemetry.rtt_ms,
            bandwidth_bps: telemetry.bandwidth_bps,
        });
    }

    let mut route_request = RouteRequest {
        target: target_addr.clone(),
        protocol: "tcp".to_string(),
        peers: peer_infos,
        diagnostic_context: None,
    };

    // Retry loop with diagnostics
    let mut max_retries = 2;
    while max_retries > 0 {
        max_retries -= 1;

        let instruction = ai.decide_route(route_request.clone()).await?;
        info!("Connection #{}: AI routing instruction: {:?}", conn_id, instruction);

        // 4. Execution
        if !instruction.path.is_empty() {
            let next_hop_str = &instruction.path[0];
            info!("Routing through peer: {}", next_hop_str);
            let next_hop = next_hop_str.parse::<PeerId>()?;

            // Build circuit using Onion Router if multi-hop, or direct P2P stream if single hop
            let path_peer_ids: Vec<PeerId> = instruction
                .path
                .iter()
                .filter_map(|p| p.parse().ok())
                .collect();

            let stream_res = if path_peer_ids.len() > 1 {
                let onion = crate::onion::OnionRouter::new(p2p.clone());
                onion.build_circuit(&path_peer_ids, &target_addr).await
            } else {
                p2p.open_stream(next_hop, hydra_p2p::TUNNEL_PROTOCOL)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            };

            match stream_res {
                Ok(mut tunnel) => {
                    use libp2p::futures::{AsyncReadExt as _, AsyncWriteExt as _};

                    // Tunnel Handshake: Send target address
                    let target_bytes = target_addr.as_bytes();
                    let len_bytes = (target_bytes.len() as u16).to_be_bytes();
                    tunnel.write_all(&len_bytes).await?;
                    tunnel.write_all(target_bytes).await?;

                    // Read response
                    let mut resp = [0u8; 1];
                    tunnel.read_exact(&mut resp).await?;

                    if resp[0] == 0x00 {
                        info!("Connection #{}: tunnel established to {} via {}", conn_id, target_addr, next_hop_str);
                        let route_reason = format!("P2P via peer {}", next_hop_str);
                        registry.update_route(conn_id, RouteType::P2P, Some(route_reason));
                        stream.write_all(&socks::success_reply()).await?;

                        let (mut ri, mut wi) = stream.into_split();
                        let (rt, wt) = tunnel.split();

                        use tokio_util::compat::FuturesAsyncReadCompatExt;
                        use tokio_util::compat::FuturesAsyncWriteCompatExt;

                        let mut rt_compat = rt.compat();
                        let mut wt_compat = wt.compat_write();

                        let client_to_tunnel = tokio::io::copy(&mut ri, &mut wt_compat);
                        let tunnel_to_client = tokio::io::copy(&mut rt_compat, &mut wi);

                        let start_time = std::time::Instant::now();
                        let (res_c2t, res_t2c) = tokio::join!(client_to_tunnel, tunnel_to_client);
                        let duration = start_time.elapsed();

                        let bytes_c2t = res_c2t.unwrap_or(0);
                        let bytes_t2c = res_t2c.unwrap_or(0);
                        let total_bytes = bytes_c2t + bytes_t2c;

                        info!(
                            "Connection #{}: P2P transfer finished: {} up, {} down. Total: {}. Duration: {:?}",
                            conn_id, bytes_c2t, bytes_t2c, total_bytes, duration
                        );

                        registry.update_bytes(conn_id, bytes_c2t, bytes_t2c);
                        registry.close(conn_id);
                        p2p.update_bandwidth(next_hop, total_bytes, duration).await;

                        if econ.update_debt(&next_hop_str, total_bytes as i64)? {
                            let econ_c = econ.clone();
                            let next_hop_c = next_hop_str.clone();
                            tokio::spawn(async move {
                                if let Err(e) = econ_c.settle(&next_hop_c).await {
                                    error!("Settlement failed for peer {}: {}", next_hop_c, e);
                                }
                            });
                        }

                        econ.verify_proof_of_transfer(&next_hop_str, true)?;
                        return Ok(()); // Success
                    } else {
                        error!(
                            "Peer {} failed to connect to target {}",
                            next_hop_str, target_addr
                        );
                        econ.verify_proof_of_transfer(&next_hop_str, false)?;

                        // DIAGNOSTIC STEP
                        // Ask another peer (or all peers) to ping the target to see if it's truly down or just this node
                        info!("Gathering diagnostics for target {}", target_addr);
                        let mut diag_context = format!(
                            "Peer {} failed to connect to {}. ",
                            next_hop_str, target_addr
                        );

                        // We will just ask the first other available peer as an example
                        for peer in &route_request.peers {
                            if peer.peer_id != *next_hop_str {
                                if let Ok(peer_id) = peer.peer_id.parse::<PeerId>() {
                                    let diag_req =
                                        hydra_p2p::diagnostics::DiagnosticRequest::PingTarget {
                                            target: target_addr.clone(),
                                        };
                                    match p2p.send_diagnostic_request(peer_id, diag_req).await {
                                        Ok(hydra_p2p::diagnostics::DiagnosticResponse::PingResult { reachable, latency_ms, error }) => {
                                            diag_context.push_str(&format!("Peer {} reported reachable={}, latency={:?}, error={:?}. ", peer.peer_id, reachable, latency_ms, error));
                                        }
                                        Err(e) => {
                                            diag_context.push_str(&format!("Failed to ask peer {} for diag: {}. ", peer.peer_id, e));
                                        }
                                    }
                                }
                            }
                        }

                        route_request.diagnostic_context = Some(diag_context);
                        info!(
                            "Retrying with diagnostics: {:?}",
                            route_request.diagnostic_context
                        );
                        // Loop continues
                    }
                }
                Err(e) => {
                    error!("Failed to open P2P tunnel to {}: {}", next_hop_str, e);
                    econ.verify_proof_of_transfer(&next_hop_str, false)?;
                    route_request.diagnostic_context = Some(format!(
                        "Could not establish P2P tunnel to {}: {}",
                        next_hop_str, e
                    ));
                    // Loop continues
                }
            }
        } else if relay_mode == "always" || (relay.is_some() && instruction.path.is_empty()) {
            if let Some(ref relay_conn) = relay {
                info!("Connection #{}: attempting WSS relay to {}", conn_id, target_addr);
                registry.update_route(conn_id, RouteType::Relay, Some("Routed via Cloudflare WSS relay".to_string()));
                match relay_conn.connect_to_target(&target_addr).await {
                    Ok(outbound) => {
                        stream.write_all(&socks::success_reply()).await?;

                        let (mut ri, mut wi) = stream.into_split();
                        let (mut ro, mut wo) = outbound.into_split();

                        let client_to_target = tokio::io::copy(&mut ri, &mut wo);
                        let target_to_client = tokio::io::copy(&mut ro, &mut wi);

                        let (res_up, res_down) = tokio::join!(client_to_target, target_to_client);
                        let up = res_up.unwrap_or(0);
                        let down = res_down.unwrap_or(0);
                        registry.update_bytes(conn_id, up, down);
                        registry.close(conn_id);
                        return Ok(());
                    }
                    Err(e) => {
                        error!("WSS relay failed: {}. Falling back to direct.", e);
                        route_request.diagnostic_context = Some(format!(
                            "WSS relay failed: {}. Trying direct.",
                            e
                        ));
                    }
                }
            }
            info!("Connection #{}: direct connection to {}", conn_id, target_addr);
            registry.update_route(conn_id, RouteType::Direct, Some("Direct fallback after relay unavailable".to_string()));
            match TcpStream::connect(&target_addr).await {
                Ok(outbound) => {
                    stream.write_all(&socks::success_reply()).await?;

                    let (mut ri, mut wi) = stream.into_split();
                    let (mut ro, mut wo) = outbound.into_split();

                    let c2t = tokio::io::copy(&mut ri, &mut wo);
                    let t2c = tokio::io::copy(&mut ro, &mut wi);

                    let (res_up, res_down) = tokio::join!(c2t, t2c);
                    let up = res_up.unwrap_or(0);
                    let down = res_down.unwrap_or(0);
                    registry.update_bytes(conn_id, up, down);
                    registry.close(conn_id);
                    return Ok(());
                }
                Err(e) => {
                    error!("Failed to connect to target {}: {}", target_addr, e);

                    // DIAGNOSTIC STEP: local failed, let's ask peers
                    info!(
                        "Local connection failed, gathering diagnostics from peers for {}",
                        target_addr
                    );
                    let mut diag_context = format!(
                        "Local direct connect to {} failed with {}. ",
                        target_addr, e
                    );

                    for peer in &route_request.peers {
                        if let Ok(peer_id) = peer.peer_id.parse::<PeerId>() {
                            let diag_req = hydra_p2p::diagnostics::DiagnosticRequest::PingTarget {
                                target: target_addr.clone(),
                            };
                            match p2p.send_diagnostic_request(peer_id, diag_req).await {
                                Ok(hydra_p2p::diagnostics::DiagnosticResponse::PingResult {
                                    reachable,
                                    latency_ms,
                                    error,
                                }) => {
                                    diag_context.push_str(&format!(
                                        "Peer {} reported reachable={}, latency={:?}, error={:?}. ",
                                        peer.peer_id, reachable, latency_ms, error
                                    ));
                                }
                                Err(err) => {
                                    diag_context.push_str(&format!(
                                        "Failed to ask peer {} for diag: {}. ",
                                        peer.peer_id, err
                                    ));
                                }
                            }
                        }
                    }

                    route_request.diagnostic_context = Some(diag_context);
                    // Loop continues to let AI decide (perhaps routing via peer that reported reachable=true)
                }
            }
        }
    }

    // If we exhaust retries
    error!("Connection #{}: all routing attempts exhausted for {}", conn_id, target_addr);
    stream.write_all(&socks::failure_reply()).await?;
    registry.close(conn_id);
    Ok(())
}
