use super::{Transport, TransportStream};
use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::client;
use russh::keys::key::PrivateKeyWithHashAlg;
use russh::keys::{HashAlg, PrivateKey};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex as StdMutex, Weak};
use std::task::{Context as TaskContext, Poll};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

const DNS_TIMEOUT: Duration = Duration::from_secs(10);
const TCP_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SSH_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(20);
const SSH_AUTH_TIMEOUT: Duration = Duration::from_secs(20);
const CHANNEL_OPEN_TIMEOUT: Duration = Duration::from_secs(15);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
const KEEPALIVE_MAX: usize = 3;
/// After a failed connect, requests fail fast with the same error for this
/// long instead of each one starting its own connect attempt.
const FAILURE_HOLD_DOWN: Duration = Duration::from_secs(3);

/// SSH authentication method.
#[derive(Debug, Clone)]
pub enum SshAuth {
    /// Path to PEM/OpenSSH private key file.
    KeyFile(String),
    /// Inline PEM/OpenSSH private key content.
    KeyPem(String),
    /// Password string.
    Password(String),
}

#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SshState {
    #[default]
    Idle,
    Connecting,
    Connected,
    Failed,
}

/// Point-in-time view of one SSH transport, for UIs and diagnostics.
#[derive(Debug, Clone, Serialize, Default)]
pub struct SshStatus {
    pub endpoint: String,
    pub state: SshState,
    pub last_error: Option<String>,
    pub host_key_fingerprint: Option<String>,
    pub host_key_algorithm: Option<String>,
    pub connected_since_ms: Option<u64>,
    pub last_change_ms: u64,
    pub connect_attempts: u64,
    pub channels_open: u64,
    pub channels_total: u64,
    pub channels_failed: u64,
    pub bytes_up: u64,
    pub bytes_down: u64,
}

struct Shared {
    status: StdMutex<SshStatus>,
    /// Set while we tear the session down ourselves, so the resulting
    /// disconnect is not reported as a failure.
    closing: std::sync::atomic::AtomicBool,
    channels_open: AtomicU64,
    channels_total: AtomicU64,
    channels_failed: AtomicU64,
    bytes_up: AtomicU64,
    bytes_down: AtomicU64,
}

impl Shared {
    fn new(endpoint: String) -> Arc<Self> {
        let shared = Arc::new(Self {
            status: StdMutex::new(SshStatus {
                endpoint,
                last_change_ms: now_ms(),
                ..Default::default()
            }),
            closing: std::sync::atomic::AtomicBool::new(false),
            channels_open: AtomicU64::new(0),
            channels_total: AtomicU64::new(0),
            channels_failed: AtomicU64::new(0),
            bytes_up: AtomicU64::new(0),
            bytes_down: AtomicU64::new(0),
        });
        let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
        registry.retain(|weak| weak.strong_count() > 0);
        registry.push(Arc::downgrade(&shared));
        shared
    }

    fn with_status(&self, f: impl FnOnce(&mut SshStatus)) {
        let mut status = self.status.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut status);
    }

    fn set_state(&self, state: SshState, error: Option<String>) {
        self.with_status(|status| {
            status.state = state;
            if error.is_some() || state == SshState::Connected {
                status.last_error = error;
            }
            status.connected_since_ms = match state {
                SshState::Connected => Some(now_ms()),
                _ => None,
            };
            status.last_change_ms = now_ms();
        });
    }

    fn snapshot(&self) -> SshStatus {
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        status.channels_open = self.channels_open.load(Ordering::Relaxed);
        status.channels_total = self.channels_total.load(Ordering::Relaxed);
        status.channels_failed = self.channels_failed.load(Ordering::Relaxed);
        status.bytes_up = self.bytes_up.load(Ordering::Relaxed);
        status.bytes_down = self.bytes_down.load(Ordering::Relaxed);
        status
    }
}

static REGISTRY: LazyLock<StdMutex<Vec<Weak<Shared>>>> = LazyLock::new(|| StdMutex::new(Vec::new()));

/// Status of every live SSH transport (dropped transports disappear).
pub fn status_snapshot() -> Vec<SshStatus> {
    let mut registry = REGISTRY.lock().unwrap_or_else(|e| e.into_inner());
    registry.retain(|weak| weak.strong_count() > 0);
    registry
        .iter()
        .filter_map(Weak::upgrade)
        .map(|shared| shared.snapshot())
        .collect()
}

// ---------------------------------------------------------------------------
// Trust-on-first-use host key pinning
// ---------------------------------------------------------------------------

static KNOWN_HOSTS_PATH: LazyLock<StdMutex<Option<PathBuf>>> = LazyLock::new(|| StdMutex::new(None));

/// JSON file (`{"host:port": "SHA256:..."}`) used to pin server host keys.
/// Without it, host keys are accepted unchecked (and logged).
pub fn set_known_hosts_path(path: PathBuf) {
    *KNOWN_HOSTS_PATH.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
}

fn known_hosts_path() -> Option<PathBuf> {
    KNOWN_HOSTS_PATH
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

fn load_known_hosts() -> HashMap<String, String> {
    let Some(path) = known_hosts_path() else {
        return HashMap::new();
    };
    std::fs::read(&path)
        .ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok())
        .unwrap_or_default()
}

fn store_known_hosts(hosts: &HashMap<String, String>) -> Result<()> {
    let Some(path) = known_hosts_path() else {
        return Ok(());
    };
    std::fs::write(&path, serde_json::to_vec_pretty(hosts)?)
        .with_context(|| format!("writing {}", path.display()))
}

pub fn known_host_fingerprint(host: &str, port: u16) -> Option<String> {
    load_known_hosts().remove(&host_key_id(host, port))
}

pub fn forget_known_host(host: &str, port: u16) -> Result<()> {
    let mut hosts = load_known_hosts();
    if hosts.remove(&host_key_id(host, port)).is_some() {
        info!(ssh_event = "host_key_forgotten", ssh_addr = %host_key_id(host, port), "SSH: trusted host key removed");
    }
    store_known_hosts(&hosts)
}

fn host_key_id(host: &str, port: u16) -> String {
    format!("{}:{}", host.trim().to_ascii_lowercase(), port)
}

// ---------------------------------------------------------------------------
// Key generation / inspection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct SshKeyInfo {
    pub private_openssh: String,
    pub public_openssh: String,
    pub fingerprint: String,
    pub algorithm: String,
}

pub fn generate_ed25519_key(comment: &str) -> Result<SshKeyInfo> {
    let mut seed = [0u8; 32];
    getrandom::getrandom(&mut seed).map_err(|e| anyhow::anyhow!("system RNG failed: {e}"))?;
    let keypair = russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&seed);
    seed.fill(0);
    let mut key = PrivateKey::from(keypair);
    key.set_comment(comment);
    describe_key(&key)
}

/// Public half and fingerprint of an OpenSSH/PEM private key.
pub fn describe_private_key(pem: &str) -> Result<SshKeyInfo> {
    let key = russh::keys::decode_secret_key(pem.trim(), None)
        .map_err(|e| anyhow::anyhow!("Not a valid unencrypted OpenSSH/PEM private key: {e}"))?;
    describe_key(&key)
}

fn describe_key(key: &PrivateKey) -> Result<SshKeyInfo> {
    let public = key.public_key();
    Ok(SshKeyInfo {
        private_openssh: key
            .to_openssh(russh::keys::ssh_key::LineEnding::LF)
            .map_err(|e| anyhow::anyhow!("encoding private key: {e}"))?
            .to_string(),
        public_openssh: public
            .to_openssh()
            .map_err(|e| anyhow::anyhow!("encoding public key: {e}"))?,
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        algorithm: public.algorithm().as_str().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Transport
// ---------------------------------------------------------------------------

type SessionHandle = Arc<client::Handle<SshClient>>;

/// SSH transport: opens direct-tcpip channels through one persistent SSH
/// session, i.e. what `ssh -D` does for each SOCKS request.
pub struct SshTransport {
    host: String,
    port: u16,
    username: String,
    auth: SshAuth,
    endpoint: String,
    session: Mutex<Option<SessionHandle>>,
    last_failure: StdMutex<Option<(Instant, String)>>,
    shared: Arc<Shared>,
}

impl SshTransport {
    pub fn new(host: String, port: u16, username: String, auth: SshAuth) -> Self {
        let endpoint = format!("{}@{}:{}", username, host, port);
        info!(
            ssh_event = "created",
            ssh_host = %host,
            ssh_port = port,
            ssh_user = %username,
            auth = auth_kind(&auth),
            "SSH transport profile created"
        );
        Self {
            shared: Shared::new(endpoint.clone()),
            host,
            port,
            username,
            auth,
            endpoint,
            session: Mutex::new(None),
            last_failure: StdMutex::new(None),
        }
    }

    async fn session_handle(&self) -> Result<SessionHandle> {
        let mut guard = self.session.lock().await;
        if let Some(handle) = guard.as_ref() {
            if !handle.is_closed() {
                return Ok(handle.clone());
            }
            warn!(ssh_event = "reconnect", ssh_addr = %self.endpoint, "SSH session closed, reconnecting");
            *guard = None;
        }

        if let Some((at, error)) = self
            .last_failure
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            if at.elapsed() < FAILURE_HOLD_DOWN {
                anyhow::bail!("{error}");
            }
        }

        match self.establish_session().await {
            Ok(handle) => {
                let handle = Arc::new(handle);
                *guard = Some(handle.clone());
                *self.last_failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
                Ok(handle)
            }
            Err(e) => {
                let message = format!("{e:#}");
                error!(ssh_event = "session_failed", ssh_addr = %self.endpoint, error = %message, "SSH session could not be established");
                *self.last_failure.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some((Instant::now(), message.clone()));
                self.shared.set_state(SshState::Failed, Some(message));
                Err(e)
            }
        }
    }

    async fn establish_session(&self) -> Result<client::Handle<SshClient>> {
        let started = Instant::now();
        self.shared.closing.store(false, Ordering::SeqCst);
        self.shared.set_state(SshState::Connecting, None);
        self.shared.with_status(|s| s.connect_attempts += 1);
        let addr_label = format!("{}:{}", self.host, self.port);
        info!(ssh_event = "connecting", ssh_addr = %addr_label, ssh_user = %self.username, "SSH: connecting to server");

        let addrs: Vec<std::net::SocketAddr> =
            tokio::time::timeout(DNS_TIMEOUT, tokio::net::lookup_host((self.host.as_str(), self.port)))
                .await
                .map_err(|_| anyhow::anyhow!("DNS lookup for {} timed out after {}s", self.host, DNS_TIMEOUT.as_secs()))?
                .map_err(|e| anyhow::anyhow!("DNS lookup for {} failed: {}", self.host, e))?
                .collect();
        info!(ssh_event = "resolved", ssh_addr = %addr_label, addrs = ?addrs, "SSH: server address resolved");

        let mut stream = None;
        let mut tcp_errors = Vec::new();
        for addr in &addrs {
            match tokio::time::timeout(TCP_CONNECT_TIMEOUT, TcpStream::connect(addr)).await {
                Ok(Ok(s)) => {
                    stream = Some(s);
                    break;
                }
                Ok(Err(e)) => {
                    warn!(ssh_event = "tcp_connect_failed", ssh_addr = %addr, error = %e, "SSH: TCP connect failed");
                    tcp_errors.push(format!("{addr}: {e}"));
                }
                Err(_) => {
                    warn!(ssh_event = "connect_timeout", ssh_addr = %addr, timeout_s = TCP_CONNECT_TIMEOUT.as_secs(), "SSH: TCP connect timed out");
                    tcp_errors.push(format!("{addr}: timed out after {}s", TCP_CONNECT_TIMEOUT.as_secs()));
                }
            }
        }
        let stream = stream.ok_or_else(|| {
            anyhow::anyhow!(
                "Cannot reach SSH server {}: {}",
                addr_label,
                if tcp_errors.is_empty() { "no addresses".to_string() } else { tcp_errors.join("; ") }
            )
        })?;
        let _ = stream.set_nodelay(true);
        info!(
            ssh_event = "tcp_connected",
            ssh_addr = %addr_label,
            peer = ?stream.peer_addr().ok(),
            local = ?stream.local_addr().ok(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "SSH: TCP connected, starting handshake"
        );

        let config = Arc::new(client::Config {
            inactivity_timeout: None,
            keepalive_interval: Some(KEEPALIVE_INTERVAL),
            keepalive_max: KEEPALIVE_MAX,
            nodelay: true,
            ..<_>::default()
        });
        let verdict = Arc::new(StdMutex::new(None));
        let handler = SshClient {
            key_id: host_key_id(&self.host, self.port),
            endpoint: self.endpoint.clone(),
            shared: self.shared.clone(),
            verdict: verdict.clone(),
        };

        let handshake =
            tokio::time::timeout(SSH_HANDSHAKE_TIMEOUT, client::connect_stream(config, stream, handler)).await;
        let mut session = match handshake {
            Err(_) => anyhow::bail!(
                "SSH handshake with {} timed out after {}s (is this really an SSH server?)",
                addr_label,
                SSH_HANDSHAKE_TIMEOUT.as_secs()
            ),
            Ok(Err(e)) => {
                if let Some(HostKeyVerdict::Mismatch { expected, actual }) =
                    verdict.lock().unwrap_or_else(|e| e.into_inner()).clone()
                {
                    anyhow::bail!(
                        "HOST KEY CHANGED for {addr_label}: trusted {expected}, server now presents {actual}. \
                         If the server was reinstalled, reset the trusted host key in the app."
                    );
                }
                anyhow::bail!("SSH handshake with {} failed: {}", addr_label, e)
            }
            Ok(Ok(session)) => session,
        };
        info!(
            ssh_event = "connected",
            ssh_addr = %addr_label,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "SSH: handshake complete, authenticating"
        );

        tokio::time::timeout(SSH_AUTH_TIMEOUT, self.authenticate(&mut session, &addr_label))
            .await
            .map_err(|_| anyhow::anyhow!("SSH authentication on {} timed out after {}s", addr_label, SSH_AUTH_TIMEOUT.as_secs()))??;

        self.shared.set_state(SshState::Connected, None);
        info!(
            ssh_event = "authenticated",
            ssh_addr = %addr_label,
            ssh_user = %self.username,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "SSH: authenticated, session ready"
        );
        Ok(session)
    }

    async fn authenticate(&self, session: &mut client::Handle<SshClient>, addr: &str) -> Result<()> {
        let username = self.username.as_str();
        let (method, result) = match &self.auth {
            SshAuth::KeyFile(path) => {
                let key = russh::keys::load_secret_key(path, None).map_err(|e| {
                    anyhow::anyhow!("Failed to load SSH key '{}': {}. Check file exists and is valid PEM/OpenSSH format.", path, e)
                })?;
                ("pubkey", self.authenticate_pubkey(session, key).await)
            }
            SshAuth::KeyPem(pem) => {
                let key = russh::keys::decode_secret_key(pem.trim(), None).map_err(|e| {
                    anyhow::anyhow!("Failed to decode private key: {}. Paste an unencrypted OpenSSH/PEM key.", e)
                })?;
                ("pubkey", self.authenticate_pubkey(session, key).await)
            }
            SshAuth::Password(password) => (
                "password",
                session
                    .authenticate_password(username, password)
                    .await
                    .map_err(anyhow::Error::from),
            ),
        };

        match result {
            Ok(russh::client::AuthResult::Success) => Ok(()),
            Ok(russh::client::AuthResult::Failure { remaining_methods, partial_success }) => {
                warn!(
                    ssh_event = "auth_failed",
                    ssh_addr = %addr,
                    method,
                    ssh_user = %username,
                    remaining_methods = ?remaining_methods,
                    partial_success,
                    "SSH authentication rejected"
                );
                let hint = if method == "pubkey" {
                    "Check that the public key is in ~/.ssh/authorized_keys on the server."
                } else {
                    "Check the password, and that the server allows PasswordAuthentication."
                };
                anyhow::bail!(
                    "SSH {method} authentication rejected for '{username}' on {addr} (server accepts: {remaining_methods:?}). {hint}"
                )
            }
            Err(e) => {
                warn!(ssh_event = "auth_error", ssh_addr = %addr, method, error = %e, "SSH authentication error");
                anyhow::bail!("SSH {method} authentication error on {addr}: {e}")
            }
        }
    }

    async fn authenticate_pubkey(
        &self,
        session: &mut client::Handle<SshClient>,
        key: PrivateKey,
    ) -> Result<russh::client::AuthResult> {
        debug!(
            ssh_event = "auth_pubkey",
            key_algorithm = key.algorithm().as_str(),
            key_fingerprint = %key.public_key().fingerprint(HashAlg::Sha256),
            "SSH: offering public key"
        );
        let hash = session.best_supported_rsa_hash().await?.flatten();
        Ok(session
            .authenticate_publickey(&self.username, PrivateKeyWithHashAlg::new(Arc::new(key), hash))
            .await?)
    }

    async fn drop_session(&self, stale: &SessionHandle) {
        let mut guard = self.session.lock().await;
        if guard.as_ref().is_some_and(|current| Arc::ptr_eq(current, stale)) {
            *guard = None;
        }
    }

    async fn open_channel(&self, target: &str) -> Result<TransportStream> {
        let (target_host, target_port) = parse_target(target)?;

        for attempt in 0..2 {
            let handle = self.session_handle().await?;
            let started = Instant::now();
            let opened = tokio::time::timeout(
                CHANNEL_OPEN_TIMEOUT,
                handle.channel_open_direct_tcpip(target_host.clone(), target_port.into(), "127.0.0.1", 0),
            )
            .await;

            match opened {
                Ok(Ok(channel)) => {
                    self.shared.channels_open.fetch_add(1, Ordering::Relaxed);
                    self.shared.channels_total.fetch_add(1, Ordering::Relaxed);
                    debug!(
                        ssh_event = "channel_open",
                        target_addr = %target,
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "SSH: direct-tcpip channel opened"
                    );
                    return Ok(Box::new(TrackedStream {
                        inner: channel.into_stream(),
                        shared: self.shared.clone(),
                        target: target.to_string(),
                        opened_at: Instant::now(),
                        up: 0,
                        down: 0,
                    }));
                }
                Ok(Err(e)) if attempt == 0 && handle.is_closed() => {
                    warn!(ssh_event = "session_lost", target_addr = %target, error = %e, "SSH session dropped while opening a channel, reconnecting");
                    self.drop_session(&handle).await;
                }
                Ok(Err(e)) => {
                    self.shared.channels_failed.fetch_add(1, Ordering::Relaxed);
                    warn!(ssh_event = "channel_failed", target_addr = %target, error = %e, "SSH direct-tcpip channel failed");
                    anyhow::bail!(
                        "SSH server refused to open {}:{}: {}. The target may be unreachable from the server, or AllowTcpForwarding is off.",
                        target_host, target_port, e
                    );
                }
                Err(_) => {
                    self.shared.channels_failed.fetch_add(1, Ordering::Relaxed);
                    warn!(ssh_event = "channel_timeout", target_addr = %target, timeout_s = CHANNEL_OPEN_TIMEOUT.as_secs(), "SSH direct-tcpip channel open timed out");
                    if attempt == 0 && handle.is_closed() {
                        self.drop_session(&handle).await;
                        continue;
                    }
                    anyhow::bail!(
                        "SSH channel to {}:{} timed out after {}s",
                        target_host, target_port, CHANNEL_OPEN_TIMEOUT.as_secs()
                    );
                }
            }
        }
        anyhow::bail!("SSH session to {} lost twice while opening a channel", self.endpoint)
    }
}

#[async_trait]
impl Transport for SshTransport {
    async fn connect(&self, target: &str) -> Result<TransportStream> {
        self.open_channel(target).await
    }

    fn name(&self) -> &str {
        "ssh"
    }

    fn supports_udp(&self) -> bool {
        false
    }

    async fn warm_up(&self) -> Result<()> {
        self.session_handle().await.map(|_| ())
    }

    async fn reset(&self) {
        let handle = self.session.lock().await.take();
        *self.last_failure.lock().unwrap_or_else(|e| e.into_inner()) = None;
        if let Some(handle) = handle {
            info!(ssh_event = "reset", ssh_addr = %self.endpoint, "SSH: dropping session on request");
            self.shared.closing.store(true, Ordering::SeqCst);
            let _ = handle
                .disconnect(russh::Disconnect::ByApplication, "reconnecting", "en")
                .await;
        }
        self.shared.set_state(SshState::Idle, None);
    }
}

#[derive(Debug, Clone)]
enum HostKeyVerdict {
    Mismatch { expected: String, actual: String },
}

struct SshClient {
    key_id: String,
    endpoint: String,
    shared: Arc<Shared>,
    verdict: Arc<StdMutex<Option<HostKeyVerdict>>>,
}

impl client::Handler for SshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        let fingerprint = server_public_key.fingerprint(HashAlg::Sha256).to_string();
        let algorithm = server_public_key.algorithm().as_str().to_string();
        self.shared.with_status(|s| {
            s.host_key_fingerprint = Some(fingerprint.clone());
            s.host_key_algorithm = Some(algorithm.clone());
        });

        if known_hosts_path().is_none() {
            warn!(ssh_event = "host_key_unchecked", ssh_addr = %self.key_id, fingerprint = %fingerprint, algorithm = %algorithm, "SSH: no known-hosts store configured, accepting host key");
            return Ok(true);
        }

        let mut hosts = load_known_hosts();
        match hosts.get(&self.key_id) {
            Some(expected) if *expected == fingerprint => {
                debug!(ssh_event = "host_key_ok", ssh_addr = %self.key_id, fingerprint = %fingerprint, "SSH: host key matches pinned key");
                Ok(true)
            }
            Some(expected) => {
                error!(ssh_event = "host_key_mismatch", ssh_addr = %self.key_id, expected = %expected, actual = %fingerprint, "SSH: HOST KEY CHANGED, refusing to connect");
                *self.verdict.lock().unwrap_or_else(|e| e.into_inner()) = Some(HostKeyVerdict::Mismatch {
                    expected: expected.clone(),
                    actual: fingerprint,
                });
                Ok(false)
            }
            None => {
                warn!(ssh_event = "host_key_pinned", ssh_addr = %self.key_id, fingerprint = %fingerprint, algorithm = %algorithm, "SSH: first connection to this server, trusting and pinning host key");
                hosts.insert(self.key_id.clone(), fingerprint);
                if let Err(e) = store_known_hosts(&hosts) {
                    warn!(error = %e, "SSH: failed to persist pinned host key");
                }
                Ok(true)
            }
        }
    }

    async fn disconnected(
        &mut self,
        reason: client::DisconnectReason<Self::Error>,
    ) -> std::result::Result<(), Self::Error> {
        if self.shared.closing.load(Ordering::SeqCst) {
            info!(ssh_event = "closed", ssh_addr = %self.endpoint, "SSH: session closed");
            return Ok(());
        }
        match reason {
            client::DisconnectReason::ReceivedDisconnect(info) => {
                warn!(ssh_event = "disconnected", ssh_addr = %self.endpoint, reason_code = ?info.reason_code, message = %info.message, "SSH: server closed the session");
                self.shared.set_state(
                    SshState::Idle,
                    Some(format!("Server closed the session: {:?} {}", info.reason_code, info.message)),
                );
                Ok(())
            }
            client::DisconnectReason::Error(e) => {
                warn!(ssh_event = "disconnected", ssh_addr = %self.endpoint, error = %e, "SSH: session ended with error");
                self.shared.set_state(SshState::Failed, Some(format!("Session lost: {e}")));
                Err(e)
            }
        }
    }
}

/// Channel stream that keeps the per-transport counters up to date.
struct TrackedStream<S> {
    inner: S,
    shared: Arc<Shared>,
    target: String,
    opened_at: Instant,
    up: u64,
    down: u64,
}

impl<S: AsyncRead + Unpin> AsyncRead for TrackedStream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        if let Poll::Ready(Ok(())) = &result {
            let n = (buf.filled().len() - before) as u64;
            self.down += n;
            self.shared.bytes_down.fetch_add(n, Ordering::Relaxed);
        }
        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for TrackedStream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let result = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &result {
            self.up += *n as u64;
            self.shared.bytes_up.fetch_add(*n as u64, Ordering::Relaxed);
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

impl<S> Drop for TrackedStream<S> {
    fn drop(&mut self) {
        self.shared.channels_open.fetch_sub(1, Ordering::Relaxed);
        debug!(
            ssh_event = "channel_closed",
            target_addr = %self.target,
            up = self.up,
            down = self.down,
            duration_ms = self.opened_at.elapsed().as_millis() as u64,
            "SSH: channel closed"
        );
    }
}

fn auth_kind(auth: &SshAuth) -> &'static str {
    match auth {
        SshAuth::KeyFile(_) => "key_file",
        SshAuth::KeyPem(_) => "key",
        SshAuth::Password(_) => "password",
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn parse_target(target: &str) -> Result<(String, u16)> {
    if target.starts_with('[') {
        let end_bracket = target
            .find(']')
            .ok_or_else(|| anyhow::anyhow!("Invalid IPv6 target: {}", target))?;
        let host = target[1..end_bracket].to_string();
        let port_str = target
            .get(end_bracket + 2..)
            .ok_or_else(|| anyhow::anyhow!("Missing port in target: {}", target))?;
        let port = port_str
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("Invalid port in target: {}", target))?;
        Ok((host, port))
    } else {
        let mut parts = target.rsplitn(2, ':');
        let port_str = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing port in target: {}", target))?;
        let host = parts
            .next()
            .ok_or_else(|| anyhow::anyhow!("Missing host in target: {}", target))?;
        let port = port_str
            .parse::<u16>()
            .map_err(|_| anyhow::anyhow!("Invalid port in target: {}", target))?;
        Ok((host.to_string(), port))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_handles_ipv4_ipv6_and_names() {
        assert_eq!(parse_target("1.2.3.4:443").unwrap(), ("1.2.3.4".into(), 443));
        assert_eq!(parse_target("[2001:db8::1]:80").unwrap(), ("2001:db8::1".into(), 80));
        assert_eq!(parse_target("example.com:22").unwrap(), ("example.com".into(), 22));
        assert!(parse_target("example.com").is_err());
    }

    #[test]
    fn generated_key_round_trips() {
        let key = generate_ed25519_key("hydra-test").unwrap();
        assert!(key.public_openssh.starts_with("ssh-ed25519 "));
        let described = describe_private_key(&key.private_openssh).unwrap();
        assert_eq!(described.fingerprint, key.fingerprint);
        assert!(describe_private_key("not a key").is_err());
    }
}
