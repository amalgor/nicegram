use super::{Transport, TransportStream};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use leaf::app::{SyncDnsClient, dns_client::DnsClient};
use leaf::proxy::outbound::HandlerBuilder;
use leaf::proxy::{self, AnyOutboundHandler};
use leaf::session::{Network, Session, SocksAddr};
use protobuf::MessageField;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::sync::RwLock;
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

    let vless = Arc::new(leaf::proxy::vless::outbound::StreamHandler {
        address: config.address.clone(),
        port: config.port,
        uuid: config.uuid.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;

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
