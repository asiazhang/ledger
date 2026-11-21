use rusqlite::{Connection, params};

use crate::crud::{
    create_exchange_rate, create_instrument, create_market_price, delete_instrument,
};
use crate::manual_price::record_manual_price;
use crate::{InstrumentInput, InstrumentType, ManualPriceInput, MarketPriceInput};
use ledger_currencies::ExchangeRateInput;
use tauri_app_lib::test_support::{
    FIXED_NOW, assert_op_write_failure, block_op_writes, open, seed_instrument,
};

fn op_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM sync_ops", [], |r| r.get(0))
        .unwrap()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

fn instrument_input(symbol: &str, name: &str) -> InstrumentInput {
    InstrumentInput {
        symbol: symbol.into(),
        kind: InstrumentType::Other,
        name: Some(name.into()),
        currency_code: "CNY".into(),
        market: None,
    }
}

#[test]
fn create_exchange_rate_op_failure_rolls_back_rate_row() {
    let conn = open();
    block_op_writes(&conn);

    let err = create_exchange_rate(
        &conn,
        ExchangeRateInput {
            base_code: "USD".into(),
            quote_code: "CNY".into(),
            rate: 7.2,
            priced_at: FIXED_NOW.into(),
            source: None,
        },
    )
    .unwrap_err();
    assert_op_write_failure(&err);

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM exchange_rates"),
        0,
        "op 落库失败时汇率行必须随事务回滚，不得残留"
    );
    assert_eq!(op_count(&conn), 0);
}

#[test]
fn create_market_price_op_failure_rolls_back_price_row() {
    let conn = open();
    seed_instrument(&conn, "inst-1", "510300", "沪深300ETF", "CNY", "unknown");
    block_op_writes(&conn);

    let err = create_market_price(
        &conn,
        MarketPriceInput {
            instrument_id: "inst-1".into(),
            price_cents: 3_910_000,
            currency_code: "CNY".into(),
            priced_at: FIXED_NOW.into(),
            source: None,
        },
    )
    .unwrap_err();
    assert_op_write_failure(&err);

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM market_prices"),
        0,
        "op 落库失败时现价行必须随事务回滚，不得残留"
    );
    assert_eq!(op_count(&conn), 0);
}

#[test]
fn delete_instrument_op_failure_keeps_instrument_row() {
    let conn = open();
    let id = create_instrument(&conn, instrument_input("600000", "浦发银行")).unwrap();
    block_op_writes(&conn);
    let ops_before = op_count(&conn);

    let err = delete_instrument(&conn, &id).unwrap_err();
    assert_op_write_failure(&err);

    let kept: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM instruments WHERE id=?1",
            params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "op 落库失败时删除必须整体回滚，标的行必须仍在");
    assert_eq!(op_count(&conn), ops_before, "失败路径不得残留新 op");
}

#[test]
fn create_instrument_op_failure_rolls_back_new_row() {
    let conn = open();
    block_op_writes(&conn);

    let err = create_instrument(&conn, instrument_input("600000", "浦发银行")).unwrap_err();
    assert_op_write_failure(&err);

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM instruments"),
        0,
        "op 落库失败时新建标的行必须随事务回滚，不得残留"
    );
    assert_eq!(op_count(&conn), 0);
}

#[test]
fn create_instrument_rename_op_failure_keeps_old_name() {
    let conn = open();
    create_instrument(&conn, instrument_input("600000", "旧名")).unwrap();
    block_op_writes(&conn);
    let ops_before = op_count(&conn);

    let err = create_instrument(&conn, instrument_input("600000", "新名")).unwrap_err();
    assert_op_write_failure(&err);

    let name: String = conn
        .query_row(
            "SELECT name FROM instruments WHERE symbol='600000'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(name, "旧名", "op 落库失败时改名必须整体回滚，名称保持旧值");
    assert_eq!(op_count(&conn), ops_before, "失败路径不得残留新 op");
}

#[test]
fn record_manual_price_op_failure_rolls_back_both_landing_points() {
    let conn = open();
    seed_instrument(&conn, "inst-1", "510300", "沪深300ETF", "CNY", "unknown");
    block_op_writes(&conn);

    let err = record_manual_price(
        &conn,
        &ManualPriceInput {
            instrument_id: "inst-1".into(),
            date: "2026-01-05".into(),
            price_cents: 3_910_000,
        },
    )
    .unwrap_err();
    assert_op_write_failure(&err);

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM price_history"),
        0,
        "op 落库失败时价格历史落点必须随事务回滚"
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM market_prices"),
        0,
        "op 落库失败时现价缓存落点必须随事务回滚"
    );
    assert_eq!(op_count(&conn), 0);
}

#[test]
fn record_manual_price_business_write_failure_rolls_back_history_and_op() {
    let conn = open();
    seed_instrument(&conn, "inst-1", "510300", "沪深300ETF", "CNY", "unknown");
    conn.execute(
        "CREATE TRIGGER block_market_prices BEFORE INSERT ON market_prices \
         BEGIN SELECT RAISE(ABORT, '测试注入：现价落点写失败'); END",
        [],
    )
    .unwrap();

    let err = record_manual_price(
        &conn,
        &ManualPriceInput {
            instrument_id: "inst-1".into(),
            date: "2026-01-05".into(),
            price_cents: 3_910_000,
        },
    )
    .unwrap_err();
    let text = err.to_string();
    assert!(
        text.contains("测试注入：现价落点写失败"),
        "错误应来自业务写落点失败注入，实际 {err:?}"
    );

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM price_history"),
        0,
        "业务写中途失败时已写的价格历史落点必须随事务回滚"
    );
    assert_eq!(op_count(&conn), 0, "业务写失败不得产出 op");
}
