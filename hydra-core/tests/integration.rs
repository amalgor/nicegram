use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

fn empty_transports() -> Vec<hydra_core::transport::ConfiguredTransport> {
    Vec::new()
}

/// Test SOCKS5 handshake and direct connection to a local echo server.
/// Validates the full path: client -> SOCKS5 -> target.
#[tokio::test]
async fn test_socks5_direct_connection() {
    // 1. Start a simple TCP echo server as the target
    let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_addr = echo_listener.local_addr().unwrap();

    tokio::spawn(async move {
        loop {
            let (mut stream, _) = echo_listener.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = [0u8; 1024];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            stream.write_all(&buf[..n]).await.ok();
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    });

    // 2. Start the Socks5Server
    let config = hydra_config::HydraConfig::default();
    let ai = Arc::new(hydra_ai::AiNegotiator::new(&config.ai));

    let p2p_config = hydra_config::NetworkConfig {
        socks5_port: 0,
        p2p_listen_port: 0,
        bootstrap_nodes: vec![],
        ..Default::default()
    };
    let (p2p_node, p2p_handle) = hydra_p2p::P2PNode::new(None, 0, &p2p_config).await.unwrap();
    tokio::spawn(async move {
        p2p_node.run().await.ok();
    });

    let _econ_dir = tempfile::tempdir().unwrap();
    let econ = Arc::new(hydra_econ::EconLedger::new(_econ_dir.path().to_str().unwrap()).unwrap());

    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();
    drop(socks_listener);

    let server = hydra_core::Socks5Server::new(
        socks_addr,
        ai,
        p2p_handle,
        econ,
        empty_transports(),
        "off".to_string(),
        None,
        None,
        None,
    );
    tokio::spawn(async move {
        server.run().await.ok();
    });

    // Give server time to bind
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 3. Connect as a SOCKS5 client
    let mut client = TcpStream::connect(socks_addr).await.unwrap();

    // Handshake: version 5, 1 method (no auth)
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00], "Server should accept no-auth");

    // CONNECT request to echo server (IPv4)
    let ip = match echo_addr {
        SocketAddr::V4(v4) => v4.ip().octets(),
        _ => panic!("Expected IPv4"),
    };
    let port = echo_addr.port().to_be_bytes();
    let mut connect_req = vec![0x05, 0x01, 0x00, 0x01];
    connect_req.extend_from_slice(&ip);
    connect_req.extend_from_slice(&port);
    client.write_all(&connect_req).await.unwrap();

    let mut connect_resp = [0u8; 10];
    client.read_exact(&mut connect_resp).await.unwrap();
    assert_eq!(connect_resp[0], 0x05, "SOCKS version");
    assert_eq!(connect_resp[1], 0x00, "Connection should succeed");

    // 4. Send data through the tunnel and verify echo
    let test_data = b"Hello through SOCKS5!";
    client.write_all(test_data).await.unwrap();

    let mut echo_buf = vec![0u8; test_data.len()];
    client.read_exact(&mut echo_buf).await.unwrap();
    assert_eq!(&echo_buf, test_data, "Echo server should return same data");
}

/// Test SOCKS5 handshake rejection for unsupported auth methods.
#[tokio::test]
async fn test_socks5_rejects_unsupported_auth() {
    let config = hydra_config::HydraConfig::default();
    let ai = Arc::new(hydra_ai::AiNegotiator::new(&config.ai));

    let p2p_config = hydra_config::NetworkConfig {
        socks5_port: 0,
        p2p_listen_port: 0,
        bootstrap_nodes: vec![],
        ..Default::default()
    };
    let (p2p_node, p2p_handle) = hydra_p2p::P2PNode::new(None, 0, &p2p_config).await.unwrap();
    tokio::spawn(async move {
        p2p_node.run().await.ok();
    });

    let econ = Arc::new({
        let dir = tempfile::tempdir().unwrap();
        hydra_econ::EconLedger::new(dir.path().to_str().unwrap()).unwrap()
    });

    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();
    drop(socks_listener);

    let server = hydra_core::Socks5Server::new(
        socks_addr,
        ai,
        p2p_handle,
        econ,
        empty_transports(),
        "off".to_string(),
        None,
        None,
        None,
    );
    tokio::spawn(async move {
        server.run().await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut client = TcpStream::connect(socks_addr).await.unwrap();

    // Offer only username/password auth (0x02), no no-auth
    client.write_all(&[0x05, 0x01, 0x02]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0xFF], "Server should reject with 0xFF");
}

/// Test SOCKS5 connection to domain name target.
#[tokio::test]
async fn test_socks5_domain_connect() {
    let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_port = echo_listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = echo_listener.accept().await {
            let mut buf = [0u8; 256];
            if let Ok(n) = stream.read(&mut buf).await {
                stream.write_all(&buf[..n]).await.ok();
            }
        }
    });

    let config = hydra_config::HydraConfig::default();
    let ai = Arc::new(hydra_ai::AiNegotiator::new(&config.ai));

    let p2p_config = hydra_config::NetworkConfig {
        socks5_port: 0,
        p2p_listen_port: 0,
        bootstrap_nodes: vec![],
        ..Default::default()
    };
    let (p2p_node, p2p_handle) = hydra_p2p::P2PNode::new(None, 0, &p2p_config).await.unwrap();
    tokio::spawn(async move {
        p2p_node.run().await.ok();
    });

    let econ = Arc::new({
        let dir = tempfile::tempdir().unwrap();
        hydra_econ::EconLedger::new(dir.path().to_str().unwrap()).unwrap()
    });

    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();
    drop(socks_listener);

    let server = hydra_core::Socks5Server::new(
        socks_addr,
        ai,
        p2p_handle,
        econ,
        empty_transports(),
        "off".to_string(),
        None,
        None,
        None,
    );
    tokio::spawn(async move {
        server.run().await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    let mut client = TcpStream::connect(socks_addr).await.unwrap();

    // Handshake
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();
    assert_eq!(resp, [0x05, 0x00]);

    // CONNECT to domain "localhost" with the echo server port
    let domain = b"localhost";
    let port_bytes = echo_port.to_be_bytes();
    let mut req = vec![0x05, 0x01, 0x00, 0x03, domain.len() as u8];
    req.extend_from_slice(domain);
    req.extend_from_slice(&port_bytes);
    client.write_all(&req).await.unwrap();

    let mut connect_resp = [0u8; 10];
    client.read_exact(&mut connect_resp).await.unwrap();
    assert_eq!(connect_resp[1], 0x00, "Domain connect should succeed");

    // Verify data flows
    client.write_all(b"domain-test").await.unwrap();
    let mut buf = [0u8; 11];
    client.read_exact(&mut buf).await.unwrap();
    assert_eq!(&buf, b"domain-test");
}

/// Test connection registry tracking during SOCKS5 session.
#[tokio::test]
async fn test_connection_registry_tracking() {
    let echo_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let echo_addr = echo_listener.local_addr().unwrap();

    tokio::spawn(async move {
        if let Ok((mut stream, _)) = echo_listener.accept().await {
            let mut buf = [0u8; 256];
            if let Ok(n) = stream.read(&mut buf).await {
                stream.write_all(&buf[..n]).await.ok();
            }
            // Close after one exchange
        }
    });

    let config = hydra_config::HydraConfig::default();
    let ai = Arc::new(hydra_ai::AiNegotiator::new(&config.ai));

    let p2p_config = hydra_config::NetworkConfig {
        socks5_port: 0,
        p2p_listen_port: 0,
        bootstrap_nodes: vec![],
        ..Default::default()
    };
    let (p2p_node, p2p_handle) = hydra_p2p::P2PNode::new(None, 0, &p2p_config).await.unwrap();
    tokio::spawn(async move {
        p2p_node.run().await.ok();
    });

    let econ = Arc::new({
        let dir = tempfile::tempdir().unwrap();
        hydra_econ::EconLedger::new(dir.path().to_str().unwrap()).unwrap()
    });

    let socks_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let socks_addr = socks_listener.local_addr().unwrap();
    drop(socks_listener);

    let server = hydra_core::Socks5Server::new(
        socks_addr,
        ai,
        p2p_handle,
        econ,
        empty_transports(),
        "off".to_string(),
        None,
        None,
        None,
    );
    let registry = server.registry();
    tokio::spawn(async move {
        server.run().await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Initially no connections
    assert_eq!(registry.stats().total_count, 0);

    // Make a SOCKS5 connection
    let mut client = TcpStream::connect(socks_addr).await.unwrap();
    client.write_all(&[0x05, 0x01, 0x00]).await.unwrap();
    let mut resp = [0u8; 2];
    client.read_exact(&mut resp).await.unwrap();

    let ip = match echo_addr {
        SocketAddr::V4(v4) => v4.ip().octets(),
        _ => panic!("Expected IPv4"),
    };
    let port = echo_addr.port().to_be_bytes();
    let mut req = vec![0x05, 0x01, 0x00, 0x01];
    req.extend_from_slice(&ip);
    req.extend_from_slice(&port);
    client.write_all(&req).await.unwrap();

    let mut connect_resp = [0u8; 10];
    client.read_exact(&mut connect_resp).await.unwrap();
    assert_eq!(connect_resp[1], 0x00);

    // Send data and close
    client.write_all(b"track-me").await.unwrap();
    let mut buf = [0u8; 8];
    client.read_exact(&mut buf).await.unwrap();
    drop(client);

    // Give time for connection to close and be tracked
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let stats = registry.stats();
    assert!(
        stats.total_count >= 1,
        "Should have at least 1 tracked connection"
    );
    assert!(
        stats.total_bytes_up + stats.total_bytes_down > 0,
        "Should have tracked bytes"
    );
}

/// Test AI routing fallback (no model, no peers -> direct).
#[tokio::test]
async fn test_ai_routing_fallback_direct() {
    let config = hydra_config::AiConfig::default();
    let ai = hydra_ai::AiNegotiator::new(&config);

    let request = hydra_ai::RouteRequest {
        target: "example.com:443".to_string(),
        protocol: "tcp".to_string(),
        peers: vec![],
        diagnostic_context: None,
    };

    let result = ai.decide_route(request).await.unwrap();
    assert!(result.path.is_empty(), "No peers -> direct route");
    assert_eq!(result.transport, "raw");
}

/// Test AI routing with peers (fallback heuristic selects best peer).
#[tokio::test]
async fn test_ai_routing_selects_best_peer() {
    let config = hydra_config::AiConfig::default();
    let ai = hydra_ai::AiNegotiator::new(&config);

    let request = hydra_ai::RouteRequest {
        target: "149.154.167.50:443".to_string(),
        protocol: "tcp".to_string(),
        peers: vec![
            hydra_ai::PeerInfo {
                peer_id: "unreliable".to_string(),
                trust_score: 20,
                current_debt: 0,
                rtt_ms: Some(10),
                bandwidth_bps: Some(1_000_000),
            },
            hydra_ai::PeerInfo {
                peer_id: "trusted-fast".to_string(),
                trust_score: 95,
                current_debt: 100,
                rtt_ms: Some(5),
                bandwidth_bps: Some(10_000_000),
            },
        ],
        diagnostic_context: None,
    };

    let result = ai.decide_route(request).await.unwrap();
    assert_eq!(
        result.path,
        vec!["trusted-fast"],
        "Should select highest-trust peer"
    );
    assert_eq!(result.transport, "vless");
}

/// Test Telegram target detection in connection registry.
#[tokio::test]
async fn test_telegram_detection_in_registry() {
    let registry = hydra_core::connections::ConnectionRegistry::new();

    let tg_id = registry.register("149.154.167.50:443", true);
    let normal_id = registry.register("8.8.8.8:53", false);

    let snaps = registry.snapshot(false);
    let tg_snap = snaps.iter().find(|s| s.id == tg_id).unwrap();
    let normal_snap = snaps.iter().find(|s| s.id == normal_id).unwrap();

    assert!(
        tg_snap.is_telegram,
        "149.154.x.x should be detected as Telegram"
    );
    assert!(!normal_snap.is_telegram, "8.8.8.8 should not be Telegram");
}
