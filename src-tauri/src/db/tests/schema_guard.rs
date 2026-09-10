//! Schema 漂移守卫机制测试（issue #971/#992 / ADR-0100）：守卫本体（内存参照库
//! 方向性 diff）的单测——漂移检出、零误报、遗留对象容忍。接线归 e2e
//! （startup_failure.feature「缺列漂移的明文库启动进入失败状态」，删除守卫
//! 接线调用即红，#963 惯例 / ADR-0087 断言强度：断言对准可观察错误结果）。
//!
//! 夹具以统一测试工厂建库（ADR-0084）：工厂本身就是迁移两行序，守卫随
//! 初始化照常运行——健康库若被误报，本文件所有测试都不会走到断言。

use crate::db::schema_guard::{BOOT_SCHEMA_DRIFT, verify_schema};

/// 健康库零误报：从零迁移（工厂两行序）后的库应通过守卫校验（验收判据 2）。
#[test]
fn healthy_db_passes_schema_guard() {
    let conn = crate::test_support::open();
    assert!(verify_schema(&conn).is_ok(), "健康库不应报漂移");
}

/// 漂移检出（列方向，验收判据 1）：参照有而实际缺的列 = 漂移。#971 实测事故
/// 形态：sync_parked_ops 缺 V021 声明的 park_params，user_version 不变、
/// 迁移裁决不再触发，守卫是唯一防线。
#[test]
fn missing_column_is_detected_as_drift() {
    let conn = crate::test_support::open();
    conn.execute("ALTER TABLE sync_parked_ops DROP COLUMN park_params", [])
        .unwrap();
    let err = verify_schema(&conn).expect_err("缺列库应报漂移");
    assert_eq!(
        err.code(),
        Some(BOOT_SCHEMA_DRIFT),
        "错误码应精确为 boot.schema-drift，实际: {err}"
    );
}

/// 漂移检出（对象方向，验收判据 1）：参照有而实际缺的对象 = 漂移（索引与
/// 视图形态；表同走 sqlite_master 清单比对，机制相同）。
#[test]
fn missing_objects_are_detected_as_drift() {
    let conn = crate::test_support::open();
    conn.execute("DROP INDEX idx_transactions_note_search", [])
        .unwrap();
    conn.execute("DROP VIEW v_holdings", []).unwrap();
    let err = verify_schema(&conn).expect_err("缺对象库应报漂移");
    assert_eq!(
        err.code(),
        Some(BOOT_SCHEMA_DRIFT),
        "错误码应精确为 boot.schema-drift，实际: {err}"
    );
}

/// 方向性容忍（验收判据 3）：实际多出的遗留对象与列不算漂移——跑过 V005 的
/// 老库合法残留搜索索引对象、`sqlite_sequence` 等内部对象（ADR-0027 语义 /
/// ADR-0100 决策 2），非方向性比对必然误报。
#[test]
fn legacy_extra_objects_are_tolerated() {
    let conn = crate::test_support::open();
    conn.execute("CREATE TABLE notes_search (id TEXT, note TEXT)", [])
        .unwrap();
    conn.execute(
        "CREATE INDEX idx_notes_search_text ON notes_search (note)",
        [],
    )
    .unwrap();
    conn.execute("ALTER TABLE merchants ADD COLUMN legacy_col TEXT", [])
        .unwrap();
    assert!(verify_schema(&conn).is_ok(), "遗留多出对象不应误报漂移");
}
