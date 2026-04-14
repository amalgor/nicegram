use crate::shell_executor::{CommandResult, ShellExecutor};
use std::collections::HashMap;
use tracing::debug;

/// Pre-built inspection commands for Android device analysis.
/// These commands are designed to run within the app sandbox without root.
pub struct DeviceInspector {
    executor: ShellExecutor,
}

/// Structured summary of device network state collected via shell commands.
#[derive(Debug, Clone, Default)]
pub struct NetworkInspection {
    /// Active TCP connections from /proc/net/tcp
    pub tcp_connections: Option<String>,
    /// Active TCP6 connections from /proc/net/tcp6
    pub tcp6_connections: Option<String>,
    /// Active UDP sockets from /proc/net/udp
    pub udp_sockets: Option<String>,
    /// Running processes visible to the app
    pub processes: Option<String>,
    /// Network interface info
    pub net_interfaces: Option<String>,
    /// DNS resolver configuration
    pub dns_config: Option<String>,
    /// Routing table
    pub routes: Option<String>,
    /// Per-UID network statistics (if accessible)
    pub uid_stats: Option<String>,
    /// Errors encountered during inspection
    pub errors: Vec<String>,
}

impl DeviceInspector {
    pub fn new() -> Self {
        Self {
            executor: ShellExecutor::new(),
        }
    }

    /// Run a full network inspection, collecting all available data.
    /// Each sub-command is best-effort: failures are logged but don't stop other commands.
    pub async fn inspect_network(&self) -> NetworkInspection {
        let mut result = NetworkInspection::default();

        let commands: Vec<(&str, &str)> = vec![
            ("tcp_connections", "cat /proc/net/tcp 2>/dev/null"),
            ("tcp6_connections", "cat /proc/net/tcp6 2>/dev/null"),
            ("udp_sockets", "cat /proc/net/udp 2>/dev/null"),
            ("processes", "ps -e -o PID,UID,COMM 2>/dev/null || ps 2>/dev/null"),
            ("net_interfaces", "ip addr 2>/dev/null || ifconfig 2>/dev/null || cat /proc/net/if_inet6 2>/dev/null"),
            ("dns_config", "cat /etc/resolv.conf 2>/dev/null; getprop net.dns1 2>/dev/null; getprop net.dns2 2>/dev/null"),
            ("routes", "ip route 2>/dev/null || cat /proc/net/route 2>/dev/null"),
            ("uid_stats", "cat /proc/net/xt_qtaguid/stats 2>/dev/null || cat /proc/uid_stat/*/tcp_snd 2>/dev/null"),
        ];

        for (name, cmd) in &commands {
            match self.executor.exec(cmd).await {
                Ok(cr) if cr.exit_code == Some(0) && !cr.stdout.trim().is_empty() => {
                    let output = cr.stdout;
                    match *name {
                        "tcp_connections" => result.tcp_connections = Some(output),
                        "tcp6_connections" => result.tcp6_connections = Some(output),
                        "udp_sockets" => result.udp_sockets = Some(output),
                        "processes" => result.processes = Some(output),
                        "net_interfaces" => result.net_interfaces = Some(output),
                        "dns_config" => result.dns_config = Some(output),
                        "routes" => result.routes = Some(output),
                        "uid_stats" => result.uid_stats = Some(output),
                        _ => {}
                    }
                    debug!(name = *name, "DeviceInspector: collected");
                }
                Ok(cr) => {
                    let msg = if cr.timed_out {
                        format!("{}: timed out", name)
                    } else {
                        format!("{}: exit={:?} stderr={}", name, cr.exit_code, cr.stderr.trim())
                    };
                    debug!("{}", msg);
                    result.errors.push(msg);
                }
                Err(e) => {
                    let msg = format!("{}: spawn error: {}", name, e);
                    debug!("{}", msg);
                    result.errors.push(msg);
                }
            }
        }

        result
    }

    /// Run a single ad-hoc command and return the result.
    pub async fn exec(&self, command: &str) -> anyhow::Result<CommandResult> {
        self.executor.exec(command).await
    }

    /// Collect a quick summary of network activity: number of TCP connections,
    /// listening ports, and established connections by UID.
    pub async fn quick_network_summary(&self) -> anyhow::Result<String> {
        let tcp_result = self.executor.exec(
            "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null"
        ).await?;

        if tcp_result.stdout.is_empty() {
            return Ok("No TCP connection data available (permission denied or no /proc/net support)".to_string());
        }

        let mut established = 0u32;
        let mut listen = 0u32;
        let mut uid_counts: HashMap<u32, u32> = HashMap::new();

        for line in tcp_result.stdout.lines().skip(1) {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 8 {
                continue;
            }
            let state = fields[3];
            match state {
                "01" => established += 1, // TCP_ESTABLISHED
                "0A" => listen += 1,      // TCP_LISTEN
                _ => {}
            }
            if let Ok(uid) = fields[7].parse::<u32>() {
                *uid_counts.entry(uid).or_default() += 1;
            }
        }

        let mut top_uids: Vec<(u32, u32)> = uid_counts.into_iter().collect();
        top_uids.sort_by(|a, b| b.1.cmp(&a.1));
        top_uids.truncate(10);

        let uid_summary: Vec<String> = top_uids
            .iter()
            .map(|(uid, count)| format!("UID {}={}", uid, count))
            .collect();

        Ok(format!(
            "TCP: {} established, {} listening. Top UIDs: {}",
            established,
            listen,
            if uid_summary.is_empty() {
                "none".to_string()
            } else {
                uid_summary.join(", ")
            }
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inspect_network_runs() {
        let inspector = DeviceInspector::new();
        let inspection = inspector.inspect_network().await;
        // On Linux dev machines, at least tcp_connections should be available
        assert!(
            inspection.tcp_connections.is_some()
                || !inspection.errors.is_empty(),
            "Expected either tcp data or error messages"
        );
    }

    #[tokio::test]
    async fn test_quick_network_summary() {
        let inspector = DeviceInspector::new();
        let summary = inspector.quick_network_summary().await.unwrap();
        assert!(!summary.is_empty());
    }
}
