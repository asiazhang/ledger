//! DeviceId（设备标识）：首用生成并持久化；换库（重装/换机）生成新标识是
//! 合法路径（ADR-0091）。被测对象自 #1089 起住协议 crate
//!（`ledger_sync_protocol::device`）；测试不随迁的原因见协议 crate Cargo.toml
//! 注释——compile_fail 负向用例优先于 dev-dependency 环（建库两行序只能经根包
//! test_support 工厂，ADR-0084 规则 1，工厂不住协议 crate 可达位置）。

use crate::test_support;
use ledger_sync_protocol::device::device_id;

#[test]
fn device_id_is_generated_once_and_persisted() {
    let conn = test_support::open();

    let id1 = device_id(&conn).unwrap();
    let id2 = device_id(&conn).unwrap();
    assert_eq!(id1, id2, "同一库内设备标识恒定");

    // 持久化：sync_device 单行落库，逻辑时钟从 0 起。
    let (stored, clock): (String, i64) = conn
        .query_row("SELECT id, logical_clock FROM sync_device", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(stored, id1);
    assert_eq!(clock, 0);

    // 标识形态：UUID（v7 时间有序，与全库主键同形态）。
    assert!(uuid::Uuid::parse_str(&id1).is_ok());
}

#[test]
fn fresh_database_gets_fresh_device_id() {
    let conn_a = test_support::open();
    let conn_b = test_support::open();

    let id_a = device_id(&conn_a).unwrap();
    let id_b = device_id(&conn_b).unwrap();
    assert_ne!(id_a, id_b, "新库（重装/换机）生成新设备标识");
}

#[test]
fn device_id_generation_rolls_back_with_transaction() {
    // 首用生成发生在调用方的写事务内：事务回滚则连同回滚，后续首用重新生成
    // ——不留半途状态。
    let conn = test_support::open();
    conn.execute("BEGIN", []).unwrap();
    let id_in_tx = device_id(&conn).unwrap();
    conn.execute("ROLLBACK", []).unwrap();

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM sync_device", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "事务回滚则未持久化");
    assert_ne!(device_id(&conn).unwrap(), id_in_tx, "回滚后重新生成新标识");
}
