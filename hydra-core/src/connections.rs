use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use crate::socks;

static NEXT_CONN_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteType {
    Direct,
    Relay,
    P2P,
}

impl RouteType {
    pub fn as_str(&self) -> &'static str {
        match self {
            RouteType::Direct => "direct",
            RouteType::Relay => "relay",
            RouteType::P2P => "p2p",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnStatus {
    Active,
    Closed,
}

impl ConnStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ConnStatus::Active => "active",
            ConnStatus::Closed => "closed",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub id: u64,
    pub target_host: String,
    pub target_port: u16,
    pub route_type: RouteType,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub start_time: Instant,
    pub is_telegram: bool,
    pub is_proxied: bool,
    pub force_proxy: Option<bool>,
    pub status: ConnStatus,
    pub ai_reason: Option<String>,
}

/// Serializable snapshot for FFI/FRB
#[derive(Debug, Clone)]
pub struct ConnectionSnapshot {
    pub id: u64,
    pub target_host: String,
    pub target_port: u16,
    pub route_type: String,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub duration_ms: u64,
    pub is_telegram: bool,
    pub is_proxied: bool,
    pub status: String,
    pub ai_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ConnectionStats {
    pub active_count: usize,
    pub total_count: usize,
    pub proxied_count: usize,
    pub total_bytes_up: u64,
    pub total_bytes_down: u64,
}

#[derive(Debug, Clone)]
pub struct ConnectionRegistry {
    inner: Arc<RwLock<HashMap<u64, ConnectionInfo>>>,
}

impl ConnectionRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register(&self, target: &str, is_proxied: bool) -> u64 {
        let id = NEXT_CONN_ID.fetch_add(1, Ordering::Relaxed);
        let (host, port) = socks::split_target(target).unwrap_or((target, 0));
        let is_telegram = socks::is_telegram_target(target);

        let info = ConnectionInfo {
            id,
            target_host: host.to_string(),
            target_port: port,
            route_type: RouteType::Direct,
            bytes_up: 0,
            bytes_down: 0,
            start_time: Instant::now(),
            is_telegram,
            is_proxied,
            force_proxy: None,
            status: ConnStatus::Active,
            ai_reason: None,
        };

        self.inner.write().unwrap().insert(id, info);
        id
    }

    pub fn update_route(&self, id: u64, route_type: RouteType, ai_reason: Option<String>) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.route_type = route_type;
            conn.is_proxied = route_type != RouteType::Direct;
            if ai_reason.is_some() {
                conn.ai_reason = ai_reason;
            }
        }
    }

    pub fn update_bytes(&self, id: u64, bytes_up: u64, bytes_down: u64) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.bytes_up += bytes_up;
            conn.bytes_down += bytes_down;
        }
    }

    pub fn close(&self, id: u64) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.status = ConnStatus::Closed;
        }
    }

    pub fn set_force_proxy(&self, id: u64, force: bool) {
        if let Some(conn) = self.inner.write().unwrap().get_mut(&id) {
            conn.force_proxy = Some(force);
        }
    }

    pub fn get_force_proxy(&self, id: u64) -> Option<bool> {
        self.inner.read().unwrap().get(&id).and_then(|c| c.force_proxy)
    }

    pub fn should_proxy(&self, target: &str) -> bool {
        socks::is_telegram_target(target)
    }

    pub fn snapshot(&self, active_only: bool) -> Vec<ConnectionSnapshot> {
        let guard = self.inner.read().unwrap();
        let now = Instant::now();
        guard
            .values()
            .filter(|c| !active_only || c.status == ConnStatus::Active)
            .map(|c| ConnectionSnapshot {
                id: c.id,
                target_host: c.target_host.clone(),
                target_port: c.target_port,
                route_type: c.route_type.as_str().to_string(),
                bytes_up: c.bytes_up,
                bytes_down: c.bytes_down,
                duration_ms: now.duration_since(c.start_time).as_millis() as u64,
                is_telegram: c.is_telegram,
                is_proxied: c.is_proxied,
                status: c.status.as_str().to_string(),
                ai_reason: c.ai_reason.clone(),
            })
            .collect()
    }

    pub fn stats(&self) -> ConnectionStats {
        let guard = self.inner.read().unwrap();
        let mut stats = ConnectionStats {
            active_count: 0,
            total_count: guard.len(),
            proxied_count: 0,
            total_bytes_up: 0,
            total_bytes_down: 0,
        };
        for c in guard.values() {
            if c.status == ConnStatus::Active {
                stats.active_count += 1;
            }
            if c.is_proxied {
                stats.proxied_count += 1;
            }
            stats.total_bytes_up += c.bytes_up;
            stats.total_bytes_down += c.bytes_down;
        }
        stats
    }

    /// Remove closed connections older than max_age to prevent unbounded growth.
    pub fn gc(&self, max_closed: usize) {
        let mut guard = self.inner.write().unwrap();
        let mut closed: Vec<(u64, Instant)> = guard
            .iter()
            .filter(|(_, c)| c.status == ConnStatus::Closed)
            .map(|(id, c)| (*id, c.start_time))
            .collect();

        if closed.len() > max_closed {
            closed.sort_by_key(|(_, t)| *t);
            let to_remove = closed.len() - max_closed;
            for (id, _) in closed.into_iter().take(to_remove) {
                guard.remove(&id);
            }
        }
    }
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_snapshot() {
        let reg = ConnectionRegistry::new();
        let id = reg.register("149.154.167.50:443", false);
        assert!(id > 0);

        let snaps = reg.snapshot(true);
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].target_host, "149.154.167.50");
        assert_eq!(snaps[0].target_port, 443);
        assert!(snaps[0].is_telegram);
        assert_eq!(snaps[0].status, "active");
    }

    #[test]
    fn test_update_route_and_bytes() {
        let reg = ConnectionRegistry::new();
        let id = reg.register("example.com:80", false);

        reg.update_route(id, RouteType::Relay, Some("AI chose relay".to_string()));
        reg.update_bytes(id, 1000, 2000);
        reg.update_bytes(id, 500, 300);

        let snaps = reg.snapshot(false);
        let conn = snaps.iter().find(|s| s.id == id).unwrap();
        assert_eq!(conn.route_type, "relay");
        assert_eq!(conn.bytes_up, 1500);
        assert_eq!(conn.bytes_down, 2300);
        assert!(conn.is_proxied);
        assert_eq!(conn.ai_reason.as_deref(), Some("AI chose relay"));
    }

    #[test]
    fn test_close_connection() {
        let reg = ConnectionRegistry::new();
        let id = reg.register("1.2.3.4:80", false);

        assert_eq!(reg.snapshot(true).len(), 1);
        reg.close(id);
        assert_eq!(reg.snapshot(true).len(), 0);
        assert_eq!(reg.snapshot(false).len(), 1);
    }

    #[test]
    fn test_telegram_detection() {
        let reg = ConnectionRegistry::new();
        assert!(reg.should_proxy("149.154.167.50:443"));
        assert!(reg.should_proxy("91.108.56.100:443"));
        assert!(!reg.should_proxy("8.8.8.8:53"));
    }

    #[test]
    fn test_force_proxy() {
        let reg = ConnectionRegistry::new();
        let id = reg.register("example.com:443", false);

        assert_eq!(reg.get_force_proxy(id), None);
        reg.set_force_proxy(id, true);
        assert_eq!(reg.get_force_proxy(id), Some(true));
    }

    #[test]
    fn test_stats() {
        let reg = ConnectionRegistry::new();
        let id1 = reg.register("149.154.167.50:443", true);
        let id2 = reg.register("google.com:443", false);

        reg.update_route(id1, RouteType::Relay, None);
        reg.update_bytes(id1, 100, 200);
        reg.update_bytes(id2, 50, 75);
        reg.close(id2);

        let stats = reg.stats();
        assert_eq!(stats.active_count, 1);
        assert_eq!(stats.total_count, 2);
        assert_eq!(stats.proxied_count, 1);
        assert_eq!(stats.total_bytes_up, 150);
        assert_eq!(stats.total_bytes_down, 275);
    }

    #[test]
    fn test_gc_removes_oldest_closed() {
        let reg = ConnectionRegistry::new();
        let id1 = reg.register("a.com:80", false);
        let id2 = reg.register("b.com:80", false);
        let id3 = reg.register("c.com:80", false);

        reg.close(id1);
        reg.close(id2);
        reg.close(id3);

        reg.gc(1);
        let snaps = reg.snapshot(false);
        assert_eq!(snaps.len(), 1);
    }

    #[test]
    fn test_unique_ids() {
        let reg = ConnectionRegistry::new();
        let id1 = reg.register("a.com:80", false);
        let id2 = reg.register("b.com:80", false);
        assert_ne!(id1, id2);
    }
}
