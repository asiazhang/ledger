//! 测试支持：捕获 tracing 事件的 Layer 与全局最大级别稳定器。
//!
//! 供本 crate 的单元测试（`db/tests.rs`）与集成测试（`tests/api_server.rs`）
//! 共用，避免两处重复实现采集器具（issue #44 code review：Duplicated Code）。
//!
//! 说明：集成测试 `tests/api_server.rs` 链接的是非 `#[cfg(test)]` 构建的 lib，
//! 因此本模块不能仅以 `#[cfg(test)]` 编译；`#[doc(hidden)]` 使其不进入文档，
//! 对生产二进制的影响只是一些未使用的测试辅助类型（可被编译器消除）。

use std::sync::{Arc, Mutex, Once};

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};

/// 捕获到的 tracing 事件，含事件级别、发射时的当前（最内层）span 名与字段。
#[derive(Clone, Debug)]
pub struct CapturedEvent {
    pub level: Level,
    /// 事件发射时的当前（最内层）span 名，用于验证事件是否归因到调用方 span。
    pub current_span: Option<String>,
    pub fields: Vec<(String, String)>,
}

/// 捕获 tracing 事件的测试层，把每个事件的级别、字段与当前 span 名记录到共享 Vec。
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

/// 遍历事件字段的 Visit 实现，把字段名与值收集为字符串。
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

/// 常驻一个 no-op 全局 subscriber，把 `tracing` 的全局 `LevelFilter::current()`
/// 稳定在 TRACE。否则并发测试线程各自注册/注销 dispatch 时，全局 MAX_LEVEL
/// 会短暂降到 OFF，导致 `tracing::debug!`/`trace!` 宏的快路径提前过滤，
/// 线程级 `set_default`/`with_default` 捕获就收不到事件（`cargo test --all` 并行时偶发）。
/// 该全局 subscriber 不截获任何日志；仅用于稳定级别判定。
static ENSURE_GLOBAL_MAX_LEVEL: Once = Once::new();

pub fn ensure_global_max_level() {
    ENSURE_GLOBAL_MAX_LEVEL.call_once(|| {
        let _ = tracing::subscriber::set_global_default(tracing_subscriber::registry());
    });
}

/// 在捕获 subscriber 生效期间执行 `f`（线程本地），返回捕获到的事件。
///
/// SQL 执行时 `trace_v2` 回调在调用线程同步发射，故能被同一线程捕获；
/// 线程内 `tracing::info!`/`debug!` 等事件同样被捕获。先调用
/// [`ensure_global_max_level`] 稳定全局级别，避免并发测试下快路径误滤。
pub fn capture_events(f: impl FnOnce()) -> Vec<CapturedEvent> {
    ensure_global_max_level();
    let layer = CaptureLayer::new();
    let captured = Arc::clone(&layer.events);
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, f);
    captured.lock().unwrap().clone()
}
