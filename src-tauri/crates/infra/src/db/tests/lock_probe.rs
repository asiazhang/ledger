use std::time::Duration;

use tracing::Level;

use crate::db::probe_lock_hold;
use crate::test_utils::capture_events;

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

#[test]
fn lock_hold_probe_stays_silent_below_threshold() {
    let events = capture_events(|| probe_lock_hold(Duration::from_millis(1)));
    assert!(
        events.iter().all(|e| e.level != Level::WARN),
        "低于阈值不应记 warn，实际捕获: {events:?}"
    );
}

#[test]
fn hold_threshold_for_matches_registered_prefix_only() {
    use crate::db::register_lock_hold_budget;

    const PREFIX: &str = "lock-probe-budget-test.";
    const BUDGET: Duration = Duration::from_secs(30);
    register_lock_hold_budget(PREFIX, BUDGET);

    assert_eq!(
        crate::db::runtime::hold_threshold_for("lock-probe-budget-test.exit-fallback"),
        BUDGET,
        "命中登记前缀的命令取登记预算"
    );
    assert_eq!(
        crate::db::runtime::hold_threshold_for("unregistered-command"),
        crate::db::LOCK_HOLD_PROBE_THRESHOLD,
        "未命中登记前缀的命令维持产品阈值"
    );
}
