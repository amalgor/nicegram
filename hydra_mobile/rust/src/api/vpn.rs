use std::sync::atomic::AtomicU16;

/// Stores the actual SOCKS5 port loaded from config at node startup.
pub static SOCKS5_PORT: AtomicU16 = AtomicU16::new(1080);
