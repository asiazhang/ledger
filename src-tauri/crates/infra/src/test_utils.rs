//! 测试支持：捕获 tracing 事件的 Layer、全局最大级别稳定器与闸门式假发射器。
//!
//! 供本 crate 的单元测试（`db/tests.rs`、`signals/emit_blocking_tests.rs`）与
//! 集成测试（`tests/api_server/`）共用，避免两处重复实现采集器具
//!（issue #44 code review：Duplicated Code）与假发射器（spec #367 code review：
//! 同一坏味道在测试桩上重演）。
//!
//! 说明：集成测试 `tests/api_server/` 链接的是非 `#[cfg(test)]` 构建的 lib，
//! 因此本模块不能仅以 `#[cfg(test)]` 编译；`#[doc(hidden)]` 使其不进入文档，
//! 对生产二进制的影响只是一些未使用的测试辅助类型（可被编译器消除）。
//
// C 类豁免（ADR-0060）：仅测试用——本模块被集成测试以非 cfg(test) 构建链接，
// 无法经 crate 根 cfg(test) 豁免覆盖，故文件级放行六件套；生产路径不得消费本模块。
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable
)]

use std::sync::{Arc, Condvar, Mutex, Once};
use std::thread;
use std::time::{Duration, Instant};

use crate::events::SignalEmitter;

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

/// 单次等待的超时上界：正常路径微秒/毫秒级即过；仅当被钉死的外部行为真的
/// 发生回归（发射阻塞写路径 / 信号永久丢失）时才耗满并失败。
pub const GATED_TIMEOUT: Duration = Duration::from_secs(5);

/// 闸门式假发射器的共享态：`posted` = 已交接（`post` 已调用、待送达）；
/// `delivered` = 假主线程已送达；`gate_open` = 闸门放行。
#[derive(Clone, Default)]
struct GatedShared {
    posted: Vec<&'static str>,
    delivered: Vec<&'static str>,
    gate_open: bool,
}

/// 闸门式假发射器（spec #364 测试哲学的共享测试桩）：`post` 只把事件
/// **交接**给内部「假主线程」即返回（与生产 `AppHandle` 实现的非阻塞投递
/// 约定同形）；假主线程在闸门放行前一直阻塞——模拟「发射迟迟未完成 /
/// 信号延迟」（真实事故中即主线程被 `webviews_lock` 卡住、emit 永不返回）。
/// 放行后把已交接事件按入队顺序依次标记为已送达并退出（测试生命周期内
/// 单次放行）。可克隆，写线程与测试各持一份。
///
/// 两个验证靶共用本桩（spec #366 / #367）：机制接缝层
///（`signals::emit_blocking_tests`，断言发射器阻塞期间写路径仍及时返回）
/// 与 HTTP 壳整链（`tests/api_server/signal_delivery.rs`，断言写请求返回后
/// 信号最终到达）。所有等待都带超时上界——回归发生时测试超时失败，而非
/// 永久挂起；刻意不复现真实死锁时序（跨线程时序问题易 flaky）。
#[derive(Clone)]
pub struct GatedEmitter {
    shared: Arc<(Mutex<GatedShared>, Condvar)>,
}

impl GatedEmitter {
    /// 新建闸门关闭（发射延迟中）的假发射器，并启动假主线程。
    pub fn gated() -> Self {
        let emitter = Self {
            shared: Arc::new((Mutex::new(GatedShared::default()), Condvar::new())),
        };
        let worker = emitter.clone();
        thread::spawn(move || worker.run_fake_main_thread());
        emitter
    }

    /// 假主线程：模拟「主线程事件循环里执行 emit」。放行前阻塞等待；
    /// 放行后把已交接事件按入队顺序依次「送达」并退出。
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

    /// 开闸放行（模拟主线程恢复，队首待送达的信号得以执行）。
    pub fn open_gate(&self) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.gate_open = true;
        cv.notify_all();
    }

    /// 已交接事件快照（不等待）。
    pub fn posted(&self) -> Vec<&'static str> {
        self.shared.0.lock().unwrap().posted.clone()
    }

    /// 已送达事件快照（不等待）。
    pub fn delivered(&self) -> Vec<&'static str> {
        self.shared.0.lock().unwrap().delivered.clone()
    }

    /// 等到有交接（带超时），返回快照——证明写路径确实把事件交给了发射器。
    pub fn wait_posted(&self) -> Vec<&'static str> {
        self.wait_until(|s| !s.posted.is_empty(), "发射器交接")
            .posted
            .clone()
    }

    /// 等到有「送达完成」（带超时），返回快照——断言「信号最终到达、不丢失」。
    pub fn wait_delivered(&self) -> Vec<&'static str> {
        self.wait_until(|s| !s.delivered.is_empty(), "信号最终到达")
            .delivered
            .clone()
    }

    /// 谓词等待（带超时上界）：条件满足即返回共享态快照，超时即 panic。
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
    /// 非阻塞交接：记录事件并唤醒假主线程后**立即返回**，不等送达完成
    ///（与生产 `AppHandle` 实现同一约定——这正是本桩帮各测试钉死的契约）。
    fn post(&self, event: &'static str) {
        let (lock, cv) = &*self.shared;
        let mut state = lock.lock().unwrap();
        state.posted.push(event);
        cv.notify_all();
    }
}
