//! 价格过期检查（issue #1190）：本地水位检查的判定矩阵——水位取现价缓存既有
//! 时点（行情通道 `priced_at` / 净值通道 `nav_date`，ADR-0038），阈值边界、
//! 北京日历日换算、持仓缺现价、通道豁免各一条。
//!
//! 判定是纯读：给定「今天」与库内状态结果唯一，故全部用例注入固定日期
//!（时钟是行为输入，ADR-0084 决策 5）。

use chrono::{NaiveDate, TimeZone, Utc};
use rusqlite::Connection;

use crate::prices::{MarketPriceWrite, upsert_market_price};
use crate::staleness::{PRICE_STALE_AFTER_DAYS, beijing_date, instrument_price_staleness_on};
use ledger_transaction::create_transaction_internal;
use tauri_app_lib::test_support::{open, seed_account, seed_fx_history_weeks};

use super::common::{insert_fund_instrument, insert_instrument_with_market, make_buy_input};

/// 判定基准日：周一（前一个周五距本日 3 个自然日，正在阈值上）。
fn today() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 3, 9).expect("固定基准日合法")
}

/// 写一条现价缓存水位（价格写入单点，非裸 SQL）：`priced_at` 为行情侧时点、
/// `nav_date` 为净值侧水位。
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

/// 让标的成为「持仓标的」（买入一批未卖出），口径与 `INVESTED_EXISTS` 同源。
/// 汇率行按货币对唯一，故每条用例先自行种一次 [`usd_rate`]。
fn hold(conn: &Connection, account_id: &str, instrument_id: &str) {
    seed_account(conn, account_id, "美股账户", "investment", "USD", 0);
    create_transaction_internal(
        conn,
        make_buy_input(account_id, instrument_id, 10.0, 10000, 0),
    )
    .expect("建仓应成功");
}

/// 建仓所需的本位币折算周点（每用例一次；#1547 写路径按交易日取数，
/// 建仓日期 make_buy_input 固定 2026-01-10）。
fn usd_rate(conn: &Connection) {
    seed_fx_history_weeks(conn, "USD", "CNY", 1.0, &["2026-01-10"]);
}

/// 行情通道阈值边界：水位距今 0 / 3 / 4 个自然日分别不计、不计（阈值上）、计入。
#[test]
fn quote_watermark_counts_only_beyond_threshold() {
    let conn = open();
    insert_instrument_with_market(&conn, "q-fresh", "F", "今日", "USD", "sh", "stock");
    insert_instrument_with_market(&conn, "q-edge", "E", "阈值上", "USD", "sh", "stock");
    insert_instrument_with_market(&conn, "q-old", "O", "过期", "USD", "sh", "stock");

    // 基准日 2026-03-09；水位日 = priced_at 的北京日历日
    seed_watermark(&conn, "q-fresh", "2026-03-09T02:00:00Z", None); // 3-09，0 天
    seed_watermark(&conn, "q-edge", "2026-03-06T02:00:00Z", None); // 3-06，3 天
    seed_watermark(&conn, "q-old", "2026-03-05T02:00:00Z", None); // 3-05，4 天

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.threshold_days, PRICE_STALE_AFTER_DAYS);
    assert_eq!(result.stale_count, 1, "只有超过阈值的行情水位计入过期");
}

/// 水位日按北京日历日换算（16:00 UTC 是北京午夜边界）：同一 UTC 日内的
/// 15:59:59Z 与 16:00:00Z 落在相邻的北京日历日，判定差一天——基准日 3-09 上
/// 前者距 4 天（过期）、后者距 3 天（阈值上，不过期）。
#[test]
fn quote_watermark_uses_beijing_calendar_day() {
    let conn = open();
    insert_instrument_with_market(&conn, "q-before", "B", "北京前一日", "USD", "sh", "stock");
    insert_instrument_with_market(&conn, "q-after", "A", "北京次日", "USD", "sh", "stock");

    // 北京 3-05 23:59 → 距 3-09 共 4 天（过期）；北京 3-06 00:00 → 3 天（不过期）
    seed_watermark(&conn, "q-before", "2026-03-05T15:59:59Z", None);
    seed_watermark(&conn, "q-after", "2026-03-05T16:00:00Z", None);

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 1, "跨北京午夜的水位按北京日历日判定");
}

/// 净值通道读 `nav_date` 水位、不读 `priced_at`：净值日期旧即过期（哪怕
/// `priced_at` 是新的）；净值日期新即不过期（哪怕 `priced_at` 是旧的）。
#[test]
fn fund_channel_reads_nav_date_watermark() {
    let conn = open();
    insert_fund_instrument(&conn, "f-old-nav", "110022", "旧净值");
    insert_fund_instrument(&conn, "f-fresh-nav", "000001", "新净值");

    seed_watermark(
        &conn,
        "f-old-nav",
        "2026-03-09T02:00:00Z",
        Some("2026-03-05"),
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

/// 水位缺失（从未落现价）：持仓标的按「持仓缺现价」计入，纯建档未交易的行
/// 不惊动用户；补上当期水位后不再计入。
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

/// 手动报价与无来源通道不在检查面：同步修不了它们（手动通道的出路是录价），
/// 即使持仓且无现价也不计入——也不许误报成「去同步」能解决。
#[test]
fn manual_and_no_source_channels_are_out_of_scope() {
    let conn = open();
    // 自建债券（bond → 手动报价通道）与市场未知的股票（→ 无来源），均无现价
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

/// 多端重放带来的纯日期水位按北京日历日直读；水位非法按缺失处置
/// （不静默当作新数据）——持仓行仍计入。
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

    seed_watermark(&conn, "q-date-only", "2026-03-05", None);
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

/// 空库零计数（无过期即不提示），阈值随结果透出。
#[test]
fn empty_book_reports_zero() {
    let conn = open();
    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 0);
    assert_eq!(result.threshold_days, PRICE_STALE_AFTER_DAYS);
}

/// 北京日历日 = UTC + 8 小时取日期（16:00 UTC 是北京午夜边界），与行情同步域
/// 同一规则。
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

// ---------------------------------------------------------------------------
// 恒定价格标的的整体豁免（ADR-0126 决策 7 / issue #1450）：恒定标的价格不随
// 时间变，水位语义对它不适用——计数面（含「持仓缺现价」一档）一并排除。
// 「删除即变红」：豁免臂撤掉（并入净值通道臂）后，下方两用例即被计入。
// ---------------------------------------------------------------------------

fn mark_constant(conn: &Connection, instrument_id: &str) {
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = ?1",
        [instrument_id],
    )
    .unwrap();
}

/// 给恒定标的建仓（基金申赎以确认单金额为权威：金额必填、单价反算不可携带）。
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
    // 持仓 + 现价缓存整行缺失：「持仓缺现价」档对恒定标的不成立（它的价格
    // 就是常量，无需水位），不计入。
    hold_fund(&conn, "acc-const", "f-const");

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 0, "恒定标的整体豁免过期计数");
}

#[test]
fn constant_price_instrument_is_exempt_even_with_stale_nav_date() {
    let conn = open();
    insert_fund_instrument(&conn, "f-const", "000198", "天弘余额宝");
    mark_constant(&conn, "f-const");
    // 打标前残留的旧净值日期水位：恒定标的没有水位语义，陈旧也不计入。
    seed_watermark(&conn, "f-const", "2026-01-30T02:00:00Z", Some("2026-01-30"));

    let result = instrument_price_staleness_on(&conn, today()).expect("检查应成功");
    assert_eq!(result.stale_count, 0, "陈旧残留水位对恒定标的不构成过期");
}
