use ipnet::IpNet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::sync::OnceLock;

/// SOCKS5 address type constants
pub const ATYP_IPV4: u8 = 0x01;
pub const ATYP_DOMAIN: u8 = 0x03;
pub const ATYP_IPV6: u8 = 0x04;

/// SOCKS5 command constants
pub const CMD_CONNECT: u8 = 0x01;

/// SOCKS5 reply codes
pub const REPLY_SUCCESS: u8 = 0x00;
pub const REPLY_GENERAL_FAILURE: u8 = 0x01;

/// SOCKS5 version
pub const SOCKS_VERSION: u8 = 0x05;

/// No-auth method
pub const AUTH_NONE: u8 = 0x00;
/// Username/password auth method
pub const AUTH_USER_PASS: u8 = 0x02;
/// No acceptable method
pub const AUTH_NO_ACCEPTABLE: u8 = 0xFF;

/// Source connection info extracted from SOCKS5 username field.
/// Format: "username|protocol|src_ip|src_port" (upstream tun2proxy v0.7.20+).
/// Legacy format with 5th field dst_ip is also accepted but no longer sent.
#[derive(Debug, Clone, Default)]
pub struct SourceInfo {
    pub username: String,
    pub protocol: String,
    pub src_ip: Option<IpAddr>,
    pub src_port: Option<u16>,
    pub dst_ip: Option<IpAddr>,
}

impl SourceInfo {
    /// Parse source info from username field.
    /// Expected format: "username|protocol|src_ip|src_port" (tun2proxy v0.7.20+).
    /// Also accepts legacy 5-field format with dst_ip.
    /// Falls back to treating entire string as username if parsing fails.
    pub fn parse(raw: &str) -> Self {
        let parts: Vec<&str> = raw.splitn(5, '|').collect();
        if parts.len() >= 4 {
            let src_ip = parts[2].parse::<IpAddr>().ok();
            let src_port = parts[3].parse::<u16>().ok();
            let dst_ip = if parts.len() >= 5 {
                parts[4].parse::<IpAddr>().ok()
            } else {
                None
            };
            Self {
                username: parts[0].to_string(),
                protocol: parts[1].to_string(),
                src_ip,
                src_port,
                dst_ip,
            }
        } else {
            Self {
                username: raw.to_string(),
                protocol: String::new(),
                src_ip: None,
                src_port: None,
                dst_ip: None,
            }
        }
    }

    pub fn has_source(&self) -> bool {
        self.src_ip.is_some()
    }
}

/// Telegram DC ranges from the official `core.telegram.org/resources/cidr.txt`
/// snapshot used by this repo.
const TELEGRAM_CIDRS: &[&str] = &[
    "91.108.56.0/22",
    "91.108.4.0/22",
    "91.108.8.0/22",
    "91.108.16.0/22",
    "91.108.12.0/22",
    "149.154.160.0/20",
    "91.105.192.0/23",
    "91.108.20.0/22",
    "185.76.151.0/24",
    "2001:b28:f23d::/48",
    "2001:b28:f23f::/48",
    "2001:67c:4e8::/48",
    "2001:b28:f23c::/48",
    "2a0a:f280::/32",
];

/// Validate SOCKS5 version byte. Returns error message if invalid.
pub fn validate_version(version: u8) -> Result<(), &'static str> {
    if version == SOCKS_VERSION {
        Ok(())
    } else {
        Err("Invalid SOCKS version")
    }
}

/// Check if the method list contains no-auth (0x00).
pub fn supports_no_auth(methods: &[u8]) -> bool {
    methods.contains(&AUTH_NONE)
}

/// Build the method selection reply.
pub fn method_selection_reply(method: u8) -> [u8; 2] {
    [SOCKS_VERSION, method]
}

/// Validate SOCKS5 request header (version + command).
/// Returns Ok(address_type) or Err with description.
pub fn validate_request_header(header: &[u8; 4]) -> Result<u8, &'static str> {
    if header[0] != SOCKS_VERSION {
        return Err("Invalid SOCKS version in request");
    }
    if header[1] != CMD_CONNECT {
        return Err("Only CONNECT command is supported");
    }
    Ok(header[3])
}

/// Parse an IPv4 address + port from raw bytes into "ip:port" string.
pub fn parse_ipv4_target(addr_bytes: &[u8; 4], port: u16) -> String {
    format!(
        "{}.{}.{}.{}:{}",
        addr_bytes[0], addr_bytes[1], addr_bytes[2], addr_bytes[3], port
    )
}

/// Parse a domain name + port into "domain:port" string.
pub fn parse_domain_target(domain: &[u8], port: u16) -> String {
    format!("{}:{}", String::from_utf8_lossy(domain), port)
}

/// Parse an IPv6 address + port from raw bytes into "[hex]:port" string.
pub fn parse_ipv6_target(addr_bytes: &[u8; 16], port: u16) -> String {
    let addr = Ipv6Addr::from(*addr_bytes);
    format!("[{}]:{}", addr, port)
}

/// Build a SOCKS5 success reply (CONNECT granted, bound to 0.0.0.0:0).
pub fn success_reply() -> [u8; 10] {
    [
        SOCKS_VERSION,
        REPLY_SUCCESS,
        0x00,
        ATYP_IPV4,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
}

/// Build a SOCKS5 failure reply (general SOCKS server failure).
pub fn failure_reply() -> [u8; 10] {
    [
        SOCKS_VERSION,
        REPLY_GENERAL_FAILURE,
        0x00,
        ATYP_IPV4,
        0,
        0,
        0,
        0,
        0,
        0,
    ]
}

/// Hydra relay infrastructure domains — connections to these must always be direct
/// to prevent routing loops when VPN is in "full" mode.
const RELAY_DOMAINS: &[&str] = &[
    "relay.hydra-net.work",
    "hydra-relay.hydra-net.workers.dev",
    "boot.ze1.org",
];

fn telegram_networks() -> &'static [IpNet] {
    static NETWORKS: OnceLock<Vec<IpNet>> = OnceLock::new();
    NETWORKS
        .get_or_init(|| {
            TELEGRAM_CIDRS
                .iter()
                .map(|cidr| {
                    cidr.parse::<IpNet>()
                        .unwrap_or_else(|error| panic!("Invalid Telegram CIDR {cidr}: {error}"))
                })
                .collect()
        })
        .as_slice()
}

fn normalized_target_host(target: &str) -> &str {
    split_target(target)
        .map(|(host, _)| host)
        .unwrap_or(target)
        .trim_start_matches('[')
        .trim_end_matches(']')
}

/// Check if a target address points to Hydra relay infrastructure.
/// These must never be proxied to avoid routing loops.
pub fn is_relay_infrastructure(target: &str) -> bool {
    let host = normalized_target_host(target);
    RELAY_DOMAINS
        .iter()
        .any(|d| host == *d || host.ends_with(*d))
}

/// Check if a target address (as "ip:port" string) points to a Telegram DC.
pub fn is_telegram_target(target: &str) -> bool {
    let host = normalized_target_host(target);
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        let addr = IpAddr::V4(ip);
        telegram_networks()
            .iter()
            .any(|network| network.contains(&addr))
    } else if let Ok(ip) = host.parse::<Ipv6Addr>() {
        let addr = IpAddr::V6(ip);
        telegram_networks()
            .iter()
            .any(|network| network.contains(&addr))
    } else {
        host.contains("telegram.org") || host.contains("t.me") || host.contains("telegram-cdn.org")
    }
}

/// Extract host:port from a target address string.
/// Returns (host, port) tuple.
pub fn split_target(target: &str) -> Option<(&str, u16)> {
    if target.starts_with('[') {
        // IPv6: [addr]:port
        let end_bracket = target.find(']')?;
        let host = &target[..=end_bracket];
        let port_str = target.get(end_bracket + 2..)?;
        let port = port_str.parse().ok()?;
        Some((host, port))
    } else {
        let mut parts = target.rsplitn(2, ':');
        let port_str = parts.next()?;
        let host = parts.next()?;
        let port = port_str.parse().ok()?;
        Some((host, port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_version() {
        assert!(validate_version(0x05).is_ok());
        assert!(validate_version(0x04).is_err());
        assert!(validate_version(0x00).is_err());
    }

    #[test]
    fn test_supports_no_auth() {
        assert!(supports_no_auth(&[0x00]));
        assert!(supports_no_auth(&[0x01, 0x00, 0x02]));
        assert!(!supports_no_auth(&[0x01, 0x02]));
        assert!(!supports_no_auth(&[]));
    }

    #[test]
    fn test_method_selection_reply() {
        assert_eq!(method_selection_reply(AUTH_NONE), [0x05, 0x00]);
        assert_eq!(method_selection_reply(AUTH_NO_ACCEPTABLE), [0x05, 0xFF]);
    }

    #[test]
    fn test_validate_request_header() {
        assert_eq!(
            validate_request_header(&[0x05, 0x01, 0x00, 0x01]),
            Ok(ATYP_IPV4)
        );
        assert_eq!(
            validate_request_header(&[0x05, 0x01, 0x00, 0x03]),
            Ok(ATYP_DOMAIN)
        );
        assert_eq!(
            validate_request_header(&[0x05, 0x01, 0x00, 0x04]),
            Ok(ATYP_IPV6)
        );
        assert!(validate_request_header(&[0x04, 0x01, 0x00, 0x01]).is_err());
        assert!(validate_request_header(&[0x05, 0x02, 0x00, 0x01]).is_err());
    }

    #[test]
    fn test_parse_ipv4_target() {
        assert_eq!(
            parse_ipv4_target(&[149, 154, 167, 50], 443),
            "149.154.167.50:443"
        );
        assert_eq!(parse_ipv4_target(&[127, 0, 0, 1], 1080), "127.0.0.1:1080");
        assert_eq!(parse_ipv4_target(&[0, 0, 0, 0], 0), "0.0.0.0:0");
    }

    #[test]
    fn test_parse_domain_target() {
        assert_eq!(
            parse_domain_target(b"telegram.org", 443),
            "telegram.org:443"
        );
        assert_eq!(parse_domain_target(b"example.com", 80), "example.com:80");
    }

    #[test]
    fn test_parse_ipv6_target() {
        let addr: [u8; 16] = [0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let result = parse_ipv6_target(&addr, 443);
        assert!(result.starts_with('['));
        assert!(result.ends_with(":443"));
        assert!(result.contains("2001:db8"));
    }

    #[test]
    fn test_parse_ipv6_loopback() {
        let addr: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
        let result = parse_ipv6_target(&addr, 8080);
        assert_eq!(result, "[::1]:8080");
    }

    #[test]
    fn test_success_reply() {
        let reply = success_reply();
        assert_eq!(reply[0], SOCKS_VERSION);
        assert_eq!(reply[1], REPLY_SUCCESS);
        assert_eq!(reply.len(), 10);
    }

    #[test]
    fn test_failure_reply() {
        let reply = failure_reply();
        assert_eq!(reply[0], SOCKS_VERSION);
        assert_eq!(reply[1], REPLY_GENERAL_FAILURE);
        assert_eq!(reply.len(), 10);
    }

    #[test]
    fn test_is_telegram_target_ipv4() {
        assert!(is_telegram_target("149.154.167.50:443"));
        assert!(is_telegram_target("149.154.160.1:80"));
        assert!(is_telegram_target("91.108.56.100:443"));
        assert!(is_telegram_target("91.108.4.1:443"));
        assert!(is_telegram_target("185.76.151.1:443"));
        assert!(!is_telegram_target("8.8.8.8:53"));
        assert!(!is_telegram_target("1.1.1.1:443"));
        assert!(!is_telegram_target("192.168.1.1:80"));
    }

    #[test]
    fn test_is_telegram_target_ipv6() {
        assert!(is_telegram_target("[2001:b28:f23d:f001::a]:443"));
        assert!(is_telegram_target("[2a0a:f280:0203::1]:443"));
        assert!(!is_telegram_target("[2001:db8::1]:443"));
    }

    #[test]
    fn test_is_telegram_target_domain() {
        assert!(is_telegram_target("telegram.org:443"));
        assert!(is_telegram_target("core.telegram.org:443"));
        assert!(is_telegram_target("t.me:443"));
        assert!(!is_telegram_target("google.com:443"));
        assert!(!is_telegram_target("example.com:80"));
    }

    #[test]
    fn test_split_target_ipv4() {
        assert_eq!(split_target("1.2.3.4:443"), Some(("1.2.3.4", 443)));
        assert_eq!(split_target("127.0.0.1:1080"), Some(("127.0.0.1", 1080)));
    }

    #[test]
    fn test_split_target_domain() {
        assert_eq!(split_target("example.com:80"), Some(("example.com", 80)));
        assert_eq!(
            split_target("telegram.org:443"),
            Some(("telegram.org", 443))
        );
    }

    #[test]
    fn test_split_target_ipv6() {
        assert_eq!(split_target("[::1]:8080"), Some(("[::1]", 8080)));
        assert_eq!(
            split_target("[2001:db8::1]:443"),
            Some(("[2001:db8::1]", 443))
        );
    }

    #[test]
    fn test_split_target_invalid() {
        assert_eq!(split_target("no_port"), None);
        assert_eq!(split_target("host:notaport"), None);
    }

    #[test]
    fn test_is_relay_infrastructure() {
        assert!(is_relay_infrastructure("relay.hydra-net.work:443"));
        assert!(is_relay_infrastructure(
            "hydra-relay.hydra-net.workers.dev:443"
        ));
        assert!(is_relay_infrastructure("boot.ze1.org:22"));
        assert!(!is_relay_infrastructure("google.com:443"));
        assert!(!is_relay_infrastructure("149.154.167.50:443"));
    }

    #[test]
    fn test_source_info_parse_full() {
        let info = SourceInfo::parse("hydra|tcp|192.168.1.100|54321");
        assert_eq!(info.username, "hydra");
        assert_eq!(info.protocol, "tcp");
        assert_eq!(info.src_ip, Some("192.168.1.100".parse().unwrap()));
        assert_eq!(info.src_port, Some(54321));
        assert!(info.has_source());
    }

    #[test]
    fn test_source_info_parse_ipv6() {
        let info = SourceInfo::parse("user|udp|::1|12345");
        assert_eq!(info.username, "user");
        assert_eq!(info.protocol, "udp");
        assert_eq!(info.src_ip, Some("::1".parse().unwrap()));
        assert_eq!(info.src_port, Some(12345));
    }

    #[test]
    fn test_source_info_parse_fallback() {
        let info = SourceInfo::parse("simple_username");
        assert_eq!(info.username, "simple_username");
        assert_eq!(info.protocol, "");
        assert_eq!(info.src_ip, None);
        assert_eq!(info.src_port, None);
        assert!(!info.has_source());
    }

    #[test]
    fn test_source_info_parse_partial() {
        let info = SourceInfo::parse("user|tcp");
        assert_eq!(info.username, "user|tcp");
        assert!(!info.has_source());
    }
}
