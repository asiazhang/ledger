use rusqlite::params;

use super::common::instrument_by_id;
use crate::constant_price::{
    ConstantPriceValue, ensure_constant_base_price, load_constant_prices, mark_constant_unit_price,
    week_monday, weekly_samples,
};
use crate::prices::{MarketPriceWrite, upsert_market_price};
use tauri_app_lib::test_support::open;

use super::common::insert_fund_instrument;

fn seed_constant(conn: &rusqlite::Connection, instrument_id: &str, cents: i64) {
    conn.execute(
        "UPDATE instruments SET constant_unit_price = ?2 WHERE id = ?1",
        params![instrument_id, cents],
    )
    .unwrap();
}

#[test]
fn mark_is_one_way_and_idempotent() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");

    assert!(mark_constant_unit_price(&conn, "inst-fund", 10_000).unwrap());
    assert!(!mark_constant_unit_price(&conn, "inst-fund", 10_000).unwrap());
    assert!(!mark_constant_unit_price(&conn, "inst-fund", 20_000).unwrap());
    let cents: Option<i64> = conn
        .query_row(
            "SELECT constant_unit_price FROM instruments WHERE id = 'inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cents, Some(10_000), "首标值不被后续确认覆盖");
}

#[test]
fn mark_clears_nav_date() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-fund",
            price_cents: 10_000,
            currency_code: "CNY",
            priced_at: "2026-01-30",
            nav_date: Some("2026-01-30"),
            source: Some("eastmoney"),
        },
    )
    .unwrap();

    assert!(mark_constant_unit_price(&conn, "inst-fund", 10_000).unwrap());
    let nav_date: Option<String> = conn
        .query_row(
            "SELECT nav_date FROM market_prices WHERE instrument_id = 'inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(nav_date, None, "打标后净值日期列为空（水位语义退出）");
}

#[test]
fn ensure_constant_base_price_inserts_once_and_never_touches_existing() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");

    assert!(
        ensure_constant_base_price(&conn, "inst-fund", 10_000, "CNY", "2026-01-30", "eastmoney")
            .unwrap()
    );
    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id = 'inst-fund'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (10_000, None), "建档常量价 = 1.0000、净值日期空");

    assert!(
        !ensure_constant_base_price(&conn, "inst-fund", 20_000, "CNY", "2026-02-02", "eastmoney")
            .unwrap()
    );
    let row: (i64, Option<String>) = conn
        .query_row(
            "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id = 'inst-fund'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(row, (10_000, None), "建档一条后不再更新");
}

#[test]
fn week_monday_matches_sqlite_week_start_generated_column() {
    let conn = open();
    let mut day = chrono::NaiveDate::from_ymd_opt(2025, 12, 28).unwrap();
    let end = chrono::NaiveDate::from_ymd_opt(2026, 1, 11).unwrap();
    while day <= end {
        let sql_key: String = conn
            .query_row(
                "SELECT date(?1, '-6 days', 'weekday 1')",
                [day.format("%Y-%m-%d").to_string()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            week_monday(day).format("%Y-%m-%d").to_string(),
            sql_key,
            "Rust 周键应与 SQLite week_start 生成列恒等"
        );
        day = day.succ_opt().unwrap();
    }
}

#[test]
fn weekly_samples_clamp_to_range_end_and_today() {
    let today = chrono::NaiveDate::from_ymd_opt(2026, 3, 9).unwrap();
    let from = chrono::NaiveDate::from_ymd_opt(2026, 2, 2).unwrap();
    let to = chrono::NaiveDate::from_ymd_opt(2026, 3, 1).unwrap();
    let samples = weekly_samples(from, to, today);
    assert_eq!(
        samples.len(),
        4,
        "四周各一条（02-02 至 03-01 恰为四个自然周）"
    );
    assert_eq!(samples[0].0, "2026-02-02");
    assert_eq!(samples[0].1, "2026-02-08", "完整周的采样日为周日");
    assert_eq!(samples[3].1, "2026-03-01");

    let samples = weekly_samples(today, today, today);
    assert_eq!(samples.len(), 1);
    assert_eq!(samples[0].1, "2026-03-09", "本周采样日不越过今天");

    let thursday = chrono::NaiveDate::from_ymd_opt(2026, 3, 5).unwrap();
    let samples = weekly_samples(from, thursday, today);
    assert_eq!(samples.last().unwrap().1, "2026-03-05");

    assert!(weekly_samples(to, from, today).is_empty());
}

#[test]
fn load_constant_prices_returns_marked_rows_with_creation_anchor() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-flat", "000001", "普通基金");
    insert_fund_instrument(&conn, "inst-const", "000198", "天弘余额宝");
    seed_constant(&conn, "inst-const", 10_000);

    let values = load_constant_prices(&conn).unwrap();
    assert_eq!(
        values,
        vec![ConstantPriceValue {
            instrument_id: "inst-const".to_string(),
            price_cents: 10_000,
            currency_code: "CNY".to_string(),
            anchor_date: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
        }],
        "只收标记行；锚点 = 建档日（FIXED_NOW 的日历日）"
    );
}

#[test]
fn marked_instrument_reads_as_constant_channel() {
    let conn = open();
    insert_fund_instrument(&conn, "inst-fund", "000198", "天弘余额宝");
    assert_eq!(
        instrument_by_id(&conn, "inst-fund").price_channel,
        crate::PriceChannel::FundNav,
        "未标记的 6 位基金仍是净值通道"
    );
    seed_constant(&conn, "inst-fund", 10_000);
    assert_eq!(
        instrument_by_id(&conn, "inst-fund").price_channel,
        crate::PriceChannel::Constant,
        "恒定单位价格在场即恒定价格通道（当且仅当该列有值）"
    );
}
