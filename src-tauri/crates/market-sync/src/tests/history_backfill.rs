use chrono::{Datelike, Days, Duration as ChronoDuration, Months, NaiveDate};
use rusqlite::{Connection, params};

use std::sync::Mutex;

use crate::SyncProgress;
use crate::channels::{FetchFuture, QuoteQuery};
use crate::fund_backfill::{BackfillOutcome, backfill_one_fund_history};
use crate::fund_nav::FundIdentity;
use crate::fund_nav::NavPoint;
use crate::history::{HistoryBackfillStats, run_history_backfill_round};
use crate::http::KlineBar;
use crate::incremental::{SyncInstrument, beijing_today, daily_window_opens};
use crate::model::WriteWitness;
use crate::tests::insert_holding;
use crate::weekly::week_monday;
use ledger_infra::error::{AppError, Result};
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, SINA_PRICE_SOURCE, TENCENT_PRICE_SOURCE,
    price_value_to_cents, upsert_market_price, upsert_price_history,
};
use ledger_investment::{InstrumentType, Market, QuoteMarket, derive_price_channel};
use tauri_app_lib::test_support::seed_instrument;

fn bar(date: &str, close: f64) -> KlineBar {
    KlineBar {
        date: date.to_string(),
        close,
    }
}

fn nav_point(date: &str, nav: f64) -> NavPoint {
    NavPoint {
        date: date.to_string(),
        nav,
    }
}

fn no_confirm(_: &str) -> FetchFuture<bool> {
    Box::pin(std::future::ready(Ok(false)))
}

fn insert_plain_instrument(
    conn: &Connection,
    id: &str,
    symbol: &str,
    kind: &str,
    currency: &str,
    market: &str,
) {
    seed_instrument(
        conn,
        id,
        symbol,
        &format!("名称-{symbol}"),
        currency,
        market,
    );
    if kind != "stock" {
        conn.execute(
            "UPDATE instruments SET instrument_type=?1 WHERE id=?2",
            params![kind, id],
        )
        .unwrap();
    }
}

fn seed_history_point(conn: &Connection, instrument_id: &str, currency: &str, date: &str) {
    let cents = price_value_to_cents(10.0);
    upsert_price_history(
        conn,
        instrument_id,
        date,
        cents,
        currency,
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents: cents,
            currency_code: currency,
            priced_at: date,
            nav_date: Some(date),
            source: Some(EASTMONEY_PRICE_SOURCE),
        },
    )
    .unwrap();
}

fn date_offset(days: i64) -> String {
    (beijing_today() - ChronoDuration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

fn weekly_series(start: NaiveDate, end: NaiveDate) -> Vec<KlineBar> {
    let mut bars = Vec::new();
    let mut week = week_monday(start);
    while week <= end {
        let friday = week + Days::new(4);
        if friday <= end {
            bars.push(bar(&friday.format("%Y-%m-%d").to_string(), 12.0));
        }
        week = week + Days::new(7);
    }
    bars
}

fn seed_buy_transaction(
    conn: &Connection,
    account_id: &str,
    instrument_id: &str,
    currency: &str,
    date: &str,
) {
    tauri_app_lib::test_support::seed_account(
        conn,
        account_id,
        &format!("账户-{account_id}"),
        "investment",
        currency,
        0,
    );
    let txn_id = format!("txn-buy-{account_id}-{instrument_id}");
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id) \
         VALUES (?1,'buy',1000,?2,1000,?3,?4,?5,?5,1,'test')",
        params![
            txn_id,
            currency,
            account_id,
            date,
            format!("{date}T00:00:00Z")
        ],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO security_transactions (transaction_id,instrument_id,action,quantity,price_cents,fee_cents) \
         VALUES (?1,?2,'buy',10,100,0)",
        params![txn_id, instrument_id],
    )
    .unwrap();
}

fn history_rows(conn: &Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM price_history WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

struct Harness {
    log: Mutex<Vec<String>>,
    klines: Vec<(&'static str, Vec<KlineBar>)>,
    fail_kline: Option<&'static str>,
    nav_history: Vec<(&'static str, Vec<NavPoint>)>,
    fail_nav_history: Option<&'static str>,
}

impl Harness {
    fn new() -> Self {
        Self {
            log: Mutex::new(vec![]),
            klines: vec![],
            fail_kline: None,
            nav_history: vec![],
            fail_nav_history: None,
        }
    }

    fn with_klines(mut self, klines: Vec<(&'static str, Vec<KlineBar>)>) -> Self {
        self.klines = klines;
        self
    }

    fn with_failing_kline(mut self, key: &'static str) -> Self {
        self.fail_kline = Some(key);
        self
    }

    fn with_nav_history(mut self, history: Vec<(&'static str, Vec<NavPoint>)>) -> Self {
        self.nav_history = history;
        self
    }

    fn with_failing_nav_history(mut self, code: &'static str) -> Self {
        self.fail_nav_history = Some(code);
        self
    }

    fn requested(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }

    fn nav_history_hits(&self, code: &str) -> usize {
        self.requested()
            .iter()
            .filter(|entry| entry.as_str() == format!("history:{code}").as_str())
            .count()
    }

    fn kline_hits(&self, key: &str) -> usize {
        self.requested()
            .iter()
            .filter(|entry| entry.as_str() == format!("kline:{key}").as_str())
            .count()
    }

    fn fetch_kline(&self, query: &QuoteQuery) -> Result<Vec<KlineBar>> {
        let key = format!("{}:{}", query.market, query.code);
        self.log.lock().unwrap().push(format!("kline:{key}"));
        if self.fail_kline.map(|f| f == key).unwrap_or(false) {
            return Err(AppError::Io("周 K 抓取失败".into()));
        }
        Ok(self
            .klines
            .iter()
            .find(|(stored, _)| *stored == key.as_str())
            .map(|(_, bars)| bars.clone())
            .unwrap_or_default())
    }

    fn fetch_nav_history(&self, code: &str) -> Result<Vec<NavPoint>> {
        self.log.lock().unwrap().push(format!("history:{code}"));
        if self.fail_nav_history.map(|f| f == code).unwrap_or(false) {
            return Err(AppError::Parse("新浪基金历史净值响应不可解析".into()));
        }
        Ok(self
            .nav_history
            .iter()
            .find(|(stored, _)| *stored == code)
            .map(|(_, points)| points.clone())
            .unwrap_or_default())
    }
}

fn run_round(
    conn: &Connection,
    harness: &Harness,
) -> (Result<HistoryBackfillStats>, Vec<SyncProgress>, bool) {
    let progress_log: Mutex<Vec<SyncProgress>> = Mutex::new(vec![]);
    let mut witness = WriteWitness::default();
    let mut fetch_kline = |query: &QuoteQuery| super::ready(harness.fetch_kline(query));
    let mut fetch_nav_history = |code: &str| super::ready(harness.fetch_nav_history(code));
    let mut progress = |p: SyncProgress| progress_log.lock().unwrap().push(p);
    let result = tauri::async_runtime::block_on(run_history_backfill_round(
        conn,
        &mut fetch_kline,
        &mut fetch_nav_history,
        &mut no_confirm,
        &mut progress,
        &mut witness,
    ));
    (
        result,
        progress_log.lock().unwrap().clone(),
        witness.any_written(),
    )
}

#[test]
fn queue_keeps_only_incomplete_and_holding_comes_first() {
    let conn = tauri_app_lib::test_support::open();
    let fresh = date_offset(2);
    let stale = date_offset(14);

    insert_holding(&conn, "acc-1", "inst-held", "600519", "stock", "CNY", "sh");
    insert_plain_instrument(&conn, "inst-fresh", "000002", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-fresh", "CNY", &fresh);
    insert_plain_instrument(&conn, "inst-stale", "000003", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-stale", "CNY", &stale);
    insert_plain_instrument(&conn, "inst-fund", "000001", "fund", "CNY", "unknown");
    insert_plain_instrument(&conn, "inst-fund-fresh", "000004", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund-fresh", "CNY", &fresh);
    insert_plain_instrument(&conn, "inst-fund-stale", "000005", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund-stale", "CNY", &stale);
    insert_plain_instrument(&conn, "inst-bond", "BOND-X", "bond", "CNY", "unknown");
    insert_plain_instrument(&conn, "inst-nosrc", "NO-MKT", "stock", "CNY", "unknown");

    let harness = Harness::new()
        .with_klines(vec![
            ("sh:600519", vec![bar(&date_offset(1), 13.0)]),
            ("sz:000003", vec![bar(&date_offset(1), 7.0)]),
        ])
        .with_nav_history(vec![
            ("000001", vec![nav_point(&date_offset(1), 1.5)]),
            ("000005", vec![nav_point(&date_offset(1), 2.5)]),
        ]);

    let (result, _progress, _written) = run_round(&conn, &harness);
    let stats = result.unwrap();

    assert_eq!(stats.queued, 4, "只有历史不完整的通道内标的进队");
    assert_eq!(stats.failed, 0);
    assert_eq!(
        harness.requested(),
        vec![
            "kline:sh:600519".to_string(),
            "history:000001".to_string(),
            "kline:sz:000003".to_string(),
            "history:000005".to_string(),
        ],
        "抓取序 = 持仓优先 + symbol 升序"
    );
    let latest = |id: &str| -> String {
        conn.query_row(
            "SELECT MAX(trade_date) FROM price_history WHERE instrument_id=?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap()
        .unwrap()
    };
    assert_eq!(history_rows(&conn, "inst-held"), 1);
    assert_eq!(latest("inst-stale"), date_offset(1), "缺周点股补到新周点");
    assert_eq!(history_rows(&conn, "inst-fund"), 1);
    assert_eq!(
        latest("inst-fund-stale"),
        date_offset(1),
        "缺周点基金补到新周点"
    );
}

#[test]
fn fund_with_no_new_nav_completes_without_writing() {
    let conn = tauri_app_lib::test_support::open();
    let stale = date_offset(14);
    insert_plain_instrument(&conn, "inst-fund", "000001", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund", "CNY", &stale);

    let harness = Harness::new().with_nav_history(vec![("000001", vec![])]);
    let (result, progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(stats.queued, 1);
    assert_eq!(stats.failed, 0);
    assert!(!written, "无新净值 = 零写入，不置见证（零变化不广播）");
    assert_eq!(history_rows(&conn, "inst-fund"), 1, "原有历史不变");
    assert_eq!(
        progress,
        vec![
            SyncProgress::instrument(0, 1),
            SyncProgress::instrument(1, 1),
        ]
    );
}

#[test]
fn queue_drains_and_stays_empty_once_histories_complete() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-held", "600519", "stock", "CNY", "sh");
    let harness = Harness::new().with_klines(vec![(
        "sh:600519",
        weekly_series(
            NaiveDate::from_ymd_opt(2026, 1, 5).expect("常量日期应可解析"),
            beijing_today(),
        ),
    )]);

    let (result, progress, written) = run_round(&conn, &harness);
    result.unwrap();
    assert_eq!(progress.len(), 2, "首轮 0/1、1/1 两格");
    assert!(written, "首轮实际落库");

    let (result, progress, _written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(stats.queued, 0, "补齐后按派生事实退出队列");
    assert!(
        progress.is_empty(),
        "队列空零动作：不发进度（空转不伪装成推进）"
    );
    assert_eq!(
        harness
            .requested()
            .iter()
            .filter(|entry| entry.starts_with("kline:"))
            .count(),
        1,
        "第二轮零周 K 请求"
    );
}

#[test]
fn queue_enqueues_fund_and_stock_short_of_first_position() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "000025", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund", "CNY", &date_offset(2));
    seed_buy_transaction(&conn, "acc-fund", "inst-fund", "CNY", &date_offset(400));
    insert_plain_instrument(&conn, "inst-stock", "600519", "stock", "CNY", "sh");
    seed_history_point(&conn, "inst-stock", "CNY", &date_offset(2));
    seed_buy_transaction(&conn, "acc-stock", "inst-stock", "CNY", &date_offset(400));
    insert_plain_instrument(&conn, "inst-deep", "000026", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-deep", "CNY", &date_offset(2));
    seed_buy_transaction(&conn, "acc-deep", "inst-deep", "CNY", &date_offset(2));

    let harness = Harness::new()
        .with_nav_history(vec![("000025", vec![nav_point(&date_offset(1), 1.5)])])
        .with_klines(vec![("sh:600519", vec![bar(&date_offset(1), 13.0)])]);
    let (result, _progress, _written) = run_round(&conn, &harness);
    let stats = result.unwrap();

    assert_eq!(stats.queued, 2, "覆盖不足的基金与股票都进队");
    assert_eq!(stats.failed, 0);
    assert_eq!(
        harness.requested(),
        vec!["history:000025".to_string(), "kline:sh:600519".to_string(),],
        "基金走全历史通道、股票走周 K 通道各一次深回填请求；覆盖达标者零请求"
    );
}

#[test]
fn stock_deep_backfill_lands_history_back_to_first_position_week() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-stock", "600519", "stock", "CNY", "sh");
    seed_history_point(&conn, "inst-stock", "CNY", &date_offset(2));
    let first_buy = date_offset(800);
    seed_buy_transaction(&conn, "acc-stock", "inst-stock", "CNY", &first_buy);

    let series = weekly_series(beijing_today() - ChronoDuration::days(820), beijing_today());
    let harness = Harness::new().with_klines(vec![("sh:600519", series)]);
    let (result, _progress, written) = run_round(&conn, &harness);
    result.unwrap();

    assert!(written, "深回填实际落库");
    let rows = history_rows(&conn, "inst-stock");
    assert!(rows > 100, "820 天 ≈ 117 个周点，实际 {rows}");
    assert_earliest_row_within_first_position_week(&conn, "inst-stock", &first_buy);
}

#[test]
fn stock_deep_backfill_zero_new_points_writes_nothing() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-stock", "600519", "stock", "CNY", "sh");
    seed_history_point(&conn, "inst-stock", "CNY", &date_offset(2));
    seed_buy_transaction(&conn, "acc-stock", "inst-stock", "CNY", &date_offset(400));

    let harness = Harness::new().with_klines(vec![("sh:600519", vec![bar(&date_offset(2), 10.0)])]);
    let (result, _progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();

    assert_eq!(stats.queued, 1, "覆盖不足照常进队");
    assert_eq!(stats.failed, 0);
    assert!(!written, "零新点不置写入见证（不置脏不广播）");
    assert_eq!(history_rows(&conn, "inst-stock"), 1, "既有历史不变");
    assert_eq!(harness.kline_hits("sh:600519"), 1, "每轮至多一次请求");
}

#[test]
fn stock_without_position_flows_keeps_two_year_window() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-stock", "600519", "stock", "CNY", "sh");

    let series = weekly_series(
        beijing_today() - ChronoDuration::days(1200),
        beijing_today(),
    );
    let harness = Harness::new().with_klines(vec![("sh:600519", series)]);
    let (result, _progress, written) = run_round(&conn, &harness);
    result.unwrap();

    assert!(written, "首刷实际落库");
    let rows = price_history_rows(&conn, "inst-stock");
    assert!(
        rows.len() > 95 && rows.len() < 110,
        "裁剪后应只余近两年 ≈ 104 周，实际 {}",
        rows.len()
    );
    let window_start = crate::incremental::two_years_ago(beijing_today())
        .format("%Y-%m-%d")
        .to_string();
    let earliest = rows.first().map(|(d, _, _)| d.clone()).unwrap();
    assert!(
        earliest.as_str() >= window_start.as_str(),
        "最早周点不得早于近两年窗口起点 {window_start}，实际 {earliest}"
    );
}

#[test]
fn stock_weekly_gap_lands_no_point_for_the_missing_week() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-stock", "600519", "stock", "CNY", "sh");

    let older = date_offset(15);
    let newer = date_offset(1);
    let harness = Harness::new().with_klines(vec![(
        "sh:600519",
        vec![bar(&older, 10.0), bar(&newer, 11.0)],
    )]);
    let (result, _progress, written) = run_round(&conn, &harness);
    result.unwrap();

    assert!(written);
    assert_eq!(
        history_rows(&conn, "inst-stock"),
        2,
        "缺周即无点，两侧各一根"
    );
    let dates: Vec<String> = price_history_rows(&conn, "inst-stock")
        .into_iter()
        .map(|(d, _, _)| d)
        .collect();
    assert_eq!(dates, vec![older, newer], "曲线跨越空档，缺周不补行");
}

#[test]
fn stock_backfill_writes_no_market_price() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-stock", "600519", "stock", "CNY", "sh");
    let harness = Harness::new().with_klines(vec![("sh:600519", vec![bar(&date_offset(1), 13.0)])]);
    let (result, _progress, _written) = run_round(&conn, &harness);
    result.unwrap();

    let market_prices: i64 = conn
        .query_row(
            "SELECT count(*) FROM market_prices WHERE instrument_id='inst-stock'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(market_prices, 0, "周 K 车道不写现价缓存");
}

#[test]
fn fund_first_fill_covers_holding_period_when_first_buy_predates_window() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let first_buy = date_offset(800);
    seed_buy_transaction(&conn, "acc-fund", "inst-fund", "CNY", &first_buy);
    let series = daily_nav_series(beijing_today() - ChronoDuration::days(820), beijing_today());
    let full = [(
        "110022",
        series
            .iter()
            .map(|(d, n)| nav_point(d, *n))
            .collect::<Vec<_>>(),
    )];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&full, &requested);

    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert!(outcome.written);
    assert_earliest_row_within_first_position_week(&conn, "inst-fund", &first_buy);
}

#[test]
fn fund_deep_backfill_lands_history_back_to_first_position_week() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let recent = date_offset(2);
    seed_fund_price(&conn, "inst-fund", 30000, &recent, true);
    let first_buy = date_offset(800);
    seed_buy_transaction(&conn, "acc-fund", "inst-fund", "CNY", &first_buy);

    let series = daily_nav_series(beijing_today() - ChronoDuration::days(820), beijing_today());
    let latest = series.last().unwrap();
    let full = [(
        "110022",
        series
            .iter()
            .map(|(d, n)| nav_point(d, *n))
            .collect::<Vec<_>>(),
    )];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&full, &requested);

    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert!(outcome.written);
    let rows = price_history_rows(&conn, "inst-fund");
    assert!(rows.len() > 50, "400 天 ≈ 57 个周点，实际 {}", rows.len());
    assert_earliest_row_within_first_position_week(&conn, "inst-fund", &first_buy);
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((price_value_to_cents(latest.1), Some(latest.0.clone()))),
        "深回填后现价 = 窗口内最新公布净值"
    );
}

#[test]
fn fund_deep_backfill_zero_new_points_writes_nothing() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "000001", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund", "CNY", &date_offset(2));
    seed_buy_transaction(&conn, "acc-fund", "inst-fund", "CNY", &date_offset(400));

    let harness =
        Harness::new().with_nav_history(vec![("000001", vec![nav_point(&date_offset(2), 10.0)])]);
    let (result, _progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();

    assert_eq!(stats.queued, 1, "覆盖不足照常进队");
    assert_eq!(stats.failed, 0);
    assert!(!written, "零新点不置写入见证（不置脏不广播）");
    assert_eq!(history_rows(&conn, "inst-fund"), 1, "既有历史不变");
    assert_eq!(harness.nav_history_hits("000001"), 1, "每轮至多一次请求");
}

#[test]
fn queue_drains_after_deep_backfill_completes_coverage() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "000025", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund", "CNY", &date_offset(2));
    let first_buy = date_offset(800);
    seed_buy_transaction(&conn, "acc-fund", "inst-fund", "CNY", &first_buy);

    let series = daily_nav_series(beijing_today() - ChronoDuration::days(820), beijing_today());
    let harness = Harness::new().with_nav_history(vec![(
        "000025",
        series
            .iter()
            .map(|(d, n)| nav_point(d, *n))
            .collect::<Vec<_>>(),
    )]);

    let (result, _progress, written) = run_round(&conn, &harness);
    result.unwrap();
    assert!(written, "首轮深回填实际落库");
    assert_earliest_row_within_first_position_week(&conn, "inst-fund", &first_buy);

    let (result, progress, _written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(stats.queued, 0, "覆盖达标后按派生事实退出队列");
    assert!(progress.is_empty());
    assert_eq!(harness.nav_history_hits("000025"), 1, "第二轮零请求");
}

fn assert_earliest_row_within_first_position_week(
    conn: &Connection,
    instrument_id: &str,
    first_buy: &str,
) {
    let first = NaiveDate::parse_from_str(first_buy, "%Y-%m-%d").unwrap();
    let monday = week_monday(first);
    let week_end = (monday + Days::new(6)).format("%Y-%m-%d").to_string();
    let rows = price_history_rows(conn, instrument_id);
    let earliest = rows.first().map(|(d, _, _)| d.clone()).unwrap_or_default();
    assert!(
        earliest >= monday.format("%Y-%m-%d").to_string() && earliest <= week_end,
        "最早周点应落在首笔持仓周 [{}, {}] 内，实际 {earliest}",
        monday.format("%Y-%m-%d"),
        week_end
    );
}

#[test]
fn round_emits_instrument_level_progress_sequence() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    insert_plain_instrument(&conn, "inst-b", "000003", "stock", "CNY", "sz");
    let harness = Harness::new().with_klines(vec![
        ("sz:000002", vec![bar(&date_offset(1), 5.0)]),
        ("sz:000003", vec![bar(&date_offset(1), 6.0)]),
    ]);
    let (result, progress, _written) = run_round(&conn, &harness);
    result.unwrap();
    assert_eq!(
        progress,
        vec![
            SyncProgress::instrument(0, 2),
            SyncProgress::instrument(1, 2),
            SyncProgress::instrument(2, 2),
        ]
    );
}

#[test]
fn round_continues_after_single_instrument_failure() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    insert_plain_instrument(&conn, "inst-b", "000003", "stock", "CNY", "sz");
    let harness = Harness::new()
        .with_failing_kline("sz:000002")
        .with_klines(vec![("sz:000003", vec![bar(&date_offset(1), 6.0)])]);
    let (result, progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(stats.queued, 2);
    assert_eq!(stats.failed, 1, "失败单只记入统计，本轮继续");
    assert_eq!(history_rows(&conn, "inst-a"), 0, "失败者零落库");
    assert_eq!(history_rows(&conn, "inst-b"), 1, "后续标的照常补齐");
    assert!(written, "成功单只的写入入见证");
    assert_eq!(
        progress.last(),
        Some(&SyncProgress::instrument(2, 2)),
        "进度推进到队列排空"
    );
}

#[test]
fn round_continues_after_fund_history_failure() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "000001", "fund", "CNY", "unknown");
    let harness = Harness::new().with_failing_nav_history("000001");
    let (result, progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!((stats.queued, stats.failed), (1, 1), "失败单只记入统计");
    assert_eq!(harness.nav_history_hits("000001"), 1, "一次全历史请求");
    assert_eq!(history_rows(&conn, "inst-fund"), 0, "失败者零落库");
    assert!(!written, "零写入不置见证");
    assert_eq!(
        progress.last(),
        Some(&SyncProgress::instrument(1, 1)),
        "进度推进到队列排空"
    );
}

#[test]
fn stock_unchanged_kline_writes_nothing_but_changed_value_rewrites() {
    let conn = tauri_app_lib::test_support::open();
    let stale = date_offset(14);
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-a", "CNY", &stale);

    let harness = Harness::new().with_klines(vec![("sz:000002", vec![bar(&stale, 10.0)])]);
    let (result, _progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!((stats.queued, stats.failed), (1, 0));
    assert!(!written, "全部无新点不置见证");

    let harness = Harness::new().with_klines(vec![("sz:000002", vec![bar(&stale, 10.5)])]);
    let (result, _progress, written) = run_round(&conn, &harness);
    result.unwrap();
    assert!(written, "同周新值应重写并置见证");
    let stored: i64 = conn
        .query_row(
            "SELECT price_cents FROM price_history WHERE instrument_id='inst-a' AND trade_date=?1",
            params![stale],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(stored, price_value_to_cents(10.5));

    let fresh = date_offset(1);
    let harness = Harness::new().with_klines(vec![(
        "sz:000002",
        vec![bar(&stale, 10.5), bar(&fresh, 11.0)],
    )]);
    let (result, _progress, written) = run_round(&conn, &harness);
    result.unwrap();
    assert!(written);
    assert_eq!(history_rows(&conn, "inst-a"), 2);
}

#[test]
fn empty_kline_completes_without_writing() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    let harness = Harness::new();
    let (result, _progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(stats.queued, 1);
    assert_eq!(stats.failed, 0);
    assert!(!written, "零有效周点 = 零写入");
    assert_eq!(history_rows(&conn, "inst-a"), 0);
}

#[test]
fn swapping_kline_channel_keeps_history_landing_without_source_key_in_orchestration() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-held", "600519", "stock", "CNY", "sh");

    let mut fetch_kline = |query: &QuoteQuery| {
        let bars = if query.market == QuoteMarket::Sh && query.code == "600519" {
            vec![bar(&date_offset(1), 13.0)]
        } else {
            vec![]
        };
        super::ready(Ok(bars))
    };
    let mut fetch_nav_history = |_: &str| super::ready(Ok(vec![]));
    let mut progress = |_: SyncProgress| {};
    let mut witness = WriteWitness::default();
    let stats = tauri::async_runtime::block_on(run_history_backfill_round(
        &conn,
        &mut fetch_kline,
        &mut fetch_nav_history,
        &mut no_confirm,
        &mut progress,
        &mut witness,
    ))
    .unwrap();

    assert_eq!(stats.queued, 1);
    assert_eq!(stats.failed, 0);
    assert_eq!(
        history_rows(&conn, "inst-held"),
        1,
        "换一个自己构造查询键的通道实现，历史照常落库（编排不含数据源键）"
    );
}

#[test]
fn production_backfill_channel_lands_history_via_tencent_kline() {
    use crate::channels::{Lane, SyncFetchChannels, SyncFetchHosts};

    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-sh", "600519", "stock", "CNY", "sh");
    insert_plain_instrument(&conn, "inst-sz", "000001", "stock", "CNY", "sz");
    insert_plain_instrument(&conn, "inst-hk", "00700", "stock", "HKD", "hk");
    insert_plain_instrument(&conn, "inst-us", "AAPL", "stock", "USD", "nasdaq");

    let day = date_offset(1);
    let entry = |symbol: &str| {
        format!(
            r#""{symbol}":{{"week":[["{day}","1262.990","1257.120","1265.880","1256.100","24891.000"]]}}"#
        )
    };
    let body = format!(
        r#"{{"code":0,"msg":"","data":{{{},{},{},{}}}}}"#,
        entry("sh600519"),
        entry("sz000001"),
        entry("hk00700"),
        entry("usAAPL.OQ"),
    );
    let (url, heads) = super::spawn_header_capture_server(body);

    let channels = SyncFetchChannels::production_lane_on_pacer(
        Lane::Backfill,
        SyncFetchHosts {
            quote: vec![],
            kline: vec![url],
            fund_batch: vec![],
            fund_history: vec![],
            disclosure: vec![],
        },
        super::zero_pacer(),
    )
    .expect("生产后台车道束应可构造");
    let mut fetch_kline = channels.fetch_kline;
    let mut fetch_nav_history = |_: &str| super::ready(Ok(vec![]));
    let mut confirm = no_confirm;
    let mut progress = |_: SyncProgress| {};
    let mut witness = WriteWitness::default();
    let stats = tauri::async_runtime::block_on(run_history_backfill_round(
        &conn,
        &mut fetch_kline,
        &mut fetch_nav_history,
        &mut confirm,
        &mut progress,
        &mut witness,
    ))
    .unwrap();

    assert_eq!((stats.queued, stats.failed), (4, 0));
    for id in ["inst-sh", "inst-sz", "inst-hk", "inst-us"] {
        assert_eq!(
            history_rows(&conn, id),
            1,
            "{id} 的周 K 打到腾讯端点后周点照常落库"
        );
    }
    let source: String = conn
        .query_row(
            "SELECT source FROM price_history WHERE instrument_id='inst-us'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, TENCENT_PRICE_SOURCE, "历史补全来源标记为新来源值");

    let today = beijing_today();
    let beg = crate::tencent_kline::KLINE_BEG;
    let end = today.format("%Y-%m-%d").to_string();
    let heads = heads.lock().unwrap().clone();
    for symbol in ["sh600519", "sz000001", "hk00700", "usAAPL.OQ"] {
        let expected =
            format!("/appstock/app/fqkline/get?param={symbol}%2Cweek%2C{beg}%2C{end}%2C1200%2C");
        assert!(
            heads.iter().any(|head| head.contains(&expected)),
            "周 K 请求须为腾讯 fqkline/get 且带腾讯查询键与整段周线区间，缺 {expected}，实际：{heads:?}"
        );
    }
}

#[test]
fn production_backfill_channel_lands_fund_history_via_sina() {
    use crate::channels::{Lane, SyncFetchChannels, SyncFetchHosts};

    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");

    let day = date_offset(1);
    let body = format!(
        r#"{{"result":{{"status":{{"code":0}},"data":{{"data":[
            {{"fbrq":"{day} 00:00:00","jjjz":"3.348","ljjz":"3.906"}},
            {{"fbrq":"2026-01-28 00:00:00","jjjz":"3.293","ljjz":"3.871"}}
        ],"total_num":"2"}}}}}}"#
    );
    let (url, heads) = super::spawn_header_capture_server(body);

    let channels = SyncFetchChannels::production_lane_on_pacer(
        Lane::Backfill,
        SyncFetchHosts {
            quote: vec![],
            kline: vec![],
            fund_batch: vec![],
            fund_history: vec![url],
            disclosure: vec![],
        },
        super::zero_pacer(),
    )
    .expect("生产后台车道束应可构造");
    let mut fetch_kline = |_: &QuoteQuery| super::ready(Ok(vec![]));
    let mut fetch_nav_history = channels.fetch_nav_history;
    let mut confirm = no_confirm;
    let mut progress = |_: SyncProgress| {};
    let mut witness = WriteWitness::default();
    let stats = tauri::async_runtime::block_on(run_history_backfill_round(
        &conn,
        &mut fetch_kline,
        &mut fetch_nav_history,
        &mut confirm,
        &mut progress,
        &mut witness,
    ))
    .unwrap();

    assert_eq!((stats.queued, stats.failed), (1, 0));
    assert_eq!(
        history_rows(&conn, "inst-fund"),
        2,
        "基金历史打到新浪端点后周点照常落库"
    );
    let source: String = conn
        .query_row(
            "SELECT source FROM price_history WHERE instrument_id='inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, SINA_PRICE_SOURCE, "历史补全来源标记为新来源值");

    let heads = heads.lock().unwrap().clone();
    let expected = "/fundInfo/api/openapi.php/CaihuiFundInfoService.getNav?";
    assert!(
        heads.iter().any(|head| head.contains(expected)
            && head.contains("symbol=110022")
            && head.contains("num=10000")),
        "历史净值请求须为新浪全历史端点且带基金代码与单请求取全参数，实际：{heads:?}"
    );
}

#[test]
fn staleness_window_requires_iso_week_gap_over_seven_days() {
    let today = beijing_today();
    let near_gap = (week_monday(today)
        - week_monday(chrono::NaiveDate::parse_from_str(&date_offset(6), "%Y-%m-%d").unwrap()))
    .num_days();
    let stale_gap = (week_monday(today)
        - week_monday(chrono::NaiveDate::parse_from_str(&date_offset(14), "%Y-%m-%d").unwrap()))
    .num_days();

    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-near", "000002", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-near", "CNY", &date_offset(6));
    insert_plain_instrument(&conn, "inst-stale", "000003", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-stale", "CNY", &date_offset(14));
    let harness = Harness::new();
    let (result, _progress, _written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(
        (near_gap > 7, stale_gap > 7),
        (false, true),
        "测试基线的周差形态：6 天前 ≤ 7、14 天前 > 7"
    );
    assert_eq!(
        stats.queued,
        if near_gap > 7 { 2 } else { 1 },
        "6 天前的周点不缺周点（不进队），14 天前的进队"
    );
}

#[test]
fn background_lane_yields_while_foreground_in_flight() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let foreground = crate::http::ForegroundGuard::enter();
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        tauri::async_runtime::block_on(crate::http::wait_foreground_idle());
        done_tx.send(()).expect("放行通知应可送达");
    });
    thread::sleep(Duration::from_millis(150));
    assert!(
        done_rx.recv_timeout(Duration::from_millis(50)).is_err(),
        "前台在途期间后台不得放行"
    );
    drop(foreground);
    waiter.join().expect("等待线程不应 panic");
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("前台离场后后台应放行");
}

#[test]
fn shared_pacer_is_process_wide_singleton() {
    use std::sync::Arc;
    assert!(Arc::ptr_eq(
        &crate::http::shared_pacer(),
        &crate::http::shared_pacer(),
    ));
}

fn fund_instrument(id: &str, code: &str) -> SyncInstrument {
    SyncInstrument {
        instrument_id: id.to_string(),
        symbol: code.to_string(),
        market: Market::Unknown,
        kind: InstrumentType::Fund,
        constant_unit_price: None,
        currency: "CNY".to_string(),
        channel: derive_price_channel(InstrumentType::Fund, Market::Unknown, code, None),
    }
}

async fn run_fund_backfill<H, C>(
    conn: &Connection,
    fund: &SyncInstrument,
    fetch_nav_history: &mut H,
    confirm_money_fund: &mut C,
) -> Result<BackfillOutcome>
where
    H: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    C: FnMut(&str) -> FetchFuture<bool> + Send,
{
    backfill_one_fund_history(
        conn,
        &FundIdentity::from_instrument(fund),
        fetch_nav_history,
        confirm_money_fund,
    )
    .await
}

fn mock_nav_history<'a>(
    history_by_code: &'a [(&'a str, Vec<NavPoint>)],
    requested: &'a Mutex<Vec<String>>,
) -> impl FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send + 'a {
    move |code: &str| {
        requested.lock().unwrap().push(code.to_string());
        let mut points = history_by_code
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, points)| points.clone())
            .unwrap_or_default();
        points.reverse();
        super::ready(Ok(points))
    }
}

fn daily_nav_series(start: NaiveDate, end: NaiveDate) -> Vec<(String, f64)> {
    let mut series = Vec::new();
    let mut nav = 1.0_f64;
    let mut day = start;
    while day <= end {
        if day.weekday().num_days_from_monday() < 5 {
            nav += 0.001;
            series.push((day.format("%Y-%m-%d").to_string(), nav));
        }
        day = day.succ_opt().unwrap();
    }
    series
}

fn expected_first_sync_start() -> String {
    beijing_today()
        .checked_sub_months(Months::new(24))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string()
}

fn fund_price_of(conn: &Connection, instrument_id: &str) -> Option<(i64, Option<String>)> {
    conn.query_row(
        "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

fn price_history_rows(conn: &Connection, instrument_id: &str) -> Vec<(String, i64, String)> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_date, price_cents, currency_code FROM price_history \
             WHERE instrument_id=?1 ORDER BY trade_date",
        )
        .unwrap();
    let rows = stmt
        .query_map(params![instrument_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

fn inject_week_write_failure(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER inject_week_write_failure BEFORE INSERT ON price_history \
         WHEN NEW.trade_date='2026-01-19' \
         BEGIN SELECT RAISE(ABORT, '注入周点写入失败'); END;",
    )
    .unwrap();
}

fn seed_fund_price(
    conn: &Connection,
    instrument_id: &str,
    price_cents: i64,
    nav_date: &str,
    with_history: bool,
) {
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents,
            currency_code: "CNY",
            priced_at: nav_date,
            nav_date: Some(nav_date),
            source: Some(EASTMONEY_PRICE_SOURCE),
        },
    )
    .unwrap();
    if with_history {
        upsert_price_history(
            conn,
            instrument_id,
            nav_date,
            price_cents,
            "CNY",
            EASTMONEY_PRICE_SOURCE,
        )
        .unwrap();
    }
}

#[test]
fn fund_first_sync_backfills_two_years_from_full_history() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let window_start = beijing_today().checked_sub_months(Months::new(24)).unwrap();
    let five_years_ago = beijing_today().checked_sub_months(Months::new(60)).unwrap();
    let series = daily_nav_series(five_years_ago, beijing_today());
    let full_by_code = [(
        "110022",
        series
            .iter()
            .map(|(d, n)| nav_point(d, *n))
            .collect::<Vec<_>>(),
    )];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&full_by_code, &requested);

    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(requested.lock().unwrap().len(), 1, "首刷每次一条请求取整只");
    assert_eq!(requested.lock().unwrap()[0], "110022", "按基金代码取全量");

    let rows = price_history_rows(&conn, "inst-fund");
    assert!(
        rows.len() >= 100,
        "近两年应有约 104 个周点，实际 {}",
        rows.len()
    );
    let earliest = rows.first().unwrap().0.as_str();
    let expected_start = expected_first_sync_start();
    let first_week_end = (window_start + Days::new(6)).format("%Y-%m-%d").to_string();
    assert!(
        earliest >= expected_start.as_str() && earliest <= first_week_end.as_str(),
        "最早周点应落在两年窗口首周内（窗口外点被裁剪）：{earliest} ∉ [{expected_start}, {first_week_end}]"
    );

    let latest = series.last().unwrap();
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((price_value_to_cents(latest.1), Some(latest.0.clone()))),
    );
    let source: String = conn
        .query_row(
            "SELECT source FROM market_prices WHERE instrument_id='inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, SINA_PRICE_SOURCE, "历史补全来源标记为新来源值");
}

#[test]
fn fund_with_nav_date_but_no_history_backfills_two_years() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let watermark = date_offset(1);
    seed_fund_price(&conn, "inst-fund", 35000, &watermark, false);
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![],
        "前置：无任何历史序列（水位冒充已回填的现场）"
    );

    let start = beijing_today().checked_sub_months(Months::new(24)).unwrap();
    let series = daily_nav_series(start, beijing_today());
    let full_by_code = [(
        "110022",
        series
            .iter()
            .map(|(d, n)| nav_point(d, *n))
            .collect::<Vec<_>>(),
    )];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&full_by_code, &requested);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(
        requested.lock().unwrap().len(),
        1,
        "首刷判据按历史序列判定：走全历史通道"
    );

    let rows = price_history_rows(&conn, "inst-fund");
    assert!(
        rows.len() >= 100,
        "首刷应回填近两年周线，实际只有 {} 个周点",
        rows.len()
    );
    let earliest = rows.first().unwrap().0.as_str();
    let window_start = expected_first_sync_start();
    let first_week_end = (start + Days::new(6)).format("%Y-%m-%d").to_string();
    assert!(
        earliest >= window_start.as_str() && earliest <= first_week_end.as_str(),
        "最早周点应落在两年窗口的首周内：{earliest} ∉ [{window_start}, {first_week_end}]"
    );

    let latest = series.last().unwrap();
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((price_value_to_cents(latest.1), Some(latest.0.clone()))),
    );
}

#[test]
fn fund_first_sync_without_window_points_lands_nothing() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    insert_plain_instrument(&conn, "inst-old", "002503", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");
    let terminated = fund_instrument("inst-old", "002503");

    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&[], &requested);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();
    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: false
        },
        "可信空：结局确定（无数据），非「窗口不完整」"
    );
    assert_eq!(price_history_rows(&conn, "inst-fund"), vec![]);
    assert_eq!(fund_price_of(&conn, "inst-fund"), None, "查无净值不写现价");

    let stale = vec![nav_point("2010-08-20", 1.0)];
    let full_by_code = [("002503", stale)];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&full_by_code, &requested);
    let outcome = tauri::async_runtime::block_on(run_fund_backfill(
        &conn,
        &terminated,
        &mut nav,
        &mut no_confirm,
    ))
    .unwrap();
    assert_eq!(
        requested.lock().unwrap().len(),
        1,
        "已终止基金同样发起全历史请求"
    );
    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: false
        },
        "窗口外点：结局确定（无数据），不在窗口内凭空造点"
    );
    assert_eq!(price_history_rows(&conn, "inst-old"), vec![]);
    assert_eq!(fund_price_of(&conn, "inst-old"), None);
}

#[test]
fn fund_first_sync_terminated_fund_within_window_backfills_to_last_point() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-term", "019852", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-term", "019852");

    let last = date_offset(30);
    let history = [(
        "019852",
        vec![nav_point(&date_offset(40), 1.1), nav_point(&last, 1.08)],
    )];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&history, &requested);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(
        price_history_rows(&conn, "inst-term").len(),
        2,
        "末点在窗口内的已终止基金整根回填"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-term"),
        Some((price_value_to_cents(1.08), Some(last))),
        "现价 = 末点净值（清盘前最后公布）"
    );
}

#[test]
fn fund_history_fetch_failure_propagates_without_writing() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let watermark = date_offset(7);
    seed_fund_price(&conn, "inst-fund", 30000, &watermark, true);

    let mut nav =
        |_: &str| super::ready(Err(AppError::Parse("新浪基金历史净值响应不可解析".into())));
    let err =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .expect_err("不可信响应应上抛，不按「无净值」计成功");
    assert!(
        err.to_string().contains("不可解析"),
        "取数层错误原样上抛：{err}"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund").len(),
        1,
        "零落库：既有历史点原样保留"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((30000, Some(watermark.clone()))),
        "失败不动现价、水位不前进"
    );
}

#[test]
fn fund_incremental_never_repulls_watermark_day() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    seed_fund_price(&conn, "inst-fund", 30000, "2026-01-30", true);

    let history = [("110022", vec![nav_point("2026-01-30", 3.348)])];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&history, &requested);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: false
        },
        "窗口内（水位次日之后）无新净值：结局确定且零写入"
    );
    assert_eq!(requested.lock().unwrap().len(), 1, "常态增量每只一次请求");
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![("2026-01-30".into(), 30000, "CNY".into())],
        "水位当日不重拉：既有周点与取值原样保留"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((30000, Some("2026-01-30".into()))),
        "无新净值不动现价"
    );
}

#[test]
fn fund_incremental_fetches_from_watermark_and_overwrites_same_week() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    seed_fund_price(&conn, "inst-fund", 30000, "2026-01-28", true);
    upsert_price_history(
        &conn,
        "inst-fund",
        "2026-01-23",
        31000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();

    let history = [(
        "110022",
        vec![
            nav_point("2026-01-23", 3.1),
            nav_point("2026-01-28", 3.0),
            nav_point("2026-01-29", 3.42),
            nav_point("2026-01-30", 3.348),
        ],
    )];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&history, &requested);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(requested.lock().unwrap().len(), 1, "常态增量每只一次请求");

    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![
            ("2026-01-23".into(), 31000, "CNY".into()),
            ("2026-01-30".into(), 33480, "CNY".into()),
        ],
        "水位当日不重拉；同周新净值整周覆盖采样日；更早周原样保留"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
        "现价 = 窗口内最新公布净值，nav_date 兼任下次水位"
    );
}

#[test]
fn fund_incremental_up_to_date_counts_synced_without_write() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let watermark = date_offset(7);
    seed_fund_price(&conn, "inst-fund", 30000, &watermark, true);
    let history = [("110022", vec![nav_point(&watermark, 3.0)])];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&history, &requested);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap();

    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: false
        },
        "已是最新：结局确定且零写入"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((30000, Some(watermark))),
        "无新净值不动现价"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund").len(),
        1,
        "无新净值零落库：既有历史点原样保留（水位增量语义，issue #1059）"
    );
}

#[test]
fn fund_backfill_write_failure_leaves_no_history() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");
    inject_week_write_failure(&conn);

    let series = vec![
        nav_point("2026-01-05", 1.000),
        nav_point("2026-01-12", 1.100),
        nav_point("2026-01-19", 1.200),
        nav_point("2026-01-30", 1.300),
    ];
    let full = [("110022", series)];
    let requested = Mutex::new(Vec::new());
    let mut nav = mock_nav_history(&full, &requested);
    let err =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut no_confirm))
            .unwrap_err();

    assert!(
        err.to_string().contains("注入周点写入失败"),
        "注入的写入失败应原样上抛：{err}"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![],
        "第 N 个周点写入失败时整只回滚，不留半根历史（下次运行仍按首刷重新采集）"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        None,
        "整只一次提交：现价与历史同事务，回滚后不留现价"
    );

    conn.execute_batch("DROP TRIGGER inject_week_write_failure;")
        .unwrap();
    let full_retry = [(
        "110022",
        vec![
            nav_point("2026-01-05", 1.000),
            nav_point("2026-01-12", 1.100),
            nav_point("2026-01-19", 1.200),
            nav_point("2026-01-30", 1.300),
        ],
    )];
    let mut nav_retry = mock_nav_history(&full_retry, &requested);
    let retry = tauri::async_runtime::block_on(run_fund_backfill(
        &conn,
        &fund,
        &mut nav_retry,
        &mut no_confirm,
    ))
    .unwrap();
    assert!(
        retry.written,
        "解除注入后重跑按首刷重新采集（原断言 synced=1）"
    );
    assert!(!retry.inconclusive);
    assert_eq!(
        price_history_rows(&conn, "inst-fund").len(),
        4,
        "下次运行补回整只历史（4 个周点）"
    );
}

#[test]
fn queue_excludes_constant_price_instruments() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-const", "000198", "fund", "CNY", "unknown");
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = 'inst-const'",
        [],
    )
    .unwrap();

    let harness = Harness::new();
    let (result, progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(stats.queued, 0, "恒定标的不进补全队列");
    assert!(progress.is_empty(), "队列空零动作：不发进度、零网络请求");
    assert!(!written);
}

#[test]
fn fund_first_sync_money_shape_marks_and_lands_no_flat_rows() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-money", "000905", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-money", "000905");

    let nav_calls = Mutex::new(0usize);
    let mut nav = income_value_nav_history(&nav_calls);

    let confirm_calls = Mutex::new(0usize);
    let mut confirm = confirm_true_counting(&confirm_calls);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut confirm))
            .unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    let cents: Option<i64> = conn
        .query_row(
            "SELECT constant_unit_price FROM instruments WHERE id = 'inst-money'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cents, Some(10_000), "官方披露自报形态确认即打标");
    assert_eq!(
        fund_price_of(&conn, "inst-money"),
        Some((10_000, None)),
        "建档常量价 1.0000 落现价缓存、净值日期为空"
    );
    assert!(
        price_history_rows(&conn, "inst-money").is_empty(),
        "确认即收尾：平坦历史行不再生长（读侧按常量取值）"
    );
    assert_eq!(*confirm_calls.lock().unwrap(), 1, "一次确认请求");
    assert_eq!(*nav_calls.lock().unwrap(), 0, "确认即收尾：零净值抓取请求");
}

#[test]
fn fund_backfill_gate_finishes_instrument_marked_constant_after_enqueue() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-money", "000905", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-money", "000905");
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = 'inst-money'",
        [],
    )
    .unwrap();

    let nav_calls = Mutex::new(0usize);
    let mut nav = empty_nav_history_counting(&nav_calls);
    let confirm_calls = Mutex::new(0usize);
    let mut confirm = confirm_true_counting(&confirm_calls);
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut confirm))
            .unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(*nav_calls.lock().unwrap(), 0, "判定门在抓取前收尾");
    assert_eq!(*confirm_calls.lock().unwrap(), 1, "一次确认请求");
    let cents: Option<i64> = conn
        .query_row(
            "SELECT constant_unit_price FROM instruments WHERE id = 'inst-money'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cents, Some(10_000), "既有标记不被清空");
    assert_eq!(
        fund_price_of(&conn, "inst-money"),
        Some((10_000, None)),
        "兜底建档常量价 1.0000 落现价缓存、净值日期为空"
    );
    assert!(
        price_history_rows(&conn, "inst-money").is_empty(),
        "确认即收尾：平坦历史行不生长"
    );
}

#[test]
fn fund_first_sync_disclosure_unavailable_is_inconclusive_and_lands_nothing() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-money", "000905", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-money", "000905");

    let nav_calls = Mutex::new(0usize);
    let mut nav = empty_nav_history_counting(&nav_calls);
    let mut confirm: fn(&str) -> FetchFuture<bool> = |_| {
        Box::pin(std::future::ready(Err(AppError::coded(
            "sync.disclosure-source-malformed",
            "基金官方披露数据源返回了无法解析的内容，请稍后重试同步",
        ))))
    };
    let outcome =
        tauri::async_runtime::block_on(run_fund_backfill(&conn, &fund, &mut nav, &mut confirm))
            .unwrap();

    assert!(!outcome.written);
    assert!(outcome.inconclusive, "披露源不可信 = 待重试，不是无数据");
    assert_eq!(
        fund_price_of(&conn, "inst-money"),
        None,
        "本轮整只不落：万份收益不得经取数面冒充单位净值"
    );
    assert!(price_history_rows(&conn, "inst-money").is_empty());
    let cents: Option<i64> = conn
        .query_row(
            "SELECT constant_unit_price FROM instruments WHERE id = 'inst-money'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(cents, None, "披露源不可信不打标");
    assert_eq!(*nav_calls.lock().unwrap(), 0, "判定门在抓取前收尾");
}

fn confirm_true_counting(calls: &Mutex<usize>) -> impl FnMut(&str) -> FetchFuture<bool> + '_ {
    move |_: &str| {
        *calls.lock().unwrap() += 1;
        Box::pin(std::future::ready(Ok(true)))
    }
}

fn empty_nav_history_counting(
    calls: &Mutex<usize>,
) -> impl FnMut(&str) -> FetchFuture<Vec<NavPoint>> + '_ {
    move |_: &str| {
        *calls.lock().unwrap() += 1;
        super::ready(Ok(vec![]))
    }
}

fn income_value_nav_history(
    calls: &Mutex<usize>,
) -> impl FnMut(&str) -> FetchFuture<Vec<NavPoint>> + '_ {
    move |_: &str| {
        *calls.lock().unwrap() += 1;
        super::ready(Ok(vec![NavPoint {
            date: "2026-09-17".into(),
            nav: 0.2229,
        }]))
    }
}

#[test]
fn daily_window_opens_only_once_per_beijing_calendar_day() {
    let day1 = NaiveDate::from_ymd_opt(2026, 1, 30).unwrap();
    let day2 = NaiveDate::from_ymd_opt(2026, 1, 31).unwrap();

    assert!(
        daily_window_opens(None, day1),
        "从未跑过（启动补跑）：窗口应开"
    );
    assert!(
        !daily_window_opens(Some(day1), day1),
        "同一自然日已跑过：巡检再到期不得重开窗口（同日只跑一轮）"
    );
    assert!(
        daily_window_opens(Some(day1), day2),
        "跨到下一个北京日历日：窗口重新打开"
    );
}
