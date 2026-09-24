//! 「首笔持仓流水日」查询计划守门（issue #1804）：`FIRST_POSITION_DATE`
//! 两臂必须各走 `security_transactions` 索引 seek，且索引钉定必须在场——
//! 不得退回「`transactions` 全扫 + 按 PK 回表」的逐标的重执行形态。
//!
//! 背景：生产日志实测该子查询逐标的（244 只）重执行吃掉收集查询 90% 耗时
//! （1737ms 现场 / 热缓存 131ms 片段），根因是 `to_instrument_id` 臂无索引。
//! V033 补索引 + 常量两臂拆分 `INDEXED BY` 钉定后，计划由 SQL 文本确定
//!（不依赖数据量与成本平局，V030 查询侧钉定先例），实测整条收集查询 14.7ms。
//!
//! 删除即红（ADR-0087 断言强度：对准「两臂不再 PK 回表、索引缺失即 prepare
//! 报错」这一用户可观察回归，不对准耗时数值——耗时在 CI 上不可判定，
//! 归 ledger-perf 门禁口径）：
//! - 删掉 V033 迁移登记或迁移文件 → 两臂计划断言与钉定缺失报错断言同时变红；
//! - 从常量里删掉任一臂的 `INDEXED BY` 钉定 → 该臂计划落回 PK 回表 → 计划断言变红。
//!
//! 次序判据不进断言面：`transactions` 与 `security_transactions` 的循环内外次序
//! 随数据量摆动、空库成本平局不可判定，小库上恒误报（ADR-0087 要求断言存在
//! 可失败的实现改动，无法构造者删除）。

use rusqlite::Connection;

use crate::holdings::{FIRST_POSITION_DATE, first_position_date};
use tauri_app_lib::test_support::open;

/// 计划明细列（EXPLAIN QUERY PLAN 第 4 列）拼成的多行文本。
fn plan_of(conn: &Connection, sql: &str) -> String {
    let mut stmt = conn
        .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
        .expect("EXPLAIN 应可准备");
    stmt.query_map([], |row| row.get::<_, String>(3))
        .expect("EXPLAIN 应可执行")
        .collect::<Result<Vec<_>, _>>()
        .expect("计划行应可读出")
        .join("\n")
}

/// 两臂各按 `INDEXED BY` 钉定走索引 seek、不退回 PK 回表（外层以 `i` 作别名，
/// 遵守别名契约）。钉定后本断言由 SQL 文本确定：删掉任一臂的 `INDEXED BY` 或
/// 删掉 V033 → 该臂计划落回 `sqlite_autoindex_security_transactions_1` → 红。
///（`transactions` 与 `security_transactions` 的循环内外次序随数据量摆动、空库
/// 成本平局可接受，不进判据——实测 37MB 库为 st 外层 + 各臂索引 seek，14.7ms。）
#[test]
fn first_position_date_arms_seek_pinned_indexes() {
    let conn = open();
    let plan = plan_of(
        &conn,
        &format!("SELECT {FIRST_POSITION_DATE} FROM instruments i"),
    );
    assert!(
        plan.contains("idx_security_transactions_instrument"),
        "转出臂应按钉定走 instrument_id 索引 seek，实际计划:\n{plan}"
    );
    assert!(
        plan.contains("idx_security_transactions_to_instrument"),
        "转入臂应按钉定走 to_instrument_id 索引 seek（V033），实际计划:\n{plan}"
    );
    assert!(
        !plan.contains("sqlite_autoindex_security_transactions_1"),
        "不得退回按 PK 回表再判腿的逐标的重执行形态，实际计划:\n{plan}",
    );
}

/// 索引缺失即 prepare 报错（钉定的防删守卫，V030 先例）：删掉 V033 后本用例
/// 与首笔腿既有用例（`tests::holdings_as_of`）同时变红——常量失去 `INDEXED BY`
/// 钉定时本用例红，守住「计划由 SQL 确定」这件事本身。
#[test]
fn first_position_date_fails_loudly_without_pinned_index() {
    let conn = open();
    conn.execute("DROP INDEX idx_security_transactions_to_instrument", [])
        .expect("测试库应有 V033 索引可删");
    let err = first_position_date(&conn, "inst-any").expect_err("索引缺失时应 prepare 报错");
    assert!(
        err.to_string()
            .contains("idx_security_transactions_to_instrument"),
        "错误应指明缺失的钉定索引，实际: {err}"
    );
}
