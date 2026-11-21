use rusqlite::Connection;

use crate::holdings::{FIRST_POSITION_DATE, first_position_date};
use tauri_app_lib::test_support::open;

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
