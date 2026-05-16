// In-memory rolling log buffer + tracing layer that mirrors anything
// emitted at INFO+ into a ring we can read back from the UI.
//
// Reason for existence: iOS stopped routing the Rust stderr stream into
// the device syslog at some point, so `idevicesyslog` no longer shows
// our `tracing::info!` lines during pairing investigation. Rather than
// chase the iOS log routing, we keep our own buffer the UI can pull.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::fmt::Write as _;
use std::sync::OnceLock;
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

const CAPACITY: usize = 500;

static BUFFER: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();

fn buffer() -> &'static Mutex<VecDeque<String>> {
    BUFFER.get_or_init(|| Mutex::new(VecDeque::with_capacity(CAPACITY)))
}

/// Returns a copy of the buffered log lines, oldest first.
pub fn snapshot() -> Vec<String> {
    buffer().lock().iter().cloned().collect()
}

pub struct RingLayer;

impl<S: Subscriber> Layer<S> for RingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut line = String::new();
        // chrono is already in the dep tree; reuse it for a readable timestamp.
        let ts = chrono::Local::now().format("%H:%M:%S%.3f");
        let meta = event.metadata();
        let _ = write!(line, "{ts} {:>5} {}: ", meta.level(), meta.target());

        struct Visitor<'a>(&'a mut String);
        impl tracing::field::Visit for Visitor<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "message" {
                    let _ = write!(self.0, "{value:?}");
                } else {
                    let _ = write!(self.0, " {}={value:?}", field.name());
                }
            }
        }
        event.record(&mut Visitor(&mut line));

        let mut buf = buffer().lock();
        if buf.len() >= CAPACITY {
            buf.pop_front();
        }
        buf.push_back(line);
    }
}
