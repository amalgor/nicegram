use flutter_rust_bridge::frb;
use tun2proxy::{Args, ArgProxy, ArgDns, ArgVerbosity, CancellationToken, general_run_async};
use lazy_static::lazy_static;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU16, Ordering};

/// Stores the actual SOCKS5 port loaded from config at node startup.
/// Set by start_hydra_node(), read by start_vpn_tunnel().
pub static SOCKS5_PORT: AtomicU16 = AtomicU16::new(1080);

lazy_static! {
    static ref VPN_CANCEL_TOKEN: Arc<Mutex<Option<CancellationToken>>> = Arc::new(Mutex::new(None));
}

#[frb(sync)]
pub fn start_vpn_tunnel(fd: i32) -> anyhow::Result<()> {
    tracing::info!("Received VPN interface FD: {}", fd);
    tracing::info!("Initializing tun2proxy with SOCKS5 bridge...");
    
    let port = SOCKS5_PORT.load(Ordering::Relaxed);
    let proxy_addr = format!("socks5://127.0.0.1:{}", port);
    tracing::info!("tun2proxy will connect to SOCKS5 at 127.0.0.1:{}", port);

    let mut args = Args::default();
    args.proxy = ArgProxy::try_from(proxy_addr.as_str()).map_err(|e| anyhow::anyhow!("{}", e))?;
    args.tun_fd = Some(fd);
    args.close_fd_on_drop = Some(true);
    args.dns = ArgDns::Virtual;
    args.verbosity = ArgVerbosity::Info;

    let shutdown_token = CancellationToken::new();
    let token_clone = shutdown_token.clone();

    {
        let mut guard = VPN_CANCEL_TOKEN.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
        *guard = Some(token_clone);
    }

    // tun2proxy needs its own dedicated runtime (long-running TUN event loop)
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tun2proxy runtime");
        rt.block_on(async {
            tracing::info!("tun2proxy started. Intercepting all device traffic.");
            match general_run_async(args, 1500, false, shutdown_token).await {
                Ok(_) => tracing::info!("tun2proxy stopped successfully."),
                Err(e) => tracing::error!("tun2proxy error: {}", e),
            }
        });
    });

    Ok(())
}

#[frb(sync)]
pub fn stop_vpn_tunnel() -> anyhow::Result<()> {
    tracing::info!("Stopping VPN tunnel...");
    
    let mut guard = VPN_CANCEL_TOKEN.lock().map_err(|e| anyhow::anyhow!("Lock poisoned: {}", e))?;
    if let Some(token) = guard.take() {
        token.cancel();
        tracing::info!("Sent cancellation signal to tun2proxy.");
    } else {
        tracing::warn!("No active VPN tunnel found to stop.");
    }

    Ok(())
}
