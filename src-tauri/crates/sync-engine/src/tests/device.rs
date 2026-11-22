use ledger_sync_protocol::device::device_id;
use tauri_app_lib::test_support;

#[test]
fn device_id_is_generated_once_and_persisted() {
    let conn = test_support::open();

    let id1 = device_id(&conn).unwrap();
    let id2 = device_id(&conn).unwrap();
    assert_eq!(id1, id2, "同一库内设备标识恒定");

    let (stored, clock): (String, i64) = conn
        .query_row("SELECT id, logical_clock FROM sync_device", [], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .unwrap();
    assert_eq!(stored, id1);
    assert_eq!(clock, 0);

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
