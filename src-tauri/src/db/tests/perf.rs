//! 性能相关测试：`security_lots` 聚合覆盖索引的实际命中、耗时分级纯函数边界
//! 与 perf trace hook 接线（ADR-0009）。

use std::time::Duration;

use rusqlite::Connection;
use tracing::Level;

use crate::db::{init_db, open_in_memory, perf_trace};
use crate::test_utils::capture_events;

/// security_lots 聚合索引：partial covering index 存在并覆盖聚合列，旧冗余索引已删除，
/// 且 v_holdings 聚合子查询实际命中该覆盖索引（EXPLAIN QUERY PLAN 出现索引名）。
#[test]
fn security_lots_active_covering_index() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();

    // 新 partial covering index 存在，含 partial 谓词与全部聚合列。
    let sql: String = conn
        .query_row(
            "SELECT sql FROM sqlite_master WHERE type='index' AND name='idx_security_lots_active_covering'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        sql.contains("remaining_quantity > 0"),
        "应为 partial index: {sql}"
    );
    for col in [
        "account_id",
        "instrument_id",
        "currency_code",
        "remaining_quantity",
        "cost_per_unit_cents",
        "updated_at",
    ] {
        assert!(sql.contains(col), "covering index 应包含 {col}: {sql}");
    }

    // 旧冗余索引已删除（account_id+instrument_id 查询由 UNIQUE 自动索引覆盖）。
    let old: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name='idx_security_lots_account_instrument'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        old, 0,
        "旧冗余索引 idx_security_lots_account_instrument 应已删除"
    );

    // 聚合子查询应使用新覆盖索引，避免全表扫描与回表。
    let mut stmt = conn
        .prepare(
            "EXPLAIN QUERY PLAN \
             SELECT account_id, instrument_id, currency_code, \
             SUM(remaining_quantity), SUM(remaining_quantity * cost_per_unit_cents), MAX(updated_at) \
             FROM security_lots WHERE remaining_quantity > 0 \
             GROUP BY account_id, instrument_id, currency_code",
        )
        .unwrap();
    let details: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(3))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    let plan = details.join(" | ");
    assert!(
        plan.contains("idx_security_lots_active_covering"),
        "聚合应使用 idx_security_lots_active_covering: {plan}"
    );
}

// ---------------------------------------------------------------------------
// Perf trace（数据库耗时日志）测试——ADR-0009
// ---------------------------------------------------------------------------

/// 时序级别纯函数边界：0、恰好阈值、略低于/略高于阈值、阈值 0。
#[test]
fn timing_level_boundaries() {
    use perf_trace::TimingClass;

    let threshold = Duration::from_millis(100);

    // 0 耗时：远低于阈值 → 正常（debug 明细）。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::ZERO),
        TimingClass::Normal
    );
    // 恰好等于阈值 → 正常（边界语义为严格大于才升级慢查询）。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(100)),
        TimingClass::Normal
    );
    // 略低于阈值 → 正常。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(99)),
        TimingClass::Normal
    );
    // 略高于阈值 → 慢查询（warn）。
    assert_eq!(
        perf_trace::timing_level(threshold, Duration::from_millis(101)),
        TimingClass::Slow
    );
    // threshold=0 且 duration>0 → 慢查询（0 阈值下非零耗时即慢查询）。
    assert_eq!(
        perf_trace::timing_level(Duration::ZERO, Duration::from_nanos(1)),
        TimingClass::Slow
    );
}

/// 接线回归：open_in_memory 默认注册 hook，执行 SELECT 1 能捕获到含 SQL 文本的事件。
/// 不限定具体级别——级别分类由 `timing_level` 纯函数测试覆盖；此处只验证 hook 接线生效
/// 且事件带 SQL 原文（占位符 SQL 记录于所有级别）。
#[test]
fn perf_trace_factory_emits_sql_event() {
    let conn = open_in_memory().unwrap();

    let events = capture_events(|| {
        conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .unwrap();
    });

    assert!(
        events.iter().any(|e| e
            .fields
            .iter()
            .any(|(k, v)| k == "sql" && v.contains("SELECT 1"))),
        "应捕获到含 SQL 文本的事件，实际捕获: {events:?}"
    );
}

/// 接线回归：threshold=0 时无需构造慢语句，正常语句也命中 warn 分支。
/// （SELECT 1 在内存库中耗时可能为 0ns，`0 > 0` 仍为 false；故用递归 CTE
/// 保证一条真实耗时的语句，验证阈值注入生效。）
#[test]
fn perf_trace_zero_threshold_emits_warn() {
    let conn = Connection::open_in_memory().unwrap();
    perf_trace::install_perf_trace(&conn, Duration::ZERO);

    let events = capture_events(|| {
        conn.query_row(
            "SELECT SUM(n) FROM (\
             WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM s WHERE n < 200000)\n             SELECT n FROM s)",
            [],
            |r| r.get::<_, i64>(0),
        )
        .unwrap();
    });

    assert!(
        events.iter().any(|e| e.level == Level::WARN),
        "threshold=0 时正常语句也应命中 warn 分支"
    );
}

/// 接线回归：在 `command` span 内执行 SQL，SQL 耗时事件应归因到该 span
/// （当前 span 名为 `command`）。这验证了 IPC 侧 `logged_invoke_handler`
/// 用 `info_span!(command, id_hint)` 包裹命令执行后，hook 事件自动继承调用方 span
/// （同步命令与 wrapper 同线程执行，归因成立）。
#[test]
fn perf_trace_sql_event_inherits_command_span() {
    let conn = open_in_memory().unwrap();

    let events = capture_events(|| {
        // 与 `logged_invoke_handler` 一致的命令 span 形状：name=command，含 command 字段。
        let span = tracing::info_span!("command", command = "list_accounts", id_hint = "");
        let _entered = span.enter();
        conn.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))
            .unwrap();
    });

    let sql_events: Vec<_> = events
        .iter()
        .filter(|e| e.fields.iter().any(|(k, _)| k == "sql"))
        .collect();
    assert!(
        !sql_events.is_empty(),
        "应捕获到 SQL 事件，实际捕获: {events:?}"
    );
    assert!(
        sql_events
            .iter()
            .all(|e| e.current_span.as_deref() == Some("command")),
        "SQL 事件应归因到 command span，实际: {sql_events:?}"
    );
}
