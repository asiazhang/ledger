use rusqlite::Connection;

use super::crud::seed_purchase_tx_with_rate;
use crate::domain::{create_item, list_items, update_item};
use crate::model::ItemInput;
use tauri_app_lib::test_support::{open, seed_exchange_rate, seed_fx_history_weeks};

const TRACE_RATE: f64 = 7.0;
const SERIES_RATE: f64 = 7.2;
const SERIES_RATE_NEW: f64 = 7.3;
const CURRENT_RATE: f64 = 7.5;

const BUY_DATE: &str = "2026-03-01";

fn seed_fork(conn: &Connection) {
    seed_exchange_rate(conn, "USD", "CNY", CURRENT_RATE);
    seed_fx_history_weeks(conn, "USD", "CNY", SERIES_RATE, &[BUY_DATE]);
}

fn seed_usd_purchase_tx(conn: &Connection, cost_cents: i64, fx_rate: Option<f64>) -> String {
    seed_purchase_tx_with_rate(conn, BUY_DATE, cost_cents, "USD", fx_rate)
}

fn item_input(name: &str, cost_cents: i64, link: Option<&str>) -> ItemInput {
    ItemInput {
        name: name.into(),
        purchase_date: BUY_DATE.into(),
        total_cost_cents: cost_cents,
        currency_code: "USD".into(),
        note: None,
        purchase_transaction_id: link.map(String::from),
    }
}

fn tx_native(conn: &Connection, tx_id: &str) -> i64 {
    conn.query_row(
        "SELECT amount_native_cents FROM transactions WHERE id=?1",
        [tx_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn item_native(conn: &Connection) -> i64 {
    list_items(conn).unwrap()[0].item.cost_native_cents
}

#[test]
fn create_item_native_inherits_source_trace_rate() {
    let conn = open();
    seed_fork(&conn);
    let tx = seed_usd_purchase_tx(&conn, 100_000, Some(TRACE_RATE));
    assert_eq!(
        tx_native(&conn, &tx),
        700_000,
        "夹具前提：交易行按显式留痕 7.0 折算"
    );

    create_item_for(&conn, &tx);
    assert_eq!(
        item_native(&conn),
        tx_native(&conn, &tx),
        "基础成本 native 与交易行精确一致"
    );
    assert_eq!(
        item_native(&conn),
        700_000,
        "来自留痕 7.0（≠ 序列 720000、≠ 当期 750000）"
    );
}

#[test]
fn create_item_falls_back_to_series_when_trace_missing() {
    let conn = open();
    seed_fork(&conn);
    let tx = seed_usd_purchase_tx(&conn, 100_000, None);
    assert_eq!(
        tx_native(&conn, &tx),
        720_000,
        "夹具前提：交易行按序列 7.2 折算"
    );
    conn.execute(
        "UPDATE transactions SET fx_rate_used=NULL, fx_rate_source=NULL WHERE id=?1",
        [&tx],
    )
    .unwrap();
    conn.execute(
        "UPDATE fx_rate_history SET rate=?1 WHERE base_code='USD' AND quote_code='CNY'",
        [SERIES_RATE_NEW],
    )
    .unwrap();

    create_item_for(&conn, &tx);
    assert_eq!(
        item_native(&conn),
        730_000,
        "回落序列重查现值（≠ 旧行 native 720000、≠ 当期 750000）"
    );
}

#[test]
fn update_extra_cost_reuses_source_trace_rate() {
    let conn = open();
    seed_fork(&conn);
    let tx = seed_usd_purchase_tx(&conn, 100_000, Some(TRACE_RATE));
    create_item_for(&conn, &tx);
    assert_eq!(item_native(&conn), 700_000);

    update_item(
        &conn,
        &item_id(&conn),
        item_input("进口手机", 120_000, None),
        &mut || {},
    )
    .unwrap();
    assert_eq!(
        item_native(&conn),
        840_000,
        "追加花费与基础同汇率 7.0（≠ 序列 864000、≠ 当期 900000）"
    );
}

#[test]
fn relink_retakes_new_trace_rate() {
    let conn = open();
    seed_fork(&conn);
    let tx_a = seed_usd_purchase_tx(&conn, 100_000, Some(TRACE_RATE));
    let tx_b = seed_purchase_tx_with_rate(&conn, "2026-04-06", 200_000, "USD", Some(7.1));
    create_item_for(&conn, &tx_a);
    assert_eq!(item_native(&conn), 700_000);

    update_item(
        &conn,
        &item_id(&conn),
        item_input("进口平板", 0, Some(&tx_b)),
        &mut || {},
    )
    .unwrap();
    let item = &list_items(&conn).unwrap()[0].item;
    assert_eq!(item.purchase_date, "2026-04-06", "换关带出新行日期");
    assert_eq!(item.total_cost_cents, 200_000, "换关带出新行成本");
    assert_eq!(
        item.cost_native_cents, 1_420_000,
        "随新行留痕 7.1 折算（≠ 序列 1440000、≠ 当期 1500000、≠ 旧留痕 1400000）"
    );
}

#[test]
fn create_item_base_currency_row_passes_no_explicit_rate() {
    let conn = open();
    seed_fork(&conn);
    let tx = seed_usd_purchase_tx(&conn, 100_000, Some(TRACE_RATE));
    tauri_app_lib::ledger_currencies::set_base_currency(&conn, "USD").unwrap();

    create_item_for(&conn, &tx);
    assert_eq!(
        item_native(&conn),
        100_000,
        "本位币行 1:1（不携显式汇率，不撞方向不符）"
    );
}

fn create_item_for(conn: &Connection, tx: &str) {
    create_item(conn, item_input("物品", 0, Some(tx)), &mut || {}).unwrap();
}

fn item_id(conn: &Connection) -> String {
    list_items(conn).unwrap()[0].item.id.clone()
}
