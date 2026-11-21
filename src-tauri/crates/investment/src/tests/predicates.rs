use rusqlite::{Connection, params};

use super::common::insert_instrument_with_market;
use crate::predicates::INVESTED_EXISTS;
use tauri_app_lib::test_support::{open, seed_account};

fn insert_lot(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    remaining: f64,
    lot_currency: &str,
) {
    let txn_id = format!("txn-{account_id}-{instrument_id}-{lot_currency}");
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',1000,'CNY',1000,?2,'2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![txn_id, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,100,0)",
        params![txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,?5,100,?6,'2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params![
            format!("lot-{txn_id}"),
            account_id,
            instrument_id,
            txn_id,
            remaining,
            lot_currency
        ],
    )
    .unwrap();
}

fn soft_delete_account(conn: &Connection, id: &str) {
    conn.execute("UPDATE accounts SET is_deleted=1 WHERE id=?1", params![id])
        .unwrap();
}

fn predicate_instrument_set(conn: &Connection) -> Vec<String> {
    let sql = format!("SELECT i.id FROM instruments i WHERE {INVESTED_EXISTS} ORDER BY i.id");
    let mut stmt = conn.prepare(&sql).unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn view_instrument_set(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT instrument_id FROM v_holdings ORDER BY instrument_id")
        .unwrap();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).unwrap();
    rows.map(|r| r.unwrap()).collect()
}

#[test]
fn invested_predicate_set_equals_v_holdings_instrument_set() {
    let conn = open();

    seed_account(&conn, "acc-live", "在持账户", "investment", "CNY", 0);
    insert_instrument_with_market(
        &conn,
        "inst-stock",
        "600001",
        "名称-600001",
        "CNY",
        "sh",
        "stock",
    );
    insert_lot(&conn, "acc-live", "inst-stock", 10.0, "CNY");

    insert_instrument_with_market(
        &conn,
        "inst-fund",
        "110011",
        "名称-110011",
        "CNY",
        "sh",
        "fund",
    );
    insert_lot(&conn, "acc-live", "inst-fund", 100.0, "CNY");
    insert_instrument_with_market(
        &conn,
        "inst-bond",
        "019547",
        "名称-019547",
        "CNY",
        "sh",
        "bond",
    );
    insert_lot(&conn, "acc-live", "inst-bond", 10.0, "CNY");

    insert_instrument_with_market(
        &conn,
        "inst-cleared",
        "600002",
        "名称-600002",
        "CNY",
        "sh",
        "stock",
    );
    insert_lot(&conn, "acc-live", "inst-cleared", 0.0, "CNY");

    seed_account(&conn, "acc-del", "已删账户", "investment", "CNY", 0);
    insert_instrument_with_market(
        &conn,
        "inst-softdel",
        "600003",
        "名称-600003",
        "CNY",
        "sh",
        "stock",
    );
    insert_lot(&conn, "acc-del", "inst-softdel", 10.0, "CNY");
    soft_delete_account(&conn, "acc-del");

    insert_instrument_with_market(
        &conn,
        "inst-multiccy",
        "600004",
        "名称-600004",
        "CNY",
        "sh",
        "stock",
    );
    insert_lot(&conn, "acc-live", "inst-multiccy", 5.0, "CNY");
    insert_lot(&conn, "acc-live", "inst-multiccy", 3.0, "USD");

    assert_eq!(
        predicate_instrument_set(&conn),
        vec![
            "inst-bond".to_string(),
            "inst-fund".to_string(),
            "inst-multiccy".to_string(),
            "inst-stock".to_string(),
        ],
        "谓词集合应恰为四只在持标的（含非股票），排除清仓与软删账户批次"
    );

    assert_eq!(
        predicate_instrument_set(&conn),
        view_instrument_set(&conn),
        "单点谓词与 v_holdings 视图的标的口径漂移"
    );

    let multiccy_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM v_holdings WHERE instrument_id='inst-multiccy'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(multiccy_rows, 2, "同标的不同币种 lot 应在视图出两行");
}
