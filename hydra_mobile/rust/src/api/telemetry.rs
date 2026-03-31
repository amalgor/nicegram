use crate::frb_generated::StreamSink;
use std::sync::Mutex;

lazy_static::lazy_static! {
    static ref LOG_STREAM: Mutex<Option<StreamSink<String>>> = Mutex::new(None);
}

pub fn create_log_stream(sink: StreamSink<String>) {
    let mut stream = LOG_STREAM.lock().unwrap();
    *stream = Some(sink);
}

// A custom tracing layer to send logs to Flutter
pub struct FlutterLogLayer;

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for FlutterLogLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        let mut visitor = StringVisitor::new();
        event.record(&mut visitor);
        let meta = event.metadata();
        let log_msg = format!("[{}] {}: {}", meta.level(), meta.target(), visitor.message);
        crate::api::shared_state::append_log_line(log_msg.clone());

        if let Ok(stream) = LOG_STREAM.lock() {
            if let Some(sink) = stream.as_ref() {
                let _ = sink.add(log_msg);
            }
        }
    }
}

struct StringVisitor {
    message: String,
}

impl StringVisitor {
    fn new() -> Self {
        Self {
            message: String::new(),
        }
    }
}

impl tracing::field::Visit for StringVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        }
    }
}
