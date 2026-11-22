#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]
#[cfg(not(any(test, feature = "test-utils")))]
compile_error!(
    "test_utils 被编入生产构建：crate 根 cfg 门缺失或失效（ADR-0111 决策 5 / issue #1132）"
);

use std::sync::{Arc, Condvar, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use crate::events::SignalEmitter;

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

#[derive(Clone, Debug)]
pub struct CapturedEvent {
    pub level: Level,
    pub current_span: Option<String>,
    pub fields: Vec<(String, String)>,
}

pub struct CaptureLayer {
    pub events: Arc<Mutex<Vec<CapturedEvent>>>,
}

impl CaptureLayer {
    pub fn new() -> Self {
        CaptureLayer {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Default for CaptureLayer {
    fn default() -> Self {
        Self::new()
    }
}

struct FieldCapture {
    fields: Vec<(String, String)>,
}

impl Visit for FieldCapture {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.fields
            .push((field.name().to_string(), format!("{value:?}")));
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
    fn record_i64(&mut self, field: &Field, value: i64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
    fn record_u64(&mut self, field: &Field, value: u64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
    fn record_f64(&mut self, field: &Field, value: f64) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
    fn record_bool(&mut self, field: &Field, value: bool) {
        self.fields
            .push((field.name().to_string(), value.to_string()));
    }
}

impl<S> Layer<S> for CaptureLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut capture = FieldCapture { fields: Vec::new() };
        event.record(&mut capture);
        self.events.lock().unwrap().push(CapturedEvent {
            level: *event.metadata().level(),
            current_span: ctx.current_span().metadata().map(|m| m.name().to_string()),
            fields: capture.fields,
        });
    }
}

static ENSURE_GLOBAL_MAX_LEVEL: Once = Once::new();

pub fn ensure_global_max_level() {
    ENSURE_GLOBAL_MAX_LEVEL.call_once(|| {
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
    });
}

pub fn capture_events(f: impl FnOnce()) -> Vec<CapturedEvent> {
    ensure_global_max_level();
    let layer = CaptureLayer::new();
    let captured = Arc::clone(&layer.events);
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, f);
    captured.lock().unwrap().clone()
}

pub const GATED_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Default)]
struct GatedShared {
    posted: Vec<&'static str>,
    delivered: Vec<&'static str>,
    gate_open: bool,
}

#[derive(Clone)]
pub struct GatedEmitter {
    shared: Arc<(Mutex<GatedShared>, Condvar)>,
}

impl GatedEmitter {
    pub fn gated() -> Self {
        let emitter = Self {
            shared: Arc::new((Mutex::new(GatedShared::default()), Condvar::new())),
        };
        let worker = emitter.clone();
        thread::spawn(move || worker.run_fake_main_thread());
        emitter
    }

    fn run_fake_main_thread(&self) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        while !state.gate_open {
            state = cv.wait(state).unwrap();
        }
        let pending = std::mem::take(&mut state.posted);
        state.delivered.extend(pending);
        cv.notify_all();
    }

    pub fn open_gate(&self) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.gate_open = true;
        cv.notify_all();
    }

    pub fn posted(&self) -> Vec<&'static str> {
        self.shared.0.lock().unwrap().posted.clone()
    }

    pub fn delivered(&self) -> Vec<&'static str> {
        self.shared.0.lock().unwrap().delivered.clone()
    }

    pub fn wait_posted(&self) -> Vec<&'static str> {
        self.wait_until(|s| !s.posted.is_empty(), "发射器交接")
            .posted
            .clone()
    }

    pub fn wait_delivered(&self) -> Vec<&'static str> {
        self.wait_until(|s| !s.delivered.is_empty(), "信号最终到达")
            .delivered
            .clone()
    }

    fn wait_until(&self, pred: impl Fn(&GatedShared) -> bool, what: &str) -> GatedShared {
        let (lock, cv) = &*self.shared;
        let deadline = Instant::now() + GATED_TIMEOUT;
        let mut state = lock.lock().unwrap();
        while !pred(&state) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            assert!(!remaining.is_zero(), "等待{what}超时（{GATED_TIMEOUT:?}）");
            let (guard, _) = cv.wait_timeout(state, remaining).unwrap();
            state = guard;
        }
        state.clone()
    }
}

impl SignalEmitter for GatedEmitter {
    fn post(&self, event: &'static str) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.posted.push(event);
        cv.notify_all();
    }
}

pub const FOREIGN_RESERVED_BYTES: u8 = 12;

const FIXTURE_PAGE_SIZE: usize = 4096;

pub fn read_reserved_byte(path: &std::path::Path) -> u8 {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path).unwrap();
    file.seek(SeekFrom::Start(
        crate::boot::encryption::HEADER_RESERVED_BYTES_OFFSET as u64,
    ))
    .unwrap();
    let mut byte = [0u8; 1];
    file.read_exact(&mut byte).unwrap();
    byte[0]
}

fn foreign_form_empty_page() -> Vec<u8> {
    let mut page = vec![0u8; FIXTURE_PAGE_SIZE];
    page[..16].copy_from_slice(&crate::boot::encryption::SQLITE_HEADER_MAGIC);
    page[16..18].copy_from_slice(&(FIXTURE_PAGE_SIZE as u16).to_be_bytes());
    page[18] = 1;
    page[19] = 1;
    page[20] = FOREIGN_RESERVED_BYTES;
    page[21] = 64;
    page[22] = 32;
    page[23] = 32;
    page[24..28].copy_from_slice(&1u32.to_be_bytes());
    page[28..32].copy_from_slice(&1u32.to_be_bytes());
    page[44..48].copy_from_slice(&4u32.to_be_bytes());
    page[56..60].copy_from_slice(&1u32.to_be_bytes());
    let usable = FIXTURE_PAGE_SIZE - usize::from(FOREIGN_RESERVED_BYTES);
    page[100] = 0x0d;
    page[105..107].copy_from_slice(&(usable as u16).to_be_bytes());
    page
}

pub fn write_foreign_form_plaintext_db(path: &std::path::Path, rows: usize) {
    std::fs::write(path, foreign_form_empty_page()).unwrap();
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE fixture_probe(name TEXT)")
        .unwrap();
    for i in 0..rows {
        conn.execute(
            "INSERT INTO fixture_probe(name) VALUES (?1)",
            [format!("夹具行-{i}")],
        )
        .unwrap();
    }
}

pub fn attach_in_memory_sync(conn: &rusqlite::Connection) {
    conn.execute_batch("ATTACH DATABASE ':memory:' AS sync")
        .expect("内存世界成对挂载（attached sync 侧就位）");
}
