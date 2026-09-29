//! 投资域简单写的事务原子性（issue #1867，ADR-0139 决策 2 的先行缺陷票）：
//! 汇率、现价、删标的、建档改名、手动报价五个编排入口的「业务写 + op 追加」
//! 必须同事务提交/回滚——任一步失败整体回滚，业务行与 op 行同生共死。
//!
//! 失败注入用纯测试侧手段（既有先例：`db::tx_scope` 行为单测与核心交易域
//! behavior 测试的触发器 RAISE(ABORT)）：`BEFORE INSERT ON sync_ops` 触发器
//! 挡下 op 落库，使编排体在**最后一步**失败。修复前（逐语句 autocommit）业务
//! 行已先行落库、op 缺席——「业务行残留」即红灯；修复后（嵌套感知事务自持）
//! 业务行与 op 行一同消失。断言对准数据终态（ADR-0087），不断言事务写法。

use rusqlite::{Connection, params};

use crate::crud::{
    create_exchange_rate, create_instrument, create_market_price, delete_instrument,
};
use crate::manual_price::record_manual_price;
use crate::{InstrumentInput, InstrumentType, ManualPriceInput, MarketPriceInput};
use ledger_currencies::ExchangeRateInput;
use tauri_app_lib::test_support::{FIXED_NOW, open, seed_instrument};

/// 注入「op 落库必失败」：sync_ops 的 BEFORE INSERT 触发器 RAISE(ABORT)，
/// 纯测试侧手段，不触及任何产品代码路径。
fn block_op_inserts(conn: &Connection) {
    conn.execute(
        "CREATE TRIGGER sync.block_sync_ops BEFORE INSERT ON sync.sync_ops \
         BEGIN SELECT RAISE(ABORT, '测试注入：op 写失败'); END",
        [],
    )
    .unwrap();
}

/// 断言错误来自注入触发器（失败确实发生在 op 落库这一步）。
fn assert_injected_failure(err: ledger_infra::error::AppError) {
    let text = err.to_string();
    assert!(
        text.contains("测试注入：op 写失败"),
        "错误应来自 op 落库失败注入，实际 {err:?}"
    );
}

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
    block_op_inserts(&conn);

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
    assert_injected_failure(err);

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
    block_op_inserts(&conn);

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
    assert_injected_failure(err);

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
    // 经核心创建入口建档（manual 来源，守卫允许删除）。
    let id = create_instrument(&conn, instrument_input("600000", "浦发银行")).unwrap();
    block_op_inserts(&conn);
    let ops_before = op_count(&conn);

    let err = delete_instrument(&conn, &id).unwrap_err();
    assert_injected_failure(err);

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
    block_op_inserts(&conn);

    let err = create_instrument(&conn, instrument_input("600000", "浦发银行")).unwrap_err();
    assert_injected_failure(err);

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
    // 首次建档（不注入，成功落库）。
    create_instrument(&conn, instrument_input("600000", "旧名")).unwrap();
    block_op_inserts(&conn);
    let ops_before = op_count(&conn);

    let err = create_instrument(&conn, instrument_input("600000", "新名")).unwrap_err();
    assert_injected_failure(err);

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
    block_op_inserts(&conn);

    let err = record_manual_price(
        &conn,
        &ManualPriceInput {
            instrument_id: "inst-1".into(),
            date: "2026-01-05".into(),
            price_cents: 3_910_000,
        },
    )
    .unwrap_err();
    assert_injected_failure(err);

    // 手动报价一条通道两落点（价格历史 + 现价缓存），两处都必须随事务回滚。
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

/// ADR-0139 决策 2 字面方向「业务写失败 ⇒ op 不存在」的非空洞形态：多落点管道
/// 中前一步业务写已成功、后一步业务写失败——已写的落点必须随整体回滚，op 不得
/// 产出。单落点入口的业务写失败发生在首条语句，op 缺席由 `?` 顺序平凡保证，
/// 不做空洞断言。注入点：market_prices 的 BEFORE INSERT 触发器挡下落点二。
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
