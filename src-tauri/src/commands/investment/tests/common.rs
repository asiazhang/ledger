//! `investment` 命令测试共享脚手架（issue #257 纯移动自原 tests.rs 顶部，
//! 抽取仅限本测试模块内部，跨模块合并见 issue #250）。

use rusqlite::{Connection, params};

use crate::models::TransactionInput;
use crate::transaction::amount::TransactionKind;

pub(super) fn setup_db() -> Connection {
    let mut conn = crate::db::open_in_memory().unwrap();
    crate::db::init_db(&mut conn).unwrap();
    conn
}

pub(super) fn insert_account(conn: &Connection, id: &str, name: &str, kind: &str, currency: &str) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![id, name, kind, currency],
    ).unwrap();
}

pub(super) fn insert_instrument(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,'stock',?3,?4,'unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, name, currency],
    ).unwrap();
}

/// buy/sell 本位币折算走 Amount 接缝（issue #70）：测试库补 1:1 汇率，
/// 非默认币种（USD）账户的交易折算不报缺汇率。
pub(super) fn insert_rate_1_1(conn: &Connection, base: &str) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-1-1',?1,'CNY',1.0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![base],
    )
    .unwrap();
}

/// 补一条指定汇率（供非 1:1 折算断言用，如 7.2）。
pub(super) fn insert_rate(conn: &Connection, base: &str, quote: &str, rate: f64) {
    conn.execute(
        "INSERT INTO exchange_rates (id,base_code,quote_code,rate,priced_at,updated_at,version,device_id) \
         VALUES ('er-rate',?1,?2,?3,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![base, quote, rate],
    )
    .unwrap();
}

pub(super) fn insert_instrument_with_market(
    conn: &Connection,
    id: &str,
    symbol: &str,
    name: &str,
    currency: &str,
    market: &str,
    kind: &str,
) {
    conn.execute(
         "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id) \
          VALUES (?1,?2,?6,?3,?4,?5,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test')",
        params![id, symbol, name, currency, market, kind],
    ).unwrap();
}

pub(super) fn make_buy_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Buy,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-10".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        idempotency_key: None,
    }
}

pub(super) fn make_sell_input(
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    fee: i64,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind: TransactionKind::Sell,
        amount_cents: 0,
        currency_code: "USD".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: "2026-01-20".into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(fee),
        idempotency_key: None,
    }
}

/// 日期可指定的 buy/sell 输入（数量推算测试需要错开周采样日）。
pub(super) fn make_trade_input(
    kind: TransactionKind,
    account_id: &str,
    instrument_id: &str,
    qty: f64,
    price: i64,
    date: &str,
) -> TransactionInput {
    TransactionInput {
        merchant_name: None,
        kind,
        amount_cents: 0,
        currency_code: "CNY".into(),
        account_id: account_id.into(),
        to_account_id: None,
        category_id: None,
        merchant_id: None,
        refund_of_transaction_id: None,
        note: None,
        date: date.into(),
        instrument_id: Some(instrument_id.into()),
        quantity: Some(qty),
        price_cents: Some(price),
        fee_cents: Some(0),
        idempotency_key: None,
    }
}
