use super::*;
use rusqlite::{Connection, params};

use crate::models::{PnlFilter, TransactionInput};

fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name, kind, currency],
    ).unwrap();
}

fn insert_instrument(conn: &Connection, id: &str, symbol: &str, name: &str, currency: &str) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,'stock',?3,?4,'unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, name, currency],
    ).unwrap();
}

fn insert_instrument_with_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,'stock',?3,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, name, currency, market],
    ).unwrap();
}

fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        kind: "buy".into(),
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
    }
}

fn make_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        kind: "sell".into(),
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
    }
}

#[test]
fn list_instruments_pagination_and_search() {
    let conn = setup_db();
    for i in 0..5 {
        insert_instrument_with_market(
            &conn,
            &format!("inst-list-{i}"),
            &format!("SYM{i}"),
            &format!("名称{i}"),
            "USD",
            if i % 2 == 0 { "sh" } else { "sz" },
        );
    }

    // 默认第一页（page_size=50），返回全量
    let filter = InstrumentListFilter::default();
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 5);
    assert_eq!(result.items.len(), 5);
    assert_eq!(result.items[0].symbol, "SYM0");

    // 分页：每页 2 条，第 1 页
    let filter = InstrumentListFilter {
        search: None,
        market: None,
        page: None,
        page_size: Some(2),
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 5);
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].symbol, "SYM0");
    assert_eq!(result.items[1].symbol, "SYM1");

    // 分页：第 2 页
    let filter = InstrumentListFilter {
        search: None,
        market: None,
        page: Some(2),
        page_size: Some(2),
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.items.len(), 2);
    assert_eq!(result.items[0].symbol, "SYM2");

    // 搜索：代码大小写不敏感
    let filter = InstrumentListFilter {
        search: Some("sym1".into()),
        market: None,
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "SYM1");

    // 搜索：名称
    let filter = InstrumentListFilter {
        search: Some("名称3".into()),
        market: None,
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 1);
    assert_eq!(result.items[0].symbol, "SYM3");

    // 市场筛选
    let filter = InstrumentListFilter {
        search: None,
        market: Some("sh".into()),
        page: None,
        page_size: None,
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 3);
    assert!(result.items.iter().all(|i| i.market == "sh"));

    // 搜索 + 市场组合
    let filter = InstrumentListFilter {
        search: Some("SYM".into()),
        market: Some("sz".into()),
        page: Some(2),
        page_size: Some(1),
    };
    let result = super::crud::list_instruments(&conn, &filter).unwrap();
    assert_eq!(result.total, 2);
    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].symbol, "SYM3");
}

#[test]
fn buy_transaction_creates_lot() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-buy", "美股", "investment", "USD");
    insert_instrument(&conn, "inst-test-nvda", "NVDA", "NVIDIA", "USD");

    let input = make_buy_input("acc-test-buy", "inst-test-nvda", 10.0, 10000, 500);
    let txn_id = create_buy_transaction(&conn, input).unwrap();

    let (kind, amount_cents, currency_code): (String, i64, String) = conn
        .query_row(
            "SELECT kind, amount_cents, currency_code FROM transactions WHERE id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(kind, "buy");
    assert_eq!(amount_cents, 100500);
    assert_eq!(currency_code, "USD");

    let (action, quantity, price_cents, fee_cents): (String, f64, i64, i64) = conn
        .query_row(
            "SELECT action, quantity, price_cents, fee_cents FROM security_transactions WHERE transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(action, "buy");
    assert!((quantity - 10.0).abs() < 0.0001);
    assert_eq!(price_cents, 10000);
    assert_eq!(fee_cents, 500);

    let (remaining_quantity, cost_per_unit): (f64, i64) = conn
        .query_row(
            "SELECT remaining_quantity, cost_per_unit_cents FROM security_lots WHERE buy_transaction_id=?1",
            params![txn_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((remaining_quantity - 10.0).abs() < 0.0001);
    assert_eq!(cost_per_unit, 10050);

    let (holding_quantity, cost_basis): (f64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents FROM v_holdings WHERE id=?1",
            params!["acc-test-buy-inst-test-nvda-USD"],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!((holding_quantity - 10.0).abs() < 0.0001);
    assert_eq!(cost_basis, 100500);
}

#[test]
fn buy_transaction_requires_investment_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-cash", "现金", "cash", "CNY");
    insert_instrument(&conn, "inst-test-cny", "600519", "茅台", "CNY");

    let input = make_buy_input("acc-test-cash", "inst-test-cny", 1.0, 10000, 0);
    assert!(create_buy_transaction(&conn, input).is_err());
}

#[test]
fn sell_transaction_matches_multiple_lots_fifo() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-sell", "美股", "investment", "USD");
    insert_instrument(&conn, "inst-test-sell", "TSLA", "Tesla", "USD");

    let lot1_txn = create_buy_transaction(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 10.0, 10000, 0),
    )
    .unwrap();
    let lot2_txn = create_buy_transaction(
        &conn,
        make_buy_input("acc-test-sell", "inst-test-sell", 5.0, 12000, 0),
    )
    .unwrap();

    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-10T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot1_txn],
    )
    .unwrap();
    conn.execute(
        "UPDATE security_lots SET created_at='2026-01-15T00:00:00Z' WHERE buy_transaction_id=?1",
        params![lot2_txn],
    )
    .unwrap();

    let sell_txn = create_sell_transaction(
        &conn,
        make_sell_input("acc-test-sell", "inst-test-sell", 12.0, 15000, 600),
    )
    .unwrap();

    let (kind, amount_cents): (String, i64) = conn
        .query_row(
            "SELECT kind, amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(kind, "sell");
    assert_eq!(amount_cents, 179400);

    let rem1: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot1_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem1 - 0.0).abs() < 0.0001);
    let rem2: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![lot2_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem2 - 3.0).abs() < 0.0001);

    let rows: Vec<(f64, i64, i64, String)> = conn
        .prepare(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1 ORDER BY quantity DESC",
        )
        .unwrap()
        .query_map(params![sell_txn], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(rows.len(), 2);
    assert!((rows[0].0 - 10.0).abs() < 0.0001);
    assert_eq!(rows[0].1, 10000);
    assert_eq!(rows[0].2, 49500);
    assert_eq!(rows[0].3, "USD");
    assert!((rows[1].0 - 2.0).abs() < 0.0001);
    assert_eq!(rows[1].1, 12000);
    assert_eq!(rows[1].2, 5900);
    assert_eq!(rows[1].3, "USD");
}

#[test]
fn sell_transaction_rejects_oversell() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-oversell", "美股", "investment", "USD");
    insert_instrument(&conn, "inst-test-oversell", "MSFT", "Microsoft", "USD");

    create_buy_transaction(
        &conn,
        make_buy_input("acc-test-oversell", "inst-test-oversell", 5.0, 10000, 0),
    )
    .unwrap();

    let sell = make_sell_input("acc-test-oversell", "inst-test-oversell", 6.0, 12000, 0);
    assert!(create_sell_transaction(&conn, sell).is_err());
}

#[test]
fn sell_transaction_pnl_deducts_fee() {
    let conn = setup_db();
    insert_account(&conn, "acc-test-pnl", "美股", "investment", "USD");
    insert_instrument(&conn, "inst-test-pnl", "AAPL", "Apple", "USD");

    let buy_txn = create_buy_transaction(
        &conn,
        make_buy_input("acc-test-pnl", "inst-test-pnl", 10.0, 10000, 0),
    )
    .unwrap();
    let sell_txn = create_sell_transaction(
        &conn,
        make_sell_input("acc-test-pnl", "inst-test-pnl", 5.0, 12000, 200),
    )
    .unwrap();

    let rem: f64 = conn
        .query_row(
            "SELECT remaining_quantity FROM security_lots WHERE buy_transaction_id=?1",
            params![buy_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rem - 5.0).abs() < 0.0001);

    let (qty, cost, pnl, ccy): (f64, i64, i64, String) = conn
        .query_row(
            "SELECT quantity, cost_per_unit_cents, realized_pnl_cents, currency_code \
             FROM security_lot_sales WHERE sell_transaction_id=?1",
            params![sell_txn],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((qty - 5.0).abs() < 0.0001);
    assert_eq!(cost, 10000);
    assert_eq!(pnl, 9800);
    assert_eq!(ccy, "USD");

    let amount_cents: i64 = conn
        .query_row(
            "SELECT amount_cents FROM transactions WHERE id=?1",
            params![sell_txn],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(amount_cents, 5 * 12000 - 200);
}

#[test]
fn list_instruments_empty_initially() {
    let conn = setup_db();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn create_instrument_inserts_and_returns_id() {
    let conn = setup_db();
    let id = crate::db::new_uuid();
    let now = crate::db::now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,'stock',?3,?4,'unknown',?5,?6,?7,?8)",
        params![id, "NVDA", "NVIDIA Corporation", "USD", now, now, 1, "test"],
    ).unwrap();
    let (symbol, name, ccy): (String, Option<String>, String) = conn
        .query_row(
            "SELECT symbol, name, currency_code FROM instruments WHERE id=?1",
            params![id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(symbol, "NVDA");
    assert_eq!(name.as_deref(), Some("NVIDIA Corporation"));
    assert_eq!(ccy, "USD");
}

#[test]
fn create_instrument_is_idempotent() {
    let conn = setup_db();
    let id1 = crate::db::new_uuid();
    let id2 = crate::db::new_uuid();
    let now = crate::db::now_iso();
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'AAPL','stock',?2,'USD','unknown',?3,?4,?5,?6)",
        params![id1, "Apple Inc.", now, now, 1, "test"],
    ).unwrap();
    let result = conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
         VALUES (?1,'AAPL','stock',?2,'USD','unknown',?3,?4,?5,?6)",
        params![id2, "Apple Again", now, now, 1, "test"],
    );
    assert!(result.is_err());
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE symbol='AAPL' AND instrument_type='stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn list_holdings_returns_after_buy_and_market_price() {
    let conn = setup_db();
    insert_account(&conn, "acc-hold", "投资账户", "investment", "USD");
    insert_instrument(&conn, "inst-hold", "GOOGL", "Alphabet", "USD");

    let buy_input = make_buy_input("acc-hold", "inst-hold", 10.0, 15000, 1000);
    create_buy_transaction(&conn, buy_input).unwrap();

    let now = crate::db::now_iso();
    let price_id = crate::db::new_uuid();
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,source,created_at,updated_at,version,device_id) \
         VALUES (?1,?2,16000,'USD',?3,NULL,?4,?5,?6,?7)",
        params![price_id, "inst-hold", now, now, now, 1, "test"],
    ).unwrap();

    let (qty, cost_basis, market_value, unrealized_pnl): (f64, i64, i64, i64) = conn
        .query_row(
            "SELECT quantity, cost_basis_cents, market_value_cents, unrealized_pnl_cents \
             FROM v_holdings WHERE instrument_id=?1",
            params!["inst-hold"],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert!((qty - 10.0).abs() < 0.0001);
    assert_eq!(cost_basis, 151000);
    assert_eq!(market_value, 160000);
    assert_eq!(unrealized_pnl, 9000);
}

fn empty_filter() -> PnlFilter {
    PnlFilter {
        account_id: None,
        instrument_id: None,
    }
}

#[test]
fn realized_pnl_summary_empty_when_no_sales() {
    let conn = setup_db();
    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();
    assert_eq!(result.total_realized_pnl_cents, 0);
    assert!(result.by_year.is_empty());
    assert!(result.by_account.is_empty());
    assert!(result.by_instrument.is_empty());
    assert!(result.details.is_empty());
}

#[test]
fn realized_pnl_summary_aggregates_single_sale() {
    let conn = setup_db();
    insert_account(&conn, "acc-pnl", "美股账户", "investment", "USD");
    insert_instrument(&conn, "inst-pnl", "AAPL", "Apple", "USD");

    let _buy = create_buy_transaction(&conn, make_buy_input("acc-pnl", "inst-pnl", 10.0, 10000, 0))
        .unwrap();
    let _sell = create_sell_transaction(
        &conn,
        make_sell_input("acc-pnl", "inst-pnl", 5.0, 12000, 200),
    )
    .unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 9800);
    assert_eq!(result.by_year.len(), 1);
    assert_eq!(result.by_year[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.by_account[0].account_id, "acc-pnl");
    assert_eq!(result.by_account[0].realized_pnl_cents, 9800);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-pnl");
    assert_eq!(result.by_instrument[0].symbol, "AAPL");
    assert_eq!(result.by_instrument[0].realized_pnl_cents, 9800);
    assert_eq!(result.details.len(), 1);
    assert_eq!(result.details[0].instrument_symbol, "AAPL");
    assert_eq!(result.details[0].quantity, 5.0);
    assert_eq!(result.details[0].realized_pnl_cents, 9800);
}

#[test]
fn realized_pnl_summary_aggregates_multiple_accounts() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "账户A", "investment", "USD");
    insert_account(&conn, "acc-b", "账户B", "investment", "USD");
    insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

    create_buy_transaction(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 1000, 0)).unwrap();
    create_buy_transaction(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 2000, 0)).unwrap();
    create_sell_transaction(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 1500, 0)).unwrap();
    create_sell_transaction(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 2500, 0)).unwrap();

    let result = query_realized_pnl_summary(&conn, &empty_filter()).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 3000);
    assert_eq!(result.by_account.len(), 2);
    assert_eq!(result.by_account[0].account_id, "acc-a");
    assert_eq!(result.by_account[0].realized_pnl_cents, 2000);
    assert_eq!(result.by_account[1].account_id, "acc-b");
    assert_eq!(result.by_account[1].realized_pnl_cents, 1000);
    assert_eq!(result.details.len(), 2);
}

#[test]
fn realized_pnl_summary_filter_by_account() {
    let conn = setup_db();
    insert_account(&conn, "acc-a", "账户A", "investment", "USD");
    insert_account(&conn, "acc-b", "账户B", "investment", "USD");
    insert_instrument(&conn, "inst-xyz", "XYZ", "Test Corp", "USD");

    create_buy_transaction(&conn, make_buy_input("acc-a", "inst-xyz", 10.0, 1000, 0)).unwrap();
    create_buy_transaction(&conn, make_buy_input("acc-b", "inst-xyz", 5.0, 2000, 0)).unwrap();
    create_sell_transaction(&conn, make_sell_input("acc-a", "inst-xyz", 4.0, 1500, 0)).unwrap();
    create_sell_transaction(&conn, make_sell_input("acc-b", "inst-xyz", 2.0, 2500, 0)).unwrap();

    let filter = PnlFilter {
        account_id: Some("acc-a".into()),
        instrument_id: None,
    };
    let result = query_realized_pnl_summary(&conn, &filter).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 2000);
    assert_eq!(result.by_account.len(), 1);
    assert_eq!(result.details.len(), 1);
}

#[test]
fn realized_pnl_summary_filter_by_instrument() {
    let conn = setup_db();
    insert_account(&conn, "acc-pnl", "美股", "investment", "USD");
    insert_instrument(&conn, "inst-a", "AAPL", "Apple", "USD");
    insert_instrument(&conn, "inst-b", "GOOGL", "Alphabet", "USD");

    create_buy_transaction(&conn, make_buy_input("acc-pnl", "inst-a", 10.0, 1000, 0)).unwrap();
    create_buy_transaction(&conn, make_buy_input("acc-pnl", "inst-b", 5.0, 2000, 0)).unwrap();
    create_sell_transaction(&conn, make_sell_input("acc-pnl", "inst-a", 4.0, 1500, 0)).unwrap();
    create_sell_transaction(&conn, make_sell_input("acc-pnl", "inst-b", 2.0, 2500, 0)).unwrap();

    let filter = PnlFilter {
        account_id: None,
        instrument_id: Some("inst-a".into()),
    };
    let result = query_realized_pnl_summary(&conn, &filter).unwrap();

    assert_eq!(result.total_realized_pnl_cents, 2000);
    assert_eq!(result.by_instrument.len(), 1);
    assert_eq!(result.by_instrument[0].instrument_id, "inst-a");
    assert_eq!(result.details.len(), 1);
}
