use chrono::{NaiveDate, TimeZone, Utc};
use rusqlite::Connection;

use crate::prices::{MarketPriceWrite, upsert_market_price};
use crate::staleness::{PRICE_STALE_AFTER_DAYS, beijing_date, instrument_price_staleness_on};
use ledger_transaction::create_transaction_internal;
use tauri_app_lib::test_support::{open, seed_account, seed_fx_history_weeks};

use super::common::{insert_fund_instrument, insert_instrument_with_market, make_buy_input};

fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 3, 9).expect("固定基准日合法")
}

fn seed_watermark(conn: &Connection, instrument_id: &str, priced_at: &str, nav_date: Option<&str>) {
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents: 10000,
            currency_code: "USD",
            priced_at,
            nav_date,
            source: Some("eastmoney"),
        },
    )
    .expect("现价缓存写入应成功");
}

fn hold(conn: &Connection, account_id: &str, instrument_id: &str) {
    seed_account(conn, account_id, "美股账户", "investment", "USD", 0);
    create_transaction_internal(
        conn,
        make_buy_input(account_id, instrument_id, 10.0, 10000, 0),
    )
    .expect("建仓应成功");
}

fn usd_rate(conn: &Connection) {
    seed_fx_history_weeks(conn, "USD", "CNY", 1.0, &["2026-01-10"]);
}

#[test]
fn quote_watermark_counts_only_beyond_threshold() {
    let conn = open();
    insert_instrument_with_market(&conn, "q-fresh", "F", "今日", "USD", "sh", "stock");
    insert_instrument_with_market(&conn, "q-edge", "E", "阈值上", "USD", "sh", "stock");
    insert_instrument_with_market(&conn, "q-old", "O", "过期", "USD", "sh", "stock");

    seed_watermark(&conn, "q-fresh", "2026-03-09T02:00:00Z", None);
    seed_watermark(&conn, "q-edge", "2026-03-02T02:00:00Z", None);
    seed_watermark(&conn, "q-old", "2026-03-01T02:00:00Z", None);

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.threshold_days, PRICE_STALE_AFTER_DAYS);
    assert_eq!(result.stale_count, 1, "只有超过阈值的行情水位计入过期");
}

#[test]
fn quote_watermark_uses_beijing_calendar_day() {
    let conn = open();
    insert_instrument_with_market(&conn, "q-before", "B", "北京前一日", "USD", "sh", "stock");
    insert_instrument_with_market(&conn, "q-after", "A", "北京次日", "USD", "sh", "stock");

    seed_watermark(&conn, "q-before", "2026-03-01T15:59:59Z", None);
    seed_watermark(&conn, "q-after", "2026-03-01T16:00:00Z", None);

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 1, "跨北京午夜的水位按北京日历日判定");
}

#[test]
fn fund_channel_reads_nav_date_watermark() {
    let conn = open();
    insert_fund_instrument(&conn, "f-old-nav", "110022", "旧净值");
    insert_fund_instrument(&conn, "f-fresh-nav", "000001", "新净值");

    seed_watermark(
        &conn,
        "f-old-nav",
        "2026-03-09T02:00:00Z",
        Some("2026-03-01"),
    );
    seed_watermark(
        &conn,
        "f-fresh-nav",
        "2026-03-05T02:00:00Z",
        Some("2026-03-09"),
    );

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 1, "净值通道以净值日期为水位");
}

#[test]
fn holding_without_watermark_counts_unheld_does_not() {
    let conn = open();
    insert_instrument_with_market(&conn, "q-held", "H", "持仓缺价", "USD", "sh", "stock");
    insert_instrument_with_market(&conn, "q-idle", "I", "建档未交易", "USD", "sh", "stock");
    usd_rate(&conn);
    hold(&conn, "acc-held", "q-held");

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 1, "只有持仓标的缺现价时计入");

    seed_watermark(&conn, "q-held", "2026-03-09T02:00:00Z", None);
    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 0, "补上当期水位后不再计入");
}

#[test]
fn manual_and_no_source_channels_are_out_of_scope() {
    let conn = open();
    insert_instrument_with_market(
        &conn,
        "m-bond",
        "019547",
        "自建债券",
        "USD",
        "unknown",
        "bond",
    );
    insert_instrument_with_market(
        &conn,
        "n-ghost",
        "GHOST",
        "无来源",
        "USD",
        "unknown",
        "stock",
    );
    usd_rate(&conn);
    hold(&conn, "acc-manual", "m-bond");
    hold(&conn, "acc-none", "n-ghost");

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(
        result.stale_count, 0,
        "无同步通道的行不属「去同步」能解决的面"
    );
}

#[test]
fn date_only_and_malformed_watermarks() {
    let conn = open();
    insert_instrument_with_market(
        &conn,
        "q-date-only",
        "D",
        "纯日期水位",
        "USD",
        "sh",
        "stock",
    );
    insert_instrument_with_market(&conn, "q-broken", "X", "非法水位", "USD", "sh", "stock");
    insert_instrument_with_market(
        &conn,
        "q-broken-idle",
        "Y",
        "非法水位未持仓",
        "USD",
        "sz",
        "stock",
    );

    seed_watermark(&conn, "q-date-only", "2026-03-01", None);
    seed_watermark(&conn, "q-broken", "不是日期", None);
    seed_watermark(&conn, "q-broken-idle", "不是日期", None);
    usd_rate(&conn);
    hold(&conn, "acc-broken", "q-broken");

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(
        result.stale_count, 2,
        "纯日期水位按北京日历日判定；非法水位按缺失、只对持仓行计数"
    );
}

#[test]
fn empty_book_reports_zero() {
    let conn = open();
    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 0);
    assert_eq!(result.threshold_days, PRICE_STALE_AFTER_DAYS);
}

#[test]
fn beijing_date_shifts_utc_by_plus_8h() {
    let at = |h: u32, m: u32, s: u32| {
        Utc.with_ymd_and_hms(2026, 3, 9, h, m, s)
            .single()
            .expect("固定时刻合法")
    };
    assert_eq!(
        beijing_date(at(15, 59, 59)),
        NaiveDate::from_ymd_opt(2026, 3, 9).expect("日期合法")
    );
    assert_eq!(
        beijing_date(at(16, 0, 0)),
        NaiveDate::from_ymd_opt(2026, 3, 10).expect("日期合法")
    );
}

fn mark_constant(conn: &Connection, instrument_id: &str) {
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = ?1",
        [instrument_id],
    )
    .unwrap();
}

fn hold_fund(conn: &Connection, account_id: &str, instrument_id: &str) {
    seed_account(conn, account_id, "货基户", "investment", "CNY", 0);
    let mut buy = make_buy_input(account_id, instrument_id, 100.0, 0, 0);
    buy.amount_cents = 10_000;
    buy.price_cents = None;
    create_transaction_internal(conn, buy).expect("建仓应成功");
}

#[test]
fn constant_price_instrument_is_exempt_even_when_invested_without_price() {
    let conn = open();
    insert_fund_instrument(&conn, "f-const", "000198", "天弘余额宝");
    mark_constant(&conn, "f-const");
    hold_fund(&conn, "acc-const", "f-const");

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 0, "恒定标的整体豁免过期计数");
}

#[test]
fn constant_price_instrument_is_exempt_even_with_stale_nav_date() {
    let conn = open();
    insert_fund_instrument(&conn, "f-const", "000198", "天弘余额宝");
    mark_constant(&conn, "f-const");
    seed_watermark(&conn, "f-const", "2026-01-30T02:00:00Z", Some("2026-01-30"));

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 0, "陈旧残留水位对恒定标的不构成过期");
}

#[test]
fn cleared_instrument_exits_count_only_beyond_exit_threshold() {
    let conn = open();
    insert_fund_instrument(&conn, "f-ancient", "002503", "清盘多年");
    insert_fund_instrument(&conn, "f-edge", "002414", "退出阈值上");
    insert_fund_instrument(&conn, "f-stale", "002435", "普通过期");

    seed_watermark(
        &conn,
        "f-ancient",
        "2026-03-09T02:00:00Z",
        Some("2020-01-01"),
    );
    seed_watermark(&conn, "f-edge", "2026-03-09T02:00:00Z", Some("2025-03-09"));
    seed_watermark(&conn, "f-stale", "2026-03-09T02:00:00Z", Some("2026-03-01"));

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(
        result.stale_count, 2,
        "超过退出阈值的已清仓行不计入，阈值上与普通过期仍计入"
    );
}

#[test]
fn invested_instrument_stays_counted_beyond_exit_threshold() {
    let conn = open();
    insert_fund_instrument(&conn, "f-held-dead", "003967", "持仓且清盘");
    usd_rate(&conn);
    hold_fund(&conn, "acc-held-dead", "f-held-dead");
    seed_watermark(
        &conn,
        "f-held-dead",
        "2026-03-09T02:00:00Z",
        Some("2020-01-01"),
    );

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(
        result.stale_count, 1,
        "持仓行不退出计数面，无论水位陈旧多久"
    );
}
