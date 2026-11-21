use rusqlite::params;

use tauri_app_lib::test_support::{
    seed_account, seed_exchange_rate, seed_instrument, seed_market_price,
};

#[test]
fn cross_currency_holding_pnl() {
    let conn = tauri_app_lib::test_support::open();

    let account_id = "acc-test-cny-inv";
    let instrument_id = "inst-test-nvda";
    seed_account(&conn, account_id, "美股CNY", "investment", "CNY", 0);
    seed_instrument(&conn, instrument_id, "NVDA", "NVIDIA", "USD", "unknown");
    seed_exchange_rate(&conn, "USD", "CNY", 7.2);

    let buy_txn_id = "txn-buy-cross";
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',100000,'USD',720000,?2,NULL,NULL,NULL,'买 NVDA','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
        params![buy_txn_id, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,1000000,0)",
        params![buy_txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,1000000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params!["lot-cross", account_id, instrument_id, buy_txn_id],
    )
    .unwrap();
    seed_market_price(&conn, instrument_id, 1_200_000, "USD");

    let (cost_basis, cost_ccy, market_value, unrealized_pnl): (i64, String, i64, i64) = conn
        .query_row(
            "SELECT cost_basis_cents, cost_currency_code, market_value_cents, unrealized_pnl_cents \
             FROM v_holdings WHERE id=?1",
            params![format!("{account_id}-{instrument_id}-USD")],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(cost_basis, 100000);
    assert_eq!(cost_ccy, "USD");
    assert_eq!(market_value, 864000);
    assert_eq!(unrealized_pnl, 144000);
}

#[test]
fn holding_reverse_rate_fallback() {
    let conn = tauri_app_lib::test_support::open();

    let account_id = "acc-test-rev";
    let instrument_id = "inst-test-rev";
    seed_account(&conn, account_id, "反向", "investment", "CNY", 0);
    seed_instrument(
        &conn,
        instrument_id,
        "REV",
        "reverse test",
        "USD",
        "unknown",
    );
    seed_exchange_rate(&conn, "CNY", "USD", 0.125);

    let buy_txn_id = "txn-buy-rev";
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',80000,'USD',640000,?2,NULL,NULL,NULL,'buy','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
        params![buy_txn_id, account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,800000,0)",
        params![buy_txn_id, instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,10,10,800000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params!["lot-rev", account_id, instrument_id, buy_txn_id],
    )
    .unwrap();
    seed_market_price(&conn, instrument_id, 960_000, "USD");

    let (market_value, unrealized_pnl): (i64, i64) = conn
        .query_row(
            "SELECT market_value_cents, unrealized_pnl_cents FROM v_holdings WHERE id=?1",
            params![format!("{account_id}-{instrument_id}-USD")],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(market_value, 768000);
    assert_eq!(unrealized_pnl, 128000);
}

#[test]
fn holding_id_unique_across_currencies() {
    let conn = tauri_app_lib::test_support::open();

    let account_id = "acc-test-id-uniq";
    let instrument_id = "inst-test-multi";
    seed_account(&conn, account_id, "多币种", "investment", "USD", 0);
    seed_instrument(
        &conn,
        instrument_id,
        "MULTI",
        "多币种标的",
        "USD",
        "unknown",
    );

    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',10000,'USD',10000,?2,NULL,NULL,NULL,'usd lot','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
        params!["txn-usd", account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',5,2000,0)",
        params!["txn-usd", instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,5,5,2000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
        params!["lot-usd", account_id, instrument_id, "txn-usd"],
    )
    .unwrap();

    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,'buy',9000,'EUR',9000,?2,NULL,NULL,NULL,'eur lot','2026-01-11','2026-01-11T00:00:00Z','2026-01-11T00:00:00Z',1,'test',0)",
        params!["txn-eur", account_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',3,3000,0)",
        params!["txn-eur", instrument_id],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,?3,?4,3,3,3000,'EUR','2026-01-11T00:00:00Z','2026-01-11T00:00:00Z',1,'test')",
        params!["lot-eur", account_id, instrument_id, "txn-eur"],
    )
    .unwrap();

    let rows: Vec<(String, String)> = conn
        .prepare("SELECT id, cost_currency_code FROM v_holdings ORDER BY cost_currency_code")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 2, "应按 lot 币种拆成两行");
    assert_eq!(rows[0].0, format!("{account_id}-{instrument_id}-EUR"));
    assert_eq!(rows[0].1, "EUR");
    assert_eq!(rows[1].0, format!("{account_id}-{instrument_id}-USD"));
    assert_eq!(rows[1].1, "USD");

    let dupes: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM (SELECT id FROM v_holdings GROUP BY id HAVING COUNT(*) > 1)",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(dupes, 0, "id 不应重复");
}

#[test]
fn holding_excludes_soft_deleted_account() {
    let conn = tauri_app_lib::test_support::open();

    let active_acc = "acc-soft-active";
    let deleted_acc = "acc-soft-deleted";
    let instrument_id = "inst-soft-test";
    for acc in [active_acc, deleted_acc] {
        seed_account(&conn, acc, acc, "investment", "USD", 0);
    }
    seed_instrument(
        &conn,
        instrument_id,
        "SOFT",
        "soft-delete test",
        "USD",
        "unknown",
    );

    for (acc, txn, lot) in [
        (active_acc, "txn-soft-a", "lot-soft-a"),
        (deleted_acc, "txn-soft-d", "lot-soft-d"),
    ] {
        conn.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,category_id,refund_of_transaction_id,note,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'buy',10000,'USD',10000,?2,NULL,NULL,NULL,'buy','2026-01-10','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test',0)",
            params![txn, acc],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
             VALUES (?1,?2,'buy',1,10000,0)",
            params![txn, instrument_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO security_lots (id,account_id,instrument_id,buy_transaction_id,initial_quantity,remaining_quantity,cost_per_unit_cents,currency_code,created_at,updated_at,version,device_id) \
             VALUES (?1,?2,?3,?4,1,1,10000,'USD','2026-01-10T00:00:00Z','2026-01-10T00:00:00Z',1,'test')",
            params![lot, acc, instrument_id, txn],
        )
        .unwrap();
    }

    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at='2026-02-01T00:00:00Z', version=version+1 WHERE id=?1",
        params![deleted_acc],
    )
    .unwrap();

    let lot_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM security_lots WHERE account_id=?1 AND remaining_quantity > 0",
            params![deleted_acc],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(lot_count, 1, "已删账户的 lot 应仍存在");

    let account_ids: Vec<String> = conn
        .prepare("SELECT account_id FROM v_holdings ORDER BY account_id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(
        account_ids,
        vec![active_acc.to_string()],
        "软删除账户的持仓不应出现在视图"
    );
}

#[test]
fn transaction_currency_conversion() {
    let conn = tauri_app_lib::test_support::open();

    let account_id = "acc-test-cny";
    seed_account(&conn, account_id, "现金", "cash", "CNY", 0);
    seed_exchange_rate(&conn, "USD", "CNY", 7.2);

    let native =
        ledger_transaction::amount::convert_to_native_current(&conn, 10000, "USD").unwrap();
    assert_eq!(native, 72000);

    let native =
        ledger_transaction::amount::convert_to_native_current(&conn, 10000, "CNY").unwrap();
    assert_eq!(native, 10000);
}
