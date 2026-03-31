use crate::api::{shared_state, simple, vpn};
use lazy_static::lazy_static;
use serde::Deserialize;
use std::ffi::{CStr, CString};
use std::os::raw::c_char;
use std::sync::mpsc;
use std::thread;
use tokio::runtime::Handle;

lazy_static! {
    static ref EXTENSION_RUNTIME: std::sync::Mutex<Option<ExtensionRuntime>> =
        std::sync::Mutex::new(None);
}

struct ExtensionRuntime {
    _thread: thread::JoinHandle<()>,
    handle: Handle,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ExtensionCommand {
    SetProxyMode { mode: String },
    SetConnectionProxy { conn_id: u64, proxied: bool },
    Snapshot,
}

fn ensure_extension_runtime() -> Result<Handle, String> {
    let mut guard = EXTENSION_RUNTIME
        .lock()
        .map_err(|e| format!("Extension runtime lock poisoned: {}", e))?;

    if let Some(runtime) = guard.as_ref() {
        return Ok(runtime.handle.clone());
    }

    let (tx, rx) = mpsc::channel();
    let thread = thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create extension runtime");
        let handle = rt.handle().clone();
        tx.send(handle).expect("failed to send runtime handle");
        rt.block_on(async {
            std::future::pending::<()>().await;
        });
    });

    let handle = rx
        .recv()
        .map_err(|e| format!("Failed to receive runtime handle: {}", e))?;
    *guard = Some(ExtensionRuntime {
        _thread: thread,
        handle: handle.clone(),
    });
    Ok(handle)
}

fn string_result<F>(f: F) -> *mut c_char
where
    F: FnOnce() -> Result<(), String>,
{
    match f() {
        Ok(()) => std::ptr::null_mut(),
        Err(err) => CString::new(err).unwrap_or_default().into_raw(),
    }
}

fn c_str_arg<'a>(ptr: *const c_char, name: &str) -> Result<&'a str, String> {
    if ptr.is_null() {
        return Err(format!("{} is null", name));
    }

    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|e| format!("{} is invalid UTF-8: {}", name, e))
}

fn run_on_runtime<F>(fut: F) -> Result<(), String>
where
    F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let handle = ensure_extension_runtime()?;
    let (tx, rx) = mpsc::channel();
    handle.spawn(async move {
        let result = fut.await.map_err(|e| e.to_string());
        let _ = tx.send(result);
    });
    rx.recv()
        .map_err(|e| format!("Extension command channel closed: {}", e))?
}

fn parse_command(command_json: &str) -> Result<ExtensionCommand, String> {
    serde_json::from_str(command_json).map_err(|e| format!("Invalid extension command: {}", e))
}

#[unsafe(no_mangle)]
pub extern "C" fn hydra_extension_start(base_dir: *const c_char, tun_fd: i32) -> *mut c_char {
    string_result(|| {
        let base_dir = c_str_arg(base_dir, "base_dir")?.to_string();
        run_on_runtime(async move {
            simple::init_extension_runtime(base_dir.clone())?;
            simple::start_hydra_node(base_dir).await?;
            vpn::start_vpn_tunnel(tun_fd)?;
            Ok(())
        })
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn hydra_extension_stop() -> *mut c_char {
    string_result(|| vpn::stop_vpn_tunnel().map_err(|e| e.to_string()))
}

#[unsafe(no_mangle)]
pub extern "C" fn hydra_extension_apply_command(command_json: *const c_char) -> *mut c_char {
    string_result(|| {
        let command_json = c_str_arg(command_json, "command_json")?.to_string();
        let command = parse_command(&command_json)?;
        match command {
            ExtensionCommand::SetProxyMode { mode } => {
                run_on_runtime(async move { simple::set_proxy_mode(mode).await })
            }
            ExtensionCommand::SetConnectionProxy { conn_id, proxied } => {
                run_on_runtime(async move { simple::set_connection_proxy(conn_id, proxied).await })
            }
            ExtensionCommand::Snapshot => {
                let _ = shared_state::snapshot_json().map_err(|e| e.to_string())?;
                Ok(())
            }
        }
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn hydra_extension_snapshot_json() -> *mut c_char {
    match shared_state::snapshot_json() {
        Ok(snapshot) => CString::new(snapshot).unwrap_or_default().into_raw(),
        Err(err) => CString::new(
            serde_json::json!({
                "error": err.to_string(),
            })
            .to_string(),
        )
        .unwrap_or_default()
        .into_raw(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn hydra_extension_string_free(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    unsafe {
        let _ = CString::from_raw(ptr);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_proxy_mode_command() {
        let cmd = parse_command(r#"{"type":"set_proxy_mode","mode":"full"}"#).unwrap();
        assert!(matches!(cmd, ExtensionCommand::SetProxyMode { mode } if mode == "full"));
    }
}
