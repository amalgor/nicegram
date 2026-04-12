use crate::connections::{ClassificationSource, TrafficCategory};
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

const FLUSH_EVERY_EVENTS: usize = 100;
const FLUSH_INTERVAL: Duration = Duration::from_secs(30);
const ROTATE_AT_BYTES: u64 = 10 * 1024 * 1024;
const MAX_ROTATED_FILES: usize = 3;

pub fn default_base_dir() -> PathBuf {
    std::env::var("HYDRA_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClassificationEvent {
    pub timestamp: u64,
    pub host: String,
    pub port: u16,
    pub app_uid: Option<u32>,
    pub package_name: Option<String>,
    pub reverse_dns: Option<String>,
    pub whois_org: Option<String>,
    pub whois_asn: Option<u32>,
    pub whois_country: Option<String>,
    pub category: TrafficCategory,
    pub confidence: f32,
    pub source: ClassificationSource,
    pub explanation: Option<String>,
    pub bytes_total: u64,
    pub duration_ms: u64,
    pub user_approved: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LogStats {
    pub total_events: u64,
    pub file_size_bytes: u64,
    pub oldest_timestamp: Option<u64>,
}

#[derive(Debug)]
pub struct ClassificationEventLog {
    base_dir: PathBuf,
    tx: Sender<ClassificationEvent>,
    stats: Arc<Mutex<LogStats>>,
}

impl ClassificationEventLog {
    pub fn new(base_dir: impl AsRef<Path>) -> std::io::Result<Self> {
        let base_dir = base_dir.as_ref().to_path_buf();
        fs::create_dir_all(&base_dir)?;
        let (tx, rx) = mpsc::channel::<ClassificationEvent>();
        let stats = Arc::new(Mutex::new(LogStats::default()));
        let writer_dir = base_dir.clone();
        let writer_stats = stats.clone();

        thread::Builder::new()
            .name("classification-event-log".to_string())
            .spawn(move || writer_loop(writer_dir, writer_stats, rx))
            .map_err(std::io::Error::other)?;

        Ok(Self { base_dir, tx, stats })
    }

    pub fn log_event(&self, event: ClassificationEvent) {
        if let Err(error) = self.tx.send(event) {
            warn!("failed to enqueue classification event: {}", error);
        }
    }

    pub fn export_dataset(&self) -> std::io::Result<Vec<ClassificationEvent>> {
        let mut events = Vec::new();
        for path in self.log_paths() {
            if !path.exists() {
                continue;
            }
            let file = File::open(&path)?;
            let reader = BufReader::new(file);
            for line in reader.lines() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(event) = serde_json::from_str::<ClassificationEvent>(&line) {
                    events.push(event);
                }
            }
        }
        Ok(events)
    }

    pub fn stats(&self) -> LogStats {
        self.stats.lock().unwrap().clone()
    }

    fn log_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(MAX_ROTATED_FILES + 1);
        paths.push(self.base_dir.join("classification_events.jsonl"));
        for idx in 1..=MAX_ROTATED_FILES {
            paths.push(self.base_dir.join(format!("classification_events.{idx}.jsonl")));
        }
        paths
    }
}

fn writer_loop(base_dir: PathBuf, stats: Arc<Mutex<LogStats>>, rx: Receiver<ClassificationEvent>) {
    let path = base_dir.join("classification_events.jsonl");
    let mut writer = open_writer(&path).ok();
    let mut pending = 0usize;
    let mut last_flush = SystemTime::now();

    loop {
        match rx.recv_timeout(FLUSH_INTERVAL) {
            Ok(event) => {
                if writer.is_none() {
                    writer = open_writer(&path).ok();
                }
                if let Some(w) = writer.as_mut() {
                    if let Ok(line) = serde_json::to_string(&event) {
                        if w.write_all(line.as_bytes()).is_ok() && w.write_all(b"\n").is_ok() {
                            pending += 1;
                            update_stats(&stats, &path, event.timestamp);
                        }
                    }
                }

                if pending >= FLUSH_EVERY_EVENTS || path_size(&path) >= ROTATE_AT_BYTES {
                    if let Some(w) = writer.as_mut() {
                        let _ = w.flush();
                    }
                    rotate_logs(&base_dir);
                    writer = open_writer(&path).ok();
                    pending = 0;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_flush.elapsed().unwrap_or_default() >= FLUSH_INTERVAL {
                    if let Some(w) = writer.as_mut() {
                        let _ = w.flush();
                    }
                    last_flush = SystemTime::now();
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if let Some(w) = writer.as_mut() {
        let _ = w.flush();
    }
}

fn update_stats(stats: &Arc<Mutex<LogStats>>, path: &Path, ts: u64) {
    let mut guard = stats.lock().unwrap();
    guard.total_events = guard.total_events.saturating_add(1);
    guard.file_size_bytes = path_size(path);
    guard.oldest_timestamp = Some(guard.oldest_timestamp.map_or(ts, |old| old.min(ts)));
}

fn open_writer(path: &Path) -> std::io::Result<BufWriter<File>> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    Ok(BufWriter::new(file))
}

fn path_size(path: &Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn rotate_logs(base_dir: &Path) {
    for idx in (1..=MAX_ROTATED_FILES).rev() {
        let src = if idx == 1 {
            base_dir.join("classification_events.jsonl")
        } else {
            base_dir.join(format!("classification_events.{}.jsonl", idx - 1))
        };
        let dst = base_dir.join(format!("classification_events.{}.jsonl", idx));
        if dst.exists() {
            let _ = fs::remove_file(&dst);
        }
        if src.exists() {
            let _ = fs::rename(&src, &dst);
        }
    }
    info!("classification log rotated");
}

pub fn make_event(
    host: String,
    port: u16,
    app_uid: Option<u32>,
    package_name: Option<String>,
    reverse_dns: Option<String>,
    whois_org: Option<String>,
    whois_asn: Option<u32>,
    whois_country: Option<String>,
    category: TrafficCategory,
    confidence: f32,
    source: ClassificationSource,
    explanation: Option<String>,
    bytes_total: u64,
    duration_ms: u64,
    user_approved: Option<bool>,
) -> ClassificationEvent {
    ClassificationEvent {
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        host,
        port,
        app_uid,
        package_name,
        reverse_dns,
        whois_org,
        whois_asn,
        whois_country,
        category,
        confidence,
        source,
        explanation,
        bytes_total,
        duration_ms,
        user_approved,
    }
}
