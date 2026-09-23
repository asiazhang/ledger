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

// ---------------------------------------------------------------------------
// 作业类持锁预算（issue #1765，ADR-0112 决策 5 反转）：备份/恢复等锁内重 IO
// 作业经壳层启动登记自己的预算，探针阈值按命令取——命中前缀取登记值，未命中
// 维持 1s 产品阈值。负向判据（删除即红）：取掉门面取值点的 `hold_threshold_for`，
// 门面预算作业测试（budgeted_slow_job_stays_silent）即红。
// ---------------------------------------------------------------------------

/// 登记前缀按 `starts_with` 匹配：命中前缀的命令取登记预算，未命中命令维持
/// 产品阈值；命中多条登记取最宽预算。
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
