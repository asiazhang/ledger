//! `item::domain` 写路径折算锚点测试（#1693 / ADR-0014 2026-09-22 修订）：
//! 创建/修改的本位币成本按交易日口径折算、**继承溯源交易行留痕汇率**。
//!
//! **三方分叉夹具**：交易行留痕 7.0（写入契约显式值）≠ 序列值 7.2/7.3
//! （`fx_rate_history` 交易周点）≠ 当期值 7.5（`exchange_rates`）——三个取数源
//! 对同一金额折出三个不同本位币数，折算入口或取数源选错任一即被断言逮住
//! （改回当期入口 `convert_to_native_current` → 当期值命中 → 本文件即红，
//! 见 issue #1693 验收「删除即变红」）。

use rusqlite::Connection;

use super::crud::seed_purchase_tx_with_rate;
use crate::domain::{create_item, list_items, update_item};
use crate::model::ItemInput;
use tauri_app_lib::test_support::{open, seed_exchange_rate, seed_fx_history_weeks};

/// 三方分叉：交易行留痕（写入契约显式汇率，`fx_rate_source=explicit`）。
const TRACE_RATE: f64 = 7.0;
/// 三方分叉：序列值（`fx_rate_history` 交易周点）。
const SERIES_RATE: f64 = 7.2;
/// 序列推新值（无留痕回落序列重查的分叉用，与旧行 native 分叉）。
const SERIES_RATE_NEW: f64 = 7.3;
/// 三方分叉：当期值（`exchange_rates`，读路径表——写路径误读即命中它）。
const CURRENT_RATE: f64 = 7.5;

/// 购买交易日期（固定域时刻，覆盖该日期所属周的序列点）。
const BUY_DATE: &str = "2026-03-01";

/// 三方分叉夹具：当期行 7.5 + 交易周序列点 7.2（USD→CNY）。
fn seed_fork(conn: &Connection) {
    seed_exchange_rate(conn, "USD", "CNY", CURRENT_RATE);
    seed_fx_history_weeks(conn, "USD", "CNY", SERIES_RATE, &[BUY_DATE]);
}

/// 指定显式汇率的 USD 购买交易（脚手架账户由 `seed_purchase_tx_with_rate`
/// 幂等补齐，币种随首笔交易），返回交易 id。
fn seed_usd_purchase_tx(conn: &Connection, cost_cents: i64, fx_rate: Option<f64>) -> String {
    seed_purchase_tx_with_rate(conn, BUY_DATE, cost_cents, "USD", fx_rate)
}

/// 创建/修改入参（购买日期/币种取夹具常量；`link` = 关联购买交易，None = 不换关
/// 维持既有溯源——创建与换关时日期/成本/币种由交易值覆盖带出，此处占位即可）。
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

/// 读交易行 native（锚点①「与交易行精确一致」的对拍值）。
fn tx_native(conn: &Connection, tx_id: &str) -> i64 {
    conn.query_row(
        "SELECT amount_native_cents FROM transactions WHERE id=?1",
        [tx_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 读单件物品本位币成本（每测仅一件）。
fn item_native(conn: &Connection) -> i64 {
    list_items(conn).unwrap()[0].item.cost_native_cents
}

/// 锚点①：创建继承溯源交易行留痕——三方分叉下 item native 来自留痕 7.0、
/// 与交易行精确一致；改回当期入口（7.5）或重查序列（7.2）即红。
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

/// 锚点②：无留痕（迁移前行）回落序列重查——留痕列置空后按序列**现值**折算：
/// 不抄交易行旧 native（720000）、不读当期表（750000）。
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
    // 迁移前行模拟（V029 加列前落库，留痕两列为空）+ 序列推出新值——
    // 与交易行旧 native（720000）、当期（750000）三方分叉，只有「回落序列重查」
    // 命中 730000。无公开入口可清留痕（留痕随行终身），此处为库内状态直置例外。
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

/// 锚点③：追加花费后重折算仍用留痕——修改总成本（维持关联、不换关）按交易行
/// 留痕 7.0 折算，数值可复现、不随编辑时间漂移；改回当期入口（7.5）即红。
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

/// 锚点④：换关随新交易行重取——关联新交易后按新行留痕 7.1 折算带出的
/// 日期/成本/币种；改回当期入口（7.5）或沿用旧留痕（7.0）即红。
#[test]
fn relink_retakes_new_trace_rate() {
    let conn = open();
    seed_fork(&conn);
    let tx_a = seed_usd_purchase_tx(&conn, 100_000, Some(TRACE_RATE));
    // 新关联行走不同日期与显式留痕（7.1 ≠ 旧行 7.0、序列 7.2、当期 7.5）。
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

/// 锚点⑤：本位币行不传显式汇率——基准币变更为交易币种后（账本级设置可变更，
/// DefaultCurrency），写入时跨币种携率的行对新基准不再有折算方向：入口自持
/// 1:1；透传留痕即撞 `fx.explicit-rate-direction-mismatch`、物品无法保存。
#[test]
fn create_item_base_currency_row_passes_no_explicit_rate() {
    let conn = open();
    seed_fork(&conn);
    let tx = seed_usd_purchase_tx(&conn, 100_000, Some(TRACE_RATE));
    // 基准币 CNY → USD：交易行写入时的 USD→CNY 留痕 7.0 对新基准方向失效。
    tauri_app_lib::ledger_currencies::set_base_currency(&conn, "USD").unwrap();

    create_item_for(&conn, &tx);
    assert_eq!(
        item_native(&conn),
        100_000,
        "本位币行 1:1（不携显式汇率，不撞方向不符）"
    );
}

/// 创建一件关联 `tx` 的物品（日期/成本/币种由交易带出）。
fn create_item_for(conn: &Connection, tx: &str) {
    create_item(conn, item_input("物品", 0, Some(tx)), &mut || {}).unwrap();
}

/// 当前唯一物品的 id（每测仅一件）。
fn item_id(conn: &Connection) -> String {
    list_items(conn).unwrap()[0].item.id.clone()
}
