//! 持锁时长探针测试（issue #1276 守门③）：超阈值记 warn 日志、不静默；低于
//! 阈值保持安静。接线面（三个取锁点都调用探针）由调用点结构保证，越界行为
//! 的可观察结果（warn 事件）在此钉住。

use std::time::Duration;

use tracing::Level;

use crate::db::probe_lock_hold;
use crate::test_utils::capture_events;

/// 超阈值：记 warn，且事件携带持有时长与阈值字段（可归因、不静默）。
#[test]
fn lock_hold_probe_warns_past_threshold() {
    let events = capture_events(|| probe_lock_hold(Duration::from_secs(2)));
    let warn = events
        .iter()
        .find(|e| e.level == Level::WARN)
        .expect("超阈值应记 warn 日志");
    assert!(
        warn.fields
            .iter()
            .any(|(k, _)| k == "hold_ms" || k == "threshold_ms"),
        "warn 事件应携带持有时长/阈值字段，实际: {warn:?}"
    );
}

/// 低于阈值：零事件（合法单事务的常态，探针保持安静不制造噪音）。
#[test]
fn lock_hold_probe_stays_silent_below_threshold() {
    let events = capture_events(|| probe_lock_hold(Duration::from_millis(1)));
    assert!(
        events.iter().all(|e| e.level != Level::WARN),
        "低于阈值不应记 warn，实际捕获: {events:?}"
    );
}
