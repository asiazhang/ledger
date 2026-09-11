//! 数据库操作耗时日志（Perf Trace）。
//!
//! 通过 `Connection::trace_v2` 的 PROFILE 事件为每条 SQL 语句记录耗时，
//! 覆盖所有执行上下文（IPC 命令、HTTP 导入、定时引擎、后台索引刷新、启动迁移）。
//! 触发装置的注册集中在本模块，由 `open_connection` / `open_in_memory`
//! 两个连接工厂共享调用，业务代码零改动（ADR-0009）。
//!
//! 级别策略：单条 SQL 耗时超过阈值 → `warn`（慢查询），否则 `debug`（全量明细）。
//! 默认阈值 100ms，由 [`DEFAULT_SLOW_QUERY_THRESHOLD`] 提供。
//!
//! 隐私约定（ADR-0006 / ADR-0009）：带占位符的 SQL 原文在所有级别均可记录；
//! 展开 SQL（内联参数值）仅 DEBUG 级记录，避免默认级别下把金额/备注等业务值
//! 落到日志文件。

use std::cell::Cell;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::trace::{TraceEvent, TraceEventCodes};

/// 慢查询默认阈值：单条 SQL 执行耗时超过该值以 warn 级别记录。
pub const DEFAULT_SLOW_QUERY_THRESHOLD: Duration = Duration::from_millis(100);

// 当前连接工厂设定的阈值（纳秒）。
//
// SQLite 的 `trace_v2` 只接受裸 `fn` 指针回调，无法通过闭包捕获阈值，
// 故借助线程局部变量传递。`install_perf_trace` 与 SQL 执行通常在同一线程
// （测试场景完全如此）；若 SQL 在其它线程执行，回调回退到默认阈值
// （应用内阈值即默认值，无行为差异）。
thread_local! {
    static THRESHOLD_NANOS: Cell<Option<u64>> = const { Cell::new(None) };
}

fn current_threshold() -> Duration {
    THRESHOLD_NANOS.with(|c| match c.get() {
        Some(ns) => Duration::from_nanos(ns),
        None => DEFAULT_SLOW_QUERY_THRESHOLD,
    })
}

/// 单条 SQL 耗时分类结果（级别决策核心的返回值）。
#[derive(PartialEq, Debug, Clone, Copy)]
pub enum TimingClass {
    /// 耗时超过阈值 → 慢查询（warn 级）。
    Slow,
    /// 未超过阈值 → 全量明细（debug 级）。
    Normal,
}

/// 分类纯函数：单条 SQL 耗时超过阈值 → [`TimingClass::Slow`]，否则 [`TimingClass::Normal`]。
///
/// 边界语义为严格大于：恰好等于阈值仍视为正常，略高于阈值才升级为慢查询。
/// 此函数是级别决策的唯一核心，独立成纯函数便于边界测试。
pub fn timing_level(threshold: Duration, duration: Duration) -> TimingClass {
    if duration > threshold {
        TimingClass::Slow
    } else {
        TimingClass::Normal
    }
}

/// 在连接上注册耗时 hook（`trace_v2` PROFILE 事件）并记录分类阈值。
///
/// 由 `open_connection` / `open_in_memory` 两个连接工厂共享调用；
/// 默认 100ms 阈值由 [`DEFAULT_SLOW_QUERY_THRESHOLD`] 提供。阈值可注入
/// 使测试无需构造慢语句即可命中 warn 分支。
pub fn install_perf_trace(conn: &Connection, threshold: Duration) {
    THRESHOLD_NANOS.with(|c| c.set(Some(threshold.as_nanos() as u64)));
    conn.trace_v2(
        TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(perf_trace_callback),
    );
}

/// 单条 SQL 执行完成的 PROFILE 回调：按耗时分类发射事件。
fn perf_trace_callback(event: TraceEvent<'_>) {
    let TraceEvent::Profile(stmt, duration) = event else {
        return;
    };

    let class = timing_level(current_threshold(), duration);
    let sql = stmt.sql();
    let duration_ms = duration.as_secs_f64() * 1000.0;
    // 展开 SQL（含内联参数值）仅在 debug 级别实际生效时记录，延续隐私约定
    // （ADR-0006 / ADR-0009：默认级别不落金额/备注等业务值）。
    // 在 debug 级别下对所有语句（含慢查询）都展开，保证性能分析拿得到完整语句。
    let expanded_sql = tracing::enabled!(tracing::Level::DEBUG)
        .then(|| stmt.expanded_sql())
        .flatten();

    match class {
        TimingClass::Slow => {
            tracing::warn!(sql = %sql, expanded_sql = ?expanded_sql, duration_ms, "慢查询");
        }
        TimingClass::Normal => {
            tracing::debug!(sql = %sql, expanded_sql = ?expanded_sql, duration_ms, "SQL 执行");
        }
    }
}
