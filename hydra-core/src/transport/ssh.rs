use super::{Transport, TransportStream};
use anyhow::{Context, Result};
use async_trait::async_trait;
use russh::client;
use russh::keys::key::PrivateKeyWithHashAlg;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

const SSH_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const SSH_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(600);
const CHANNEL_BUF_SIZE: usize = 32768;

/// SSH authentication method.
#[derive(Debug, Clone)]
pub enum SshAuth {
    /// Path to PEM/OpenSSH private key file.
    KeyFile(String),
    /// Password string.
    Password(String),
}

/// SSH transport: opens direct-tcpip channels through a persistent SSH session.
/// Equivalent to `ssh -L` (local port forwarding) but dynamic per-target.
pub struct SshTransport {
    host: String,
    port: u16,
    username: String,
    auth: SshAuth,
    session: Mutex<Option<client::Handle<SshClient>>>,
}

impl SshTransport {
    pub fn new(host: String, port: u16, username: String, auth: SshAuth) -> Self {
        info!(
            ssh_host = %host,
            ssh_port = port,
            ssh_user = %username,
            "SshTransport created"
        );
        Self {
            host,
            port,
            username,
            auth,
            session: Mutex::new(None),
        }
    }

    async fn ensure_connected(
        guard: &mut tokio::sync::MutexGuard<'_, Option<client::Handle<SshClient>>>,
        host: &str,
        port: u16,
        username: &str,
        auth: &SshAuth,
    ) -> Result<()> {
        let needs_connect = match guard.as_ref() {
            Some(h) if !h.is_closed() => false,
            Some(_) => {
                warn!("SSH session closed, reconnecting");
                true
            }
            None => true,
        };

        if needs_connect {
            let handle = Self::establish_session(host, port, username, auth).await?;
            **guard = Some(handle);
        }
        Ok(())
    }

    async fn establish_session(
        host: &str,
        port: u16,
        username: &str,
        auth: &SshAuth,
    ) -> Result<client::Handle<SshClient>> {
        let config = client::Config {
            inactivity_timeout: Some(SSH_INACTIVITY_TIMEOUT),
            ..<_>::default()
        };
        let config = Arc::new(config);

        let addr = format!("{}:{}", host, port);
        info!(addr = %addr, "SSH: connecting...");

        let sh = SshClient {};
        let mut session = tokio::time::timeout(
            SSH_CONNECT_TIMEOUT,
            client::connect(config, &*addr, sh),
        )
        .await
        .map_err(|_| anyhow::anyhow!(
            "SSH connect to {} timed out after {}s. Check host/port and network.",
            addr, SSH_CONNECT_TIMEOUT.as_secs()
        ))?
        .map_err(|e| anyhow::anyhow!("SSH connect to {} failed: {}. Verify server is running.", addr, e))?;

        match auth {
            SshAuth::KeyFile(path) => {
                let key_pair = russh::keys::load_secret_key(path, None)
                    .map_err(|e| anyhow::anyhow!(
                        "Failed to load SSH key '{}': {}. Check file exists and is valid PEM/OpenSSH format.",
                        path, e
                    ))?;
                let auth_res = session
                    .authenticate_publickey(
                        username,
                        PrivateKeyWithHashAlg::new(
                            Arc::new(key_pair),
                            session.best_supported_rsa_hash().await?.flatten(),
                        ),
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("SSH pubkey auth error: {}", e))?;

                if !auth_res.success() {
                    anyhow::bail!(
                        "SSH pubkey auth failed for user '{}' on {}. Check key is authorized on server.",
                        username, addr
                    );
                }
            }
            SshAuth::Password(password) => {
                let auth_res = session
                    .authenticate_password(username, password)
                    .await
                    .map_err(|e| anyhow::anyhow!("SSH password auth error: {}", e))?;

                if !auth_res.success() {
                    anyhow::bail!(
                        "SSH password auth failed for user '{}' on {}. Check credentials.",
                        username, addr
                    );
                }
            }
        }

        info!(addr = %addr, user = %username, "SSH: authenticated");
        Ok(session)
    }

    async fn open_channel(&self, target: &str) -> Result<TransportStream> {
        let (target_host, target_port) = parse_target(target)?;

        let mut guard = self.session.lock().await;
        Self::ensure_connected(&mut guard, &self.host, self.port, &self.username, &self.auth).await?;
        let session = guard.as_mut().expect("session must be Some after ensure_connected");

        let channel = session
            .channel_open_direct_tcpip(
                target_host.clone(),
                target_port.into(),
                "127.0.0.1".to_string(),
                0,
            )
            .await
            .map_err(|e| anyhow::anyhow!(
                "SSH direct-tcpip to {}:{} failed: {}. Server may not allow TCP forwarding.",
                target_host, target_port, e
            ))?;

        drop(guard);

        debug!(
            target_addr = %target,
            "SSH: direct-tcpip channel opened"
        );

        let (local_stream, bridge_stream) = create_tcp_pair().await
            .context("Failed to create local TCP bridge pair for SSH channel")?;

        let target_label = target.to_string();
        tokio::spawn(async move {
            if let Err(e) = bridge_ssh_channel(channel, bridge_stream, &target_label).await {
                debug!(target_addr = %target_label, error = %e, "SSH bridge task ended");
            }
        });

        Ok(Box::new(local_stream))
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
}

struct SshClient {}

impl client::Handler for SshClient {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> std::result::Result<bool, Self::Error> {
        // TODO: implement known_hosts checking
        Ok(true)
    }
}

async fn bridge_ssh_channel(
    mut channel: russh::Channel<russh::client::Msg>,
    bridge_stream: TcpStream,
    target: &str,
) -> Result<()> {
    let (mut bridge_read, mut bridge_write) = bridge_stream.into_split();
    let mut buf = vec![0u8; CHANNEL_BUF_SIZE];
    let mut bridge_eof = false;

    loop {
        tokio::select! {
            r = bridge_read.read(&mut buf), if !bridge_eof => {
                match r {
                    Ok(0) => {
                        bridge_eof = true;
                        let _ = channel.eof().await;
                    }
                    Ok(n) => {
                        if let Err(e) = channel.data(&buf[..n]).await {
                            debug!(target_addr = %target, "SSH channel write error: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        debug!(target_addr = %target, "Bridge read error: {}", e);
                        let _ = channel.eof().await;
                        break;
                    }
                }
            }
            msg = channel.wait() => {
                match msg {
                    Some(russh::ChannelMsg::Data { ref data }) => {
                        if bridge_write.write_all(data).await.is_err() {
                            debug!(target_addr = %target, "Bridge write error");
                            break;
                        }
                    }
                    Some(russh::ChannelMsg::Eof) => {
                        if !bridge_eof {
                            let _ = channel.eof().await;
                        }
                        break;
                    }
                    Some(russh::ChannelMsg::WindowAdjusted { .. }) => {}
                    Some(_) => {}
                    None => break,
                }
            }
        }
    }

    let _ = bridge_write.shutdown().await;
    debug!(target_addr = %target, "SSH bridge finished");
    Ok(())
}

fn parse_target(target: &str) -> Result<(String, u16)> {
    if target.starts_with('[') {
        let end_bracket = target.find(']')
            .ok_or_else(|| anyhow::anyhow!("Invalid IPv6 target: {}", target))?;
        let host = target[1..end_bracket].to_string();
        let port_str = target.get(end_bracket + 2..)
            .ok_or_else(|| anyhow::anyhow!("Missing port in target: {}", target))?;
        let port = port_str.parse::<u16>()
            .map_err(|_| anyhow::anyhow!("Invalid port in target: {}", target))?;
        Ok((host, port))
    } else {
        let mut parts = target.rsplitn(2, ':');
        let port_str = parts.next()
            .ok_or_else(|| anyhow::anyhow!("Missing port in target: {}", target))?;
        let host = parts.next()
            .ok_or_else(|| anyhow::anyhow!("Missing host in target: {}", target))?;
        let port = port_str.parse::<u16>()
            .map_err(|_| anyhow::anyhow!("Invalid port in target: {}", target))?;
        Ok((host.to_string(), port))
    }
}

async fn create_tcp_pair() -> Result<(TcpStream, TcpStream)> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let connect_fut = TcpStream::connect(addr);
    let accept_fut = listener.accept();
    let (client, (server, _)) = tokio::try_join!(connect_fut, accept_fut)?;
    Ok((client, server))
}
