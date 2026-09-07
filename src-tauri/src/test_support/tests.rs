//! 工厂自身行为的直接断言（issue #751）：种子落行、组合种子三行齐备、对拍断言
//! 在一致世界通过、漂移世界变红。「回填 == 实时」对拍的场景级覆盖由两域迁移后的
//! balance_cache 测试承载（行为等价判据：全量既有测试保持绿，ADR-0084 决策 7）。

use rusqlite::params;

use super::{assert_balance_cache_matches_realtime, open, seed_account, seed_exchange_rate};
use super::{seed_fx_rate_history, seed_instrument, seed_investment_setup, seed_price_history};
use crate::accounts::balance::refresh_account_balances;

fn scalar(conn: &rusqlite::Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).unwrap()
}

/// `open()`：外键开启、迁移至最新、V004 默认种子在位（建库两行序的吸收语义）。
#[test]
fn open_initializes_memory_db() {
    let conn = open();
    let fk: i64 = conn
        .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fk, 1, "外键约束应随建库打开");
    assert!(
        scalar(&conn, "SELECT COUNT(*) FROM accounts") > 0,
        "迁移默认种子（V004）应在位"
    );
}

/// `seed_account` 全位置签名落行：列值、簿记戳（FIXED_NOW 内部发放）与初始余额。
#[test]
fn seed_account_lands_full_row() {
    let conn = open();
    seed_account(&conn, "acc-1", "钱包", "cash", "CNY", 6667);
    let (name, kind, currency, initial, created_at, device): (
        String,
        String,
        String,
        i64,
        String,
        String,
    ) = conn
        .query_row(
            "SELECT name,type,currency_code,initial_balance_cents,created_at,device_id \
             FROM accounts WHERE id='acc-1'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        (name.as_str(), kind.as_str(), currency.as_str()),
        ("钱包", "cash", "CNY")
    );
    assert_eq!(initial, 6667);
    assert_eq!(created_at, super::FIXED_NOW, "簿记戳应由工厂内部发放");
    assert_eq!(device, "test");
}

/// `seed_instrument` / `seed_price_history` / `seed_fx_rate_history`：列清单落行、
/// 域时刻（采样日）显式收参、来源固定 eastmoney。
#[test]
fn reference_data_seeds_land_rows() {
    let conn = open();
    seed_instrument(&conn, "inst-1", "600519.SH", "贵州茅台", "CNY", "sh");
    let (symbol, market): (String, String) = conn
        .query_row(
            "SELECT symbol,market FROM instruments WHERE id='inst-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((symbol.as_str(), market.as_str()), ("600519.SH", "sh"));

    seed_price_history(&conn, "ph-1", "inst-1", "2026-05-27", 170000, "CNY");
    let (price, date): (i64, String) = conn
        .query_row(
            "SELECT price_cents,trade_date FROM price_history WHERE id='ph-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((price, date.as_str()), (170000, "2026-05-27"));

    seed_fx_rate_history(&conn, "fxh-1", "HKD", "CNY", "2026-05-27", 0.92);
    let rate: f64 = conn
        .query_row(
            "SELECT rate FROM fx_rate_history WHERE id='fxh-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rate - 0.92).abs() < f64::EPSILON);
}

/// `seed_exchange_rate`：行 id 由货币对派生（表约束每货币对一行），汇率值落行。
#[test]
fn seed_exchange_rate_derives_pair_id() {
    let conn = open();
    let id = seed_exchange_rate(&conn, "USD", "CNY", 7.2);
    assert_eq!(id, "er-USD-CNY");
    let rate: f64 = conn
        .query_row(
            "SELECT rate FROM exchange_rates WHERE id='er-USD-CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rate - 7.2).abs() < f64::EPSILON);
}

/// 投资铺垫组合种子：账户（USD 投资户）+ 标的（USD 股票）+ 1:1 汇率三行齐备。
#[test]
fn investment_setup_seeds_account_instrument_and_rate() {
    let conn = open();
    let (acc, inst) = seed_investment_setup(&conn, "acc-inv", "inst-k");
    assert_eq!((acc.as_str(), inst.as_str()), ("acc-inv", "inst-k"));
    assert_eq!(
        scalar(
            &conn,
            "SELECT COUNT(*) FROM accounts WHERE id='acc-inv' AND type='investment'"
        ),
        1
    );
    assert_eq!(
        scalar(
            &conn,
            "SELECT COUNT(*) FROM instruments WHERE id='inst-k' AND currency_code='USD'"
        ),
        1
    );
    let rate: f64 = conn
        .query_row(
            "SELECT rate FROM exchange_rates WHERE base_code='USD' AND quote_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!((rate - 1.0).abs() < f64::EPSILON);
}

/// 对拍断言：种子直插账户经整体重算接缝回填后，「缓存 == 实时」在世界一致时通过。
#[test]
fn balance_cache_assertion_passes_on_consistent_world() {
    let conn = open();
    seed_account(&conn, "acc-a", "现金", "cash", "CNY", 6667);
    seed_investment_setup(&conn, "acc-inv", "inst-k");
    refresh_account_balances(&conn, &["acc-a", "acc-inv"]).unwrap();
    assert_balance_cache_matches_realtime(&conn);
}

/// 对拍断言：缓存值漂移（含缓存行缺失）即红，消息携带账户定位——不变式唯一
/// 维护点的守护语义（ADR-0067）。
#[test]
#[should_panic(expected = "缓存应等于实时计算")]
fn balance_cache_assertion_catches_drift() {
    let conn = open();
    seed_account(&conn, "acc-a", "现金", "cash", "CNY", 1000);
    refresh_account_balances(&conn, &["acc-a"]).unwrap();
    conn.execute(
        "UPDATE account_balance_cache SET balance_cents = balance_cents + 100 WHERE account_id=?1",
        params!["acc-a"],
    )
    .unwrap();
    assert_balance_cache_matches_realtime(&conn);
}
