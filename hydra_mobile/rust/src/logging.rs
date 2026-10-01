//! Process-wide logging: one `tracing` pipeline feeding
//! - the live Flutter log view (stream + in-memory backfill ring),
//! - rotating log files on disk (survive crashes/suspension, exportable),
//! - stderr (visible in Xcode / `devicectl --console` / the harness).
//!
//! Line format (shared with `lib/logging/log_store.dart`):
//! `HH:MM:SS.mmm [LEVEL] target: message key=value ...`

use std::fmt::Write as _;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::OnceLock;

const MAX_LOG_FILES: usize = 10;
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
const FILE_QUEUE_DEPTH: usize = 20_000;
const PANIC_FILE: &str = "panic.log";

/// Default verbosity: debug for our crates, quieter for chatty dependencies.
/// The Flutter view filters further (INFO by default).
const DEFAULT_FILTER: &str = "debug,russh=info,hyper=info,h2=info,rustls=warn,tungstenite=info,\
tokio_tungstenite=info,hickory_proto=info,hickory_resolver=info,leaf=info,sled=info,libp2p=info,\
mio=info,want=info,reqwest=info";

struct FileSink {
    tx: SyncSender<String>,
    dir: PathBuf,
}

static FILE_SINK: OnceLock<FileSink> = OnceLock::new();
static INIT: OnceLock<()> = OnceLock::new();

/// Install the global subscriber (idempotent). `log_dir` enables file logs.
pub fn init(log_dir: Option<PathBuf>) {
    if let Some(dir) = log_dir.as_ref() {
        if FILE_SINK.get().is_none() {
            match open_file_sink(dir) {
                Ok(sink) => {
                    let _ = FILE_SINK.set(sink);
                }
                Err(e) => eprintln!("hydra: file logging disabled: {e:#}"),
            }
        }
    }

    if INIT.set(()).is_err() {
        return;
    }

    use tracing_subscriber::layer::SubscriberExt;
    let filter = tracing_subscriber::EnvFilter::try_from_env("HYDRA_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(DEFAULT_FILTER));
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(HydraLogLayer);
    let _ = tracing::subscriber::set_global_default(subscriber);

    install_panic_hook();
    report_previous_panic();
}

pub fn log_dir() -> Option<PathBuf> {
    FILE_SINK.get().map(|sink| sink.dir.clone())
}

/// Newest first.
pub fn log_files() -> Vec<PathBuf> {
    let Some(dir) = log_dir() else {
        return Vec::new();
    };
    let mut files = list_session_logs(&dir);
    files.reverse();
    files
}

/// Format and emit one line to every sink. Also used for lines that
/// originate in Dart so the file has a single interleaved timeline.
pub fn emit(level: &str, target: &str, body: &str) {
    let line = format!(
        "{} [{}] {}: {}",
        chrono::Local::now().format("%H:%M:%S%.3f"),
        level,
        target,
        body
    );
    if std::env::var_os("HYDRA_LOG_STDERR").is_some() || cfg!(debug_assertions) || cfg!(target_os = "ios") {
        eprintln!("{line}");
    }
    if let Some(sink) = FILE_SINK.get() {
        // Never block the caller on disk I/O; drop lines if the writer lags.
        let _ = sink.tx.try_send(line.clone());
    }
    crate::api::telemetry::publish(line);
}

struct HydraLogLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for HydraLogLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut visitor = FieldVisitor::default();
        event.record(&mut visitor);
        let meta = event.metadata();
        let mut body = visitor.message;
        if !visitor.fields.is_empty() {
            if !body.is_empty() {
                body.push(' ');
            }
            body.push_str(&visitor.fields);
        }
        emit(meta.level().as_str(), meta.target(), &body);
    }
}

#[derive(Default)]
struct FieldVisitor {
    message: String,
    fields: String,
}

impl FieldVisitor {
    fn push_field(&mut self, name: &str, value: std::fmt::Arguments<'_>) {
        if !self.fields.is_empty() {
            self.fields.push(' ');
        }
        let _ = write!(self.fields, "{name}={value}");
    }
}

impl tracing::field::Visit for FieldVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.push_field(field.name(), format_args!("{value}"));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{value:?}");
        } else {
            self.push_field(field.name(), format_args!("{value:?}"));
        }
    }
}

fn open_file_sink(dir: &Path) -> anyhow::Result<FileSink> {
    std::fs::create_dir_all(dir)?;
    prune_old_logs(dir);
    let path = new_session_path(dir);
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let (tx, rx) = mpsc::sync_channel::<String>(FILE_QUEUE_DEPTH);
    let sink = FileSink {
        tx,
        dir: dir.to_path_buf(),
    };
    std::thread::Builder::new()
        .name("hydra-log-writer".into())
        .spawn({
            let dir = dir.to_path_buf();
            move || writer_loop(dir, file, rx)
        })?;
    Ok(sink)
}

fn writer_loop(dir: PathBuf, file: File, rx: Receiver<String>) {
    let mut written = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut out = BufWriter::new(file);
    let _ = writeln!(
        out,
        "==== Hydra log session started {} (pid {}) ====",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S %:z"),
        std::process::id()
    );
    while let Ok(line) = rx.recv() {
        let mut batch = vec![line];
        while let Ok(more) = rx.try_recv() {
            batch.push(more);
            if batch.len() >= 512 {
                break;
            }
        }
        for line in batch {
            written += line.len() as u64 + 1;
            let _ = writeln!(out, "{line}");
        }
        // Flush per batch: the app can be killed while suspended at any time.
        let _ = out.flush();

        if written > MAX_FILE_BYTES {
            let path = new_session_path(&dir);
            match OpenOptions::new().create(true).append(true).open(&path) {
                Ok(file) => {
                    out = BufWriter::new(file);
                    written = 0;
                    prune_old_logs(&dir);
                }
                Err(e) => eprintln!("hydra: log rotation failed: {e}"),
            }
        }
    }
}

fn new_session_path(dir: &Path) -> PathBuf {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S%.3f");
    dir.join(format!("hydra-{stamp}.log"))
}

/// Oldest first.
fn list_session_logs(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("hydra-") && n.ends_with(".log"))
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn prune_old_logs(dir: &Path) {
    let files = list_session_logs(dir);
    if files.len() >= MAX_LOG_FILES {
        for old in &files[..files.len() + 1 - MAX_LOG_FILES] {
            let _ = std::fs::remove_file(old);
        }
    }
}

fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "<non-string panic payload>".into());
        let backtrace = std::backtrace::Backtrace::force_capture();
        let report = format!(
            "PANIC in thread '{}' at {}: {}\n{}",
            thread.name().unwrap_or("<unnamed>"),
            location,
            payload,
            backtrace
        );
        // Written synchronously: a panic may be followed by an abort.
        if let Some(dir) = log_dir() {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(dir.join(PANIC_FILE)) {
                let _ = writeln!(f, "{} {}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), report);
            }
        }
        emit("ERROR", "panic", &report.replace('\n', " | "));
        previous(info);
    }));
}

fn report_previous_panic() {
    let Some(dir) = log_dir() else { return };
    let path = dir.join(PANIC_FILE);
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return;
    };
    tracing::error!(
        target: "panic",
        "Previous session crashed with a Rust panic:\n{}",
        contents.trim()
    );
    let _ = std::fs::rename(&path, dir.join("panic.reported.log"));
}
