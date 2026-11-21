use std::cell::Cell;
use std::time::Duration;

use rusqlite::Connection;
use rusqlite::trace::{TraceEvent, TraceEventCodes};

pub const DEFAULT_SLOW_QUERY_THRESHOLD: Duration = Duration::from_millis(100);

thread_local! {
    static THRESHOLD_NANOS: Cell<Option<u64>> = const { Cell::new(None) };
}

fn current_threshold() -> Duration {
    THRESHOLD_NANOS.with(|c| match c.get() {
        Some(ns) => Duration::from_nanos(ns),
        None => DEFAULT_SLOW_QUERY_THRESHOLD,
    })
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum TimingClass {
    Slow,
    Normal,
}

pub fn timing_level(threshold: Duration, duration: Duration) -> TimingClass {
    if duration > threshold {
        TimingClass::Slow
    } else {
        TimingClass::Normal
    }
}

pub fn install_perf_trace(conn: &Connection, threshold: Duration) {
    THRESHOLD_NANOS.with(|c| c.set(Some(threshold.as_nanos() as u64)));
    conn.trace_v2(
        TraceEventCodes::SQLITE_TRACE_PROFILE,
        Some(perf_trace_callback),
    );
}

fn perf_trace_callback(event: TraceEvent<'_>) {
    let TraceEvent::Profile(stmt, duration) = event else {
        return;
    };

    let class = timing_level(current_threshold(), duration);
    let sql = stmt.sql();
    let duration_ms = duration.as_secs_f64() * 1000.0;
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
