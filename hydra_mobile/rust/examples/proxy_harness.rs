//! Host-side harness: runs the same Rust API the Flutter app calls, on the Mac.
//! See `hydra_mobile/tool/e2e_local_ssh.sh` for the automated end-to-end check.
//!
//! cargo run -p rust_lib_hydra_mobile --example proxy_harness -- \
//!     --base-dir /tmp/hydra-harness --ssh user@127.0.0.1:2222 --key /path/to/id_ed25519

use rust_lib_hydra_mobile::api::{routes, simple};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1).cloned())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let base_dir = arg(&args, "--base-dir").unwrap_or_else(|| "/tmp/hydra-harness".into());
    std::fs::create_dir_all(&base_dir)?;

    let config = std::path::Path::new(&base_dir).join("hydra.toml");
    if !config.exists() {
        let bundled = concat!(env!("CARGO_MANIFEST_DIR"), "/../assets/hydra.toml");
        std::fs::copy(bundled, &config)?;
    }

    std::env::set_var("HYDRA_LOG_STDERR", "1");
    simple::init_app(Some(format!("{base_dir}/logs")));
    simple::prepare_local_runtime(base_dir.clone()).await?;

    if let Some(ssh) = arg(&args, "--ssh") {
        let (user, host_port) = ssh.split_once('@').expect("--ssh user@host:port");
        let (host, port) = host_port.rsplit_once(':').unwrap_or((host_port, "22"));
        let (auth, credential) = match (arg(&args, "--key"), arg(&args, "--password")) {
            (Some(key), _) => ("key", std::fs::read_to_string(key)?),
            (None, Some(pw)) => ("password", pw),
            _ => panic!("--key or --password required with --ssh"),
        };
        let test = routes::test_ssh_server(
            None,
            host.into(),
            port.parse()?,
            user.into(),
            auth.into(),
            Some(credential.clone()),
        )
        .await?;
        eprintln!("harness: test result {test}");
        let saved = routes::save_ssh_server(
            None,
            String::new(),
            host.into(),
            port.parse()?,
            user.into(),
            auth.into(),
            Some(credential),
            true,
        )
        .await?;
        eprintln!("harness: saved server {saved}");
    }

    simple::start_hydra_node(base_dir).await?;
    eprintln!("harness: node started, Ctrl-C to stop");

    let status_every = arg(&args, "--status-every").and_then(|s| s.parse::<u64>().ok());
    loop {
        tokio::select! {
            _ = tokio::signal::ctrl_c() => break,
            _ = tokio::time::sleep(std::time::Duration::from_secs(status_every.unwrap_or(3600))) => {
                if status_every.is_some() {
                    eprintln!("harness: status {}", simple::get_proxy_status().await?);
                }
            }
        }
    }
    simple::stop_hydra_node().await?;
    Ok(())
}
