use crate::frb_generated::StreamSink;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref LOG_STREAM: Mutex<Option<StreamSink<String>>> = Mutex::new(None);
}

/// Live stream of formatted log lines (see `crate::logging` for the format).
pub fn create_log_stream(sink: StreamSink<String>) {
    let mut stream = LOG_STREAM.lock().unwrap_or_else(|e| e.into_inner());
    *stream = Some(sink);
}

#[flutter_rust_bridge::frb(ignore)]
pub(crate) fn publish(line: String) {
    crate::api::shared_state::append_log_line(line.clone());
    if let Ok(stream) = LOG_STREAM.lock() {
        if let Some(sink) = stream.as_ref() {
            let _ = sink.add(line);
        }
    }
}
