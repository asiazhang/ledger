//! 自建标的删除守卫（issue #292 / ADR-0036 决策 5）：仅来源为手动且无任何
//! buy/sell 流水引用（security_transactions 无行）的自建标的可物理删除；
//! 有流水引用拒删、同步来源标的拒删，均给中文错误信息。

use rusqlite::{Connection, params};

use crate::transaction::create_transaction_internal;

use super::super::*;
use super::common::*;

/// 插入指定来源的标的行（守卫两态的夹具：'eastmoney' 同步 / 'manual' 手动）。
fn insert_instrument_with_source(conn: &Connection, id: &str, symbol: &str, source: &str) {
    conn.execute(
        "INSERT INTO instruments (id,symbol,instrument_type,name,currency_code,market,created_at,updated_at,version,device_id,source) \
         VALUES (?1,?2,'other',?3,'CNY','unknown','2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',?4)",
        params![id, symbol, symbol, source],
    )
    .unwrap();
}

fn count_instruments(conn: &Connection, id: &str) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM instruments WHERE id=?1",
        params![id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 无引用自建标的：物理删除成功，行消失（现价缓存随 CASCADE 一并消失）。
#[test]
fn delete_manual_instrument_without_trades_succeeds() {
    let conn = setup_db();
    insert_instrument_with_source(&conn, "inst-manual-1", "稳稳地幸福", "manual");
    // 现价缓存行（手动报价通道可落）应随标的删除级联消失。
    conn.execute(
        "INSERT INTO market_prices (id,instrument_id,price_cents,currency_code,priced_at,created_at,updated_at,version,device_id) \
         VALUES ('mp-1','inst-manual-1',13180,'CNY','2026-08-28','2026-08-28T00:00:00Z','2026-08-28T00:00:00Z',1,'test')",
        [],
    )
    .unwrap();

    delete_instrument(&conn, "inst-manual-1").unwrap();

    assert_eq!(
        count_instruments(&conn, "inst-manual-1"),
        0,
        "自建标的应被物理删除"
    );
    let mp: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM market_prices WHERE instrument_id='inst-manual-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mp, 0, "现价缓存应随标的删除级联消失");
}

/// 有 buy/sell 流水引用的自建标的：拒删（中文错误），行保留。
#[test]
fn delete_instrument_with_trades_rejected() {
    let conn = setup_db();
    insert_account(&conn, "acc-inv", "证券户", "investment", "CNY");
    insert_instrument_with_source(&conn, "inst-manual-2", "HW-VR", "manual");
    create_transaction_internal(
        &conn,
        make_buy_input("acc-inv", "inst-manual-2", 10.0, 10000, 0),
    )
    .unwrap();

    let err = delete_instrument(&conn, "inst-manual-2").unwrap_err();
    assert!(
        err.to_string().contains("已有买卖流水"),
        "错误应说明有流水引用不可删：{err}"
    );
    assert_eq!(
        count_instruments(&conn, "inst-manual-2"),
        1,
        "有流水引用的标的应保留"
    );
}

/// 同步来源标的：即使无流水引用也拒删（填错由全量同步修正，ADR-0036 决策 5）。
#[test]
fn delete_sync_source_instrument_rejected() {
    let conn = setup_db();
    insert_instrument_with_source(&conn, "inst-em-1", "600519", "eastmoney");

    let err = delete_instrument(&conn, "inst-em-1").unwrap_err();
    assert!(
        err.to_string().contains("同步来源"),
        "错误应说明同步来源标的不支持删除：{err}"
    );
    assert_eq!(
        count_instruments(&conn, "inst-em-1"),
        1,
        "同步来源标的应保留"
    );
}

/// 不存在的标的 id：码化 NotFound（`instrument.not-found`），中文错误。
#[test]
fn delete_missing_instrument_not_found() {
    let conn = setup_db();
    let err = delete_instrument(&conn, "不存在的id").unwrap_err();
    assert!(
        err.to_string().contains("标的 不存在的id 不存在"),
        "删除不存在的标的应报 NotFound：{err}"
    );
}
