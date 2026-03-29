use std::net::{Ipv4Addr, Ipv6Addr};

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
/// No acceptable method
pub const AUTH_NO_ACCEPTABLE: u8 = 0xFF;

/// Telegram DC IP subnets (used for auto-proxy detection)
pub const TELEGRAM_SUBNETS: &[(u8, u8)] = &[
    (149, 154), // 149.154.0.0/16
    (91, 108),  // 91.108.0.0/16
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
    [SOCKS_VERSION, REPLY_SUCCESS, 0x00, ATYP_IPV4, 0, 0, 0, 0, 0, 0]
}

/// Build a SOCKS5 failure reply (general SOCKS server failure).
pub fn failure_reply() -> [u8; 10] {
    [SOCKS_VERSION, REPLY_GENERAL_FAILURE, 0x00, ATYP_IPV4, 0, 0, 0, 0, 0, 0]
}

/// Check if a target address (as "ip:port" string) points to a Telegram DC.
pub fn is_telegram_target(target: &str) -> bool {
    let host = target.split(':').next().unwrap_or("");
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        let octets = ip.octets();
        TELEGRAM_SUBNETS
            .iter()
            .any(|(a, b)| octets[0] == *a && octets[1] == *b)
    } else {
        host.contains("telegram.org")
            || host.contains("t.me")
            || host.contains("core.telegram.org")
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
        assert_eq!(
            parse_ipv4_target(&[127, 0, 0, 1], 1080),
            "127.0.0.1:1080"
        );
        assert_eq!(
            parse_ipv4_target(&[0, 0, 0, 0], 0),
            "0.0.0.0:0"
        );
    }

    #[test]
    fn test_parse_domain_target() {
        assert_eq!(
            parse_domain_target(b"telegram.org", 443),
            "telegram.org:443"
        );
        assert_eq!(
            parse_domain_target(b"example.com", 80),
            "example.com:80"
        );
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
        assert!(is_telegram_target("149.154.0.1:80"));
        assert!(is_telegram_target("91.108.56.100:443"));
        assert!(is_telegram_target("91.108.4.1:443"));
        assert!(!is_telegram_target("8.8.8.8:53"));
        assert!(!is_telegram_target("1.1.1.1:443"));
        assert!(!is_telegram_target("192.168.1.1:80"));
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
}
