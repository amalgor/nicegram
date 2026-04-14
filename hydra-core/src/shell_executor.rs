use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tracing::{debug, info, warn};

/// Maximum output size per command (128 KiB).
/// Larger output is truncated with a marker.
const MAX_OUTPUT_BYTES: usize = 128 * 1024;

/// Default timeout for a single command execution.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Commands that are never allowed regardless of context.
/// Prevents accidental damage to the device.
const BLOCKED_COMMANDS: &[&str] = &[
    "rm",
    "mkfs",
    "dd",
    "reboot",
    "shutdown",
    "poweroff",
    "halt",
    "init",
    "kill",
    "killall",
    "pkill",
    "su",
    "mount",
    "umount",
    "chown",
    "chmod",
    "iptables",
    "ip6tables",
    "ndc",
    "setenforce",
];

/// Result of a single command execution.
#[derive(Debug, Clone)]
pub struct CommandResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub timed_out: bool,
}

/// Executes shell commands on the local device.
///
/// Designed for on-device AI to inspect network state, running apps, etc.
/// All commands run inside the app sandbox with no root privileges.
pub struct ShellExecutor {
    shell_path: String,
    timeout: Duration,
}

impl ShellExecutor {
    pub fn new() -> Self {
        let shell_path = detect_shell();
        info!(shell = %shell_path, "ShellExecutor initialized");
        Self {
            shell_path,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Execute a command string. The command is passed to `sh -c "..."`.
    ///
    /// Returns an error only for infrastructure failures (spawn failed).
    /// Command failures (non-zero exit code) are reported in `CommandResult`.
    pub async fn exec(&self, command: &str) -> anyhow::Result<CommandResult> {
        let first_word = command.split_whitespace().next().unwrap_or("");
        if is_blocked(first_word) {
            return Ok(CommandResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!(
                    "[BLOCKED] Command '{}' is not allowed. \
                     Only read-only inspection commands are permitted.",
                    first_word
                ),
                truncated: false,
                timed_out: false,
            });
        }

        // Also check for piped / chained commands
        for segment in command.split(&['|', ';', '&'][..]) {
            let word = segment.trim().split_whitespace().next().unwrap_or("");
            if is_blocked(word) {
                return Ok(CommandResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!(
                        "[BLOCKED] Command '{}' in pipeline is not allowed. \
                         Only read-only inspection commands are permitted.",
                        word
                    ),
                    truncated: false,
                    timed_out: false,
                });
            }
        }

        debug!(command = %command, timeout_ms = %self.timeout.as_millis(), "ShellExecutor::exec");

        let mut child = Command::new(&self.shell_path)
            .arg("-c")
            .arg(command)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to spawn shell '{}': {}. \
                     Ensure the shell binary exists and is executable.",
                    self.shell_path,
                    e
                )
            })?;

        let result = tokio::time::timeout(self.timeout, async {
            let mut stdout_buf = Vec::with_capacity(4096);
            let mut stderr_buf = Vec::with_capacity(1024);
            let mut stdout_truncated = false;
            let mut stderr_truncated = false;

            if let Some(mut stdout) = child.stdout.take() {
                let mut buf = [0u8; 8192];
                loop {
                    match stdout.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let remaining = MAX_OUTPUT_BYTES.saturating_sub(stdout_buf.len());
                            if remaining == 0 {
                                stdout_truncated = true;
                                break;
                            }
                            let take = n.min(remaining);
                            stdout_buf.extend_from_slice(&buf[..take]);
                            if take < n {
                                stdout_truncated = true;
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }

            if let Some(mut stderr) = child.stderr.take() {
                let mut buf = [0u8; 4096];
                let max_stderr = MAX_OUTPUT_BYTES / 4;
                loop {
                    match stderr.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            let remaining = max_stderr.saturating_sub(stderr_buf.len());
                            if remaining == 0 {
                                stderr_truncated = true;
                                break;
                            }
                            let take = n.min(remaining);
                            stderr_buf.extend_from_slice(&buf[..take]);
                            if take < n {
                                stderr_truncated = true;
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }

            let status = child.wait().await.ok();

            let mut stdout = String::from_utf8_lossy(&stdout_buf).into_owned();
            let mut stderr = String::from_utf8_lossy(&stderr_buf).into_owned();

            if stdout_truncated {
                stdout.push_str("\n[TRUNCATED] Output exceeded 128 KiB limit.");
            }
            if stderr_truncated {
                stderr.push_str("\n[TRUNCATED] Stderr exceeded 32 KiB limit.");
            }

            CommandResult {
                exit_code: status.and_then(|s| s.code()),
                stdout,
                stderr,
                truncated: stdout_truncated || stderr_truncated,
                timed_out: false,
            }
        })
        .await;

        match result {
            Ok(cr) => {
                debug!(
                    exit_code = ?cr.exit_code,
                    stdout_len = cr.stdout.len(),
                    stderr_len = cr.stderr.len(),
                    truncated = cr.truncated,
                    "ShellExecutor::exec completed"
                );
                Ok(cr)
            }
            Err(_) => {
                warn!(command = %command, "ShellExecutor::exec timed out");
                Ok(CommandResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!(
                        "[TIMEOUT] Command did not complete within {} seconds.",
                        self.timeout.as_secs()
                    ),
                    truncated: false,
                    timed_out: true,
                })
            }
        }
    }

    /// Execute a sequence of commands, returning results for each.
    pub async fn exec_batch(&self, commands: &[&str]) -> Vec<anyhow::Result<CommandResult>> {
        let mut results = Vec::with_capacity(commands.len());
        for cmd in commands {
            results.push(self.exec(cmd).await);
        }
        results
    }
}

fn is_blocked(word: &str) -> bool {
    let base = word.rsplit('/').next().unwrap_or(word);
    BLOCKED_COMMANDS.iter().any(|&b| base == b)
}

fn detect_shell() -> String {
    for path in &["/system/bin/sh", "/bin/sh", "/usr/bin/sh"] {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }
    "sh".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_exec_echo() {
        let executor = ShellExecutor::new();
        let result = executor.exec("echo hello").await.unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert_eq!(result.stdout.trim(), "hello");
        assert!(result.stderr.is_empty() || result.stderr.trim().is_empty());
        assert!(!result.truncated);
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn test_exec_blocked_command() {
        let executor = ShellExecutor::new();
        let result = executor.exec("rm -rf /").await.unwrap();
        assert!(result.stderr.contains("[BLOCKED]"));
        assert_eq!(result.exit_code, None);
    }

    #[tokio::test]
    async fn test_exec_blocked_in_pipe() {
        let executor = ShellExecutor::new();
        let result = executor.exec("echo test | rm something").await.unwrap();
        assert!(result.stderr.contains("[BLOCKED]"));
    }

    #[tokio::test]
    async fn test_exec_blocked_with_path() {
        let executor = ShellExecutor::new();
        let result = executor.exec("/bin/rm -rf /").await.unwrap();
        assert!(result.stderr.contains("[BLOCKED]"));
    }

    #[tokio::test]
    async fn test_exec_nonexistent_command() {
        let executor = ShellExecutor::new();
        let result = executor.exec("nonexistent_command_xyz").await.unwrap();
        assert_ne!(result.exit_code, Some(0));
    }

    #[tokio::test]
    async fn test_exec_timeout() {
        let executor = ShellExecutor::new().with_timeout(Duration::from_millis(200));
        let result = executor.exec("sleep 10").await.unwrap();
        assert!(result.timed_out);
        assert!(result.stderr.contains("[TIMEOUT]"));
    }

    #[tokio::test]
    async fn test_exec_stderr() {
        let executor = ShellExecutor::new();
        let result = executor.exec("echo error >&2").await.unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(result.stderr.trim().contains("error"));
    }

    #[tokio::test]
    async fn test_exec_exit_code() {
        let executor = ShellExecutor::new();
        let result = executor.exec("exit 42").await.unwrap();
        assert_eq!(result.exit_code, Some(42));
    }

    #[tokio::test]
    async fn test_exec_batch() {
        let executor = ShellExecutor::new();
        let results = executor.exec_batch(&["echo one", "echo two"]).await;
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].as_ref().unwrap().stdout.trim(), "one");
        assert_eq!(results[1].as_ref().unwrap().stdout.trim(), "two");
    }

    #[tokio::test]
    async fn test_exec_proc_net() {
        let executor = ShellExecutor::new();
        let result = executor.exec("cat /proc/net/tcp 2>/dev/null || echo no-procfs").await.unwrap();
        assert_eq!(result.exit_code, Some(0));
        assert!(!result.stdout.is_empty());
    }

    #[test]
    fn test_is_blocked() {
        assert!(is_blocked("rm"));
        assert!(is_blocked("/bin/rm"));
        assert!(is_blocked("/system/bin/rm"));
        assert!(!is_blocked("ls"));
        assert!(!is_blocked("cat"));
        assert!(!is_blocked("grep"));
        assert!(!is_blocked("ps"));
    }

    #[test]
    fn test_detect_shell() {
        let shell = detect_shell();
        assert!(!shell.is_empty());
    }
}
