pub mod diagnostics;
pub mod telemetry;

use anyhow::{Context, Result, anyhow};
use diagnostics::{DiagnosticRequest, DiagnosticResponse};
use libp2p::{
    PeerId, StreamProtocol, Swarm,
    futures::{StreamExt, stream::BoxStream},
    gossipsub::{self, IdentTopic, MessageAuthenticity},
    kad::{self, store::MemoryStore},
    mdns, noise, ping,
    request_response::{self, ProtocolSupport},
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux,
};
use hydra_config::NetworkConfig;
use libp2p_stream as p2p_stream;
use std::sync::Arc;
use std::time::Duration;

pub const TUNNEL_PROTOCOL: StreamProtocol = StreamProtocol::new("/hydra/tunnel/1.0.0");
pub const RELAY_ENDPOINTS_TOPIC: &str = "hydra/relay-endpoints/1.0";
use telemetry::{PeerMetrics, TelemetryStore};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info};

#[derive(NetworkBehaviour)]
#[behaviour(out_event = "HydraEvent")]
pub struct HydraBehaviour {
    pub kademlia: kad::Behaviour<MemoryStore>,
    pub gossipsub: gossipsub::Behaviour,
    pub mdns: libp2p::swarm::behaviour::toggle::Toggle<mdns::tokio::Behaviour>,
    pub stream: p2p_stream::Behaviour,
    pub ping: ping::Behaviour,
    pub diagnostics: request_response::cbor::Behaviour<DiagnosticRequest, DiagnosticResponse>,
}

#[derive(Debug)]
pub enum HydraEvent {
    Kademlia(kad::Event),
    Gossipsub(gossipsub::Event),
    Mdns(mdns::Event),
    Stream(()),
    Ping(ping::Event),
    Diagnostics(request_response::Event<DiagnosticRequest, DiagnosticResponse>),
}

impl From<kad::Event> for HydraEvent {
    fn from(event: kad::Event) -> Self {
        HydraEvent::Kademlia(event)
    }
}

impl From<gossipsub::Event> for HydraEvent {
    fn from(event: gossipsub::Event) -> Self {
        HydraEvent::Gossipsub(event)
    }
}

impl From<mdns::Event> for HydraEvent {
    fn from(event: mdns::Event) -> Self {
        HydraEvent::Mdns(event)
    }
}

impl From<()> for HydraEvent {
    fn from(_: ()) -> Self {
        HydraEvent::Stream(())
    }
}

impl From<ping::Event> for HydraEvent {
    fn from(event: ping::Event) -> Self {
        HydraEvent::Ping(event)
    }
}

impl From<request_response::Event<DiagnosticRequest, DiagnosticResponse>> for HydraEvent {
    fn from(event: request_response::Event<DiagnosticRequest, DiagnosticResponse>) -> Self {
        HydraEvent::Diagnostics(event)
    }
}

pub type P2PStream = libp2p::Stream;

pub enum P2PCommand {
    GetPeers {
        resp: oneshot::Sender<Vec<PeerId>>,
    },
    OpenStream {
        peer_id: PeerId,
        protocol: StreamProtocol,
        resp: oneshot::Sender<Result<libp2p::Stream>>,
    },
    DiagnosticRequest {
        peer_id: PeerId,
        request: DiagnosticRequest,
        resp: oneshot::Sender<Result<DiagnosticResponse>>,
    },
    PublishRelayEndpoint {
        data: Vec<u8>,
        resp: oneshot::Sender<Result<()>>,
    },
}

pub struct P2PNode {
    swarm: Swarm<HydraBehaviour>,
    cmd_rx: mpsc::Receiver<P2PCommand>,
    incoming_streams: BoxStream<'static, (PeerId, libp2p::Stream)>,
    telemetry: Arc<TelemetryStore>,
    p2p: P2PHandle,
}

#[derive(Clone)]
pub struct P2PHandle {
    cmd_tx: mpsc::Sender<P2PCommand>,
    telemetry: Arc<TelemetryStore>,
}

impl P2PHandle {
    pub async fn get_peers(&self) -> Result<Vec<PeerId>> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2PCommand::GetPeers { resp: tx })
            .await
            .map_err(|e| anyhow!("Failed to send GetPeers command: {}", e))?;
        rx.await
            .map_err(|e| anyhow!("Failed to receive GetPeers response: {}", e))
    }

    pub async fn get_telemetry(&self, peer_id: &PeerId) -> Option<PeerMetrics> {
        self.telemetry.get_metrics(peer_id).await
    }

    pub async fn update_bandwidth(&self, peer_id: PeerId, bytes: u64, duration: Duration) {
        self.telemetry
            .update_bandwidth(peer_id, bytes, duration)
            .await
    }

    pub async fn open_stream(&self, peer_id: PeerId, protocol: StreamProtocol) -> Result<libp2p::Stream> {
        let (tx, rx) = oneshot::channel::<Result<libp2p::Stream>>();
        self.cmd_tx
            .send(P2PCommand::OpenStream {
                peer_id,
                protocol,
                resp: tx,
            })
            .await
            .map_err(|e| anyhow!("Failed to send OpenStream command: {}", e))?;
        rx.await
            .map_err(|e| anyhow!("Failed to receive OpenStream response: {}", e))?
    }

    pub async fn send_diagnostic_request(
        &self,
        peer_id: PeerId,
        request: DiagnosticRequest,
    ) -> Result<DiagnosticResponse> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2PCommand::DiagnosticRequest {
                peer_id,
                request,
                resp: tx,
            })
            .await
            .map_err(|e| anyhow!("Failed to send DiagnosticRequest command: {}", e))?;
        rx.await
            .map_err(|e| anyhow!("Failed to receive DiagnosticRequest response: {}", e))?
    }

    /// Publish a relay endpoint announcement to the gossipsub network.
    /// data should be JSON: {"url": "wss://...", "latency_ms": N, "alive": true, "timestamp": epoch}
    pub async fn publish_relay_endpoint(&self, data: Vec<u8>) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.cmd_tx
            .send(P2PCommand::PublishRelayEndpoint { data, resp: tx })
            .await
            .map_err(|e| anyhow!("Failed to send PublishRelayEndpoint command: {}", e))?;
        rx.await
            .map_err(|e| anyhow!("Failed to receive PublishRelayEndpoint response: {}", e))?
    }
}

impl P2PNode {
    /// Create a no-op P2PHandle for environments where P2P cannot initialize
    /// (e.g. Android without /etc/resolv.conf). All commands sent to this handle
    /// will fail with "P2P unavailable" errors.
    pub fn dummy_handle() -> P2PHandle {
        let (tx, _rx) = mpsc::channel(1);
        P2PHandle {
            cmd_tx: tx,
            telemetry: Arc::new(TelemetryStore::new()),
        }
    }

    pub async fn new(keypair: Option<libp2p::identity::Keypair>, listen_port: u16, network_config: &NetworkConfig) -> Result<(Self, P2PHandle)> {
        let local_key = keypair.unwrap_or_else(libp2p::identity::Keypair::generate_ed25519);
        let mut swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_dns_config(
                libp2p_dns::ResolverConfig::cloudflare(),
                libp2p_dns::ResolverOpts::default(),
            )
            .with_behaviour(|key| {
                let peer_id = PeerId::from(key.public());
                let store = MemoryStore::new(peer_id);
                
let kademlia = kad::Behaviour::new(peer_id, store);

                let gossipsub_config = gossipsub::ConfigBuilder::default()
                    .heartbeat_interval(Duration::from_secs(10))
                    .validation_mode(gossipsub::ValidationMode::Permissive)
                    .build()
                    .map_err(|e| std::io::Error::other(format!("gossipsub config: {}", e)))?;
                let mut gossipsub = gossipsub::Behaviour::new(
                    MessageAuthenticity::Signed(key.clone()),
                    gossipsub_config,
                )
                .map_err(|e| std::io::Error::other(format!("gossipsub: {}", e)))?;

                let relay_topic = IdentTopic::new(RELAY_ENDPOINTS_TOPIC);
                gossipsub.subscribe(&relay_topic)
                    .map_err(|e| std::io::Error::other(format!("gossipsub subscribe: {}", e)))?;

                let mdns_enabled = listen_port == 0;
                let mdns = if mdns_enabled {
                    Some(mdns::tokio::Behaviour::new(mdns::Config::default(), peer_id)?)
                } else {
                    None
                };
                let mdns = libp2p::swarm::behaviour::toggle::Toggle::from(mdns);
                let stream = p2p_stream::Behaviour::new();
                let ping = ping::Behaviour::new(
                    ping::Config::new().with_interval(Duration::from_secs(15)),
                );
                let diagnostics = request_response::cbor::Behaviour::<
                    DiagnosticRequest,
                    DiagnosticResponse,
                >::new(
                    [(
                        StreamProtocol::new("/hydra/diag/1.0.0"),
                        ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                );
                Ok(HydraBehaviour {
                    kademlia,
                    gossipsub,
                    mdns,
                    stream,
                    ping,
                    diagnostics,
                })
            })
            .map_err(|e| anyhow!("Failed to build behaviour: {}", e))?
            .with_swarm_config(|c| c.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();

        swarm.listen_on(format!("/ip4/0.0.0.0/tcp/{}", listen_port).parse()?)?;

        let mut control = swarm.behaviour_mut().stream.new_control();
        let incoming_streams = control
            .accept(TUNNEL_PROTOCOL)
            .map_err(|e| anyhow!("Failed to accept protocol: {}", e))?;

        
        let (tx, rx) = mpsc::channel(32);
        
        for boot_addr_str in &network_config.bootstrap_nodes {
            match boot_addr_str.parse::<libp2p::Multiaddr>() {
                Ok(boot_addr) => {
                    if let Err(e) = swarm.dial(boot_addr) {
                        tracing::warn!("Failed to dial bootstrap node {}: {}", boot_addr_str, e);
                    } else {
                        tracing::info!("Dialing bootstrap node at {}", boot_addr_str);
                    }
                }
                Err(e) => {
                    tracing::error!("Invalid bootstrap node address '{}': {}", boot_addr_str, e);
                }
            }
        }

        let telemetry = Arc::new(TelemetryStore::new());
        let p2p_handle = P2PHandle {
            cmd_tx: tx,
            telemetry: telemetry.clone(),
        };
        Ok((
            Self {
                swarm,
                cmd_rx: rx,
                incoming_streams: incoming_streams.boxed(),
                telemetry,
                p2p: p2p_handle.clone(),
            },
            p2p_handle,
        ))
    }

    pub async fn run(mut self) -> Result<()> {
        let mut pending_diag_requests: std::collections::HashMap<
            request_response::OutboundRequestId,
            oneshot::Sender<Result<DiagnosticResponse>>,
        > = std::collections::HashMap::new();

        loop {
            tokio::select! {
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            info!("P2P node listening on {}", address);
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, endpoint, .. } => {
                            info!("Established connection to {}", peer_id);
                            self.swarm.behaviour_mut().kademlia.add_address(&peer_id, endpoint.get_remote_address().clone());
                        }
                        SwarmEvent::OutgoingConnectionError { peer_id, error, .. } => {
                            tracing::error!("Outgoing connection error to {:?}: {}", peer_id, error);
                        }
                        SwarmEvent::Behaviour(HydraEvent::Mdns(mdns::Event::Discovered(list))) => {
                            for (peer_id, multiaddr) in list {
                                info!("mDNS discovered peer: {} at {}", peer_id, multiaddr);
                                self.swarm.behaviour_mut().kademlia.add_address(&peer_id, multiaddr);
                            }
                        }
                        SwarmEvent::Behaviour(HydraEvent::Ping(ping::Event { peer, result, .. })) => {
                            match result {
                                Ok(rtt) => {
                                    debug!("Ping to {}: {:?}", peer, rtt);
                                    self.telemetry.update_rtt(peer, rtt).await;
                                }
                                Err(e) => debug!("Ping to {} failed: {}", peer, e),
                            }
                        }
                        SwarmEvent::Behaviour(HydraEvent::Diagnostics(request_response::Event::Message { peer, message, .. })) => {
                            match message {
                                request_response::Message::Request { request_id: _, request, channel } => {
                                    info!("Received diagnostic request from {}: {:?}", peer, request);
                                    // Handle diagnostic request
                                    let response = match request {
                                        DiagnosticRequest::PingTarget { target } => {
                                            // Real TCP connect check to measure reachability and latency
                                            let start = std::time::Instant::now();
                                            match tokio::net::TcpStream::connect(&target).await {
                                                Ok(_) => {
                                                    let latency_ms = Some(start.elapsed().as_millis() as u64);
                                                    DiagnosticResponse::PingResult { reachable: true, latency_ms, error: None }
                                                }
                                                Err(e) => {
                                                    DiagnosticResponse::PingResult { reachable: false, latency_ms: None, error: Some(e.to_string()) }
                                                }
                                            }
                                        }
                                    };
                                    let _ = self.swarm.behaviour_mut().diagnostics.send_response(channel, response);
                                }
                                request_response::Message::Response { request_id, response } => {
                                    info!("Received diagnostic response from {}: {:?}", peer, response);
                                    if let Some(resp_sender) = pending_diag_requests.remove(&request_id) {
                                        let _ = resp_sender.send(Ok(response));
                                    }
                                }
                            }
                        }
                        SwarmEvent::Behaviour(HydraEvent::Diagnostics(request_response::Event::OutboundFailure { request_id, error, .. })) => {
                            if let Some(resp_sender) = pending_diag_requests.remove(&request_id) {
                                let _ = resp_sender.send(Err(anyhow!("Diagnostic request failed: {}", error)));
                            }
                        }
                        SwarmEvent::Behaviour(HydraEvent::Gossipsub(gossipsub::Event::Message {
                            propagation_source,
                            message,
                            ..
                        })) => {
                            if message.topic == IdentTopic::new(RELAY_ENDPOINTS_TOPIC).hash() {
                                if let Ok(text) = String::from_utf8(message.data.clone()) {
                                    info!("Received relay endpoint from {}: {}", propagation_source, text);
                                }
                            }
                        }
                        SwarmEvent::Behaviour(HydraEvent::Gossipsub(gossipsub::Event::Subscribed { peer_id, topic })) => {
                            debug!("Peer {} subscribed to {}", peer_id, topic);
                        }
                        _ => {}
                    }
                }
                Some((peer_id, mut stream)) = self.incoming_streams.next() => {
                    info!("Incoming tunnel stream from {}", peer_id);
                    let p2p_clone = self.p2p.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_incoming_tunnel(peer_id, &mut stream, p2p_clone).await {
                            error!("Error handling tunnel from {}: {}", peer_id, e);
                        }
                    });
                }
                cmd = self.cmd_rx.recv() => {
                    if let Some(cmd) = cmd {
                        match cmd {
                            P2PCommand::GetPeers { resp } => {
                                let mut peers = Vec::new();
                                for bucket in self.swarm.behaviour_mut().kademlia.kbuckets() {
                                    for entry in bucket.iter() {
                                        peers.push(*entry.node.key.preimage());
                                    }
                                }
                                let _ = resp.send(peers);
                            }
                            P2PCommand::OpenStream { peer_id, protocol, resp } => {
                                let mut control = self.swarm.behaviour_mut().stream.new_control();
                                tokio::spawn(async move {
                                    match control.open_stream(peer_id, protocol).await {
                                        Ok(s) => { let _ = resp.send(Ok(s)); }
                                        Err(e) => { let _ = resp.send(Err(anyhow!(e))); }
                                    }
                                });
                            }
                            P2PCommand::DiagnosticRequest { peer_id, request, resp } => {
                                let request_id = self.swarm.behaviour_mut().diagnostics.send_request(&peer_id, request);
                                pending_diag_requests.insert(request_id, resp);
                            }
                            P2PCommand::PublishRelayEndpoint { data, resp } => {
                                let topic = IdentTopic::new(RELAY_ENDPOINTS_TOPIC);
                                let result = self.swarm.behaviour_mut().gossipsub
                                    .publish(topic, data)
                                    .map(|_| ())
                                    .map_err(|e| anyhow!("Gossipsub publish failed: {}", e));
                                let _ = resp.send(result);
                            }
                        }
                    }
                }
            }
        }
    }
}

async fn handle_incoming_tunnel(
    _peer_id: PeerId,
    stream: &mut libp2p::Stream,
    p2p: P2PHandle,
) -> Result<()> {
    use tokio::net::TcpStream;

    // 1. Read target address from stream
    let mut len_buf = [0u8; 2];
    libp2p::futures::AsyncReadExt::read_exact(stream, &mut len_buf)
        .await
        .context("Failed to read target len")?;
    let len = u16::from_be_bytes(len_buf) as usize;

    let mut buf = vec![0u8; len];
    libp2p::futures::AsyncReadExt::read_exact(stream, &mut buf)
        .await
        .context("Failed to read target addr")?;
    let target = String::from_utf8(buf).context("Invalid target addr UTF-8")?;

    info!("Tunneling to target: {}", target);

    if target.starts_with("peer:") {
        let next_peer_str = &target[5..];
        let next_peer: PeerId = next_peer_str
            .parse()
            .context("Invalid peer ID for multi-hop")?;

        match p2p
            .open_stream(next_peer, TUNNEL_PROTOCOL)
            .await
        {
            Ok(mut outbound) => {
                libp2p::futures::AsyncWriteExt::write_all(stream, &[0x00])
                    .await
                    .context("Failed to send success byte")?;

                let (mut ri, mut wi) = libp2p::futures::AsyncReadExt::split(stream);
                let (mut ro, mut wo) = libp2p::futures::AsyncReadExt::split(&mut outbound);

                let client_to_target = libp2p::futures::io::copy(&mut ri, &mut wo);
                let target_to_client = libp2p::futures::io::copy(&mut ro, &mut wi);

                tokio::select! {
                    res = client_to_target => debug!("Tunnel to target finished: {:?}", res),
                    res = target_to_client => debug!("Target to tunnel finished: {:?}", res),
                }
            }
            Err(e) => {
                error!("Failed to connect to next hop {}: {}", next_peer, e);
                let _ = libp2p::futures::AsyncWriteExt::write_all(stream, &[0x01]).await; // Failure byte
            }
        }
    } else {
        // 2. Connect to target directly
        match TcpStream::connect(&target).await {
            Ok(mut outbound) => {
                libp2p::futures::AsyncWriteExt::write_all(stream, &[0x00])
                    .await
                    .context("Failed to send success byte")?;

                // Bridge futures-based stream to tokio-based outbound
                use tokio_util::compat::FuturesAsyncReadCompatExt;
                use tokio_util::compat::FuturesAsyncWriteCompatExt;

                let (ri, wi) = libp2p::futures::AsyncReadExt::split(stream);
                let (mut ro, mut wo) = outbound.split();

                let mut ri_compat = ri.compat();
                let mut wi_compat = wi.compat_write();

                let client_to_target = tokio::io::copy(&mut ri_compat, &mut wo);
                let target_to_client = tokio::io::copy(&mut ro, &mut wi_compat);

                tokio::select! {
                    res = client_to_target => debug!("Tunnel to target finished: {:?}", res),
                    res = target_to_client => debug!("Target to tunnel finished: {:?}", res),
                }
            }
            Err(e) => {
                error!("Failed to connect to target {}: {}", target, e);
                let _ = libp2p::futures::AsyncWriteExt::write_all(stream, &[0x01]).await; // Failure byte
            }
        }
    }

    Ok(())
}
