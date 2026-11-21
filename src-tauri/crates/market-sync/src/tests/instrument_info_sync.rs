use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rusqlite::{Connection, params};

use crate::SyncProgress;
use crate::bulk::{
    BULK_DISABLE_PERIOD, BULK_FAILURE_THRESHOLD, BulkFetchCircuit, BulkFetchSurfaces, BulkNavPoint,
    FetchFundBatch, FundBatch, FundNameDictionary, FundNavTable,
};
use crate::channels::{
    FetchFuture, FetchMoneyFundForm, Lane, QuoteItem, QuoteQuery, SyncFetchChannels,
    SyncFetchHosts, do_incremental_sync_channels,
};
use crate::fund_nav::NavPoint;
use crate::incremental::{beijing_date, beijing_today, do_incremental_sync_with};
use crate::model::WriteWitness;
use crate::session::ScopedSession;
use crate::{FetchFundName, FetchNavHistory};
use ledger_infra::error::{AppError, Result};
use ledger_investment::QuoteMarket;
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, SINA_PRICE_SOURCE, TENCENT_PRICE_SOURCE,
    upsert_market_price, upsert_price_history,
};

use super::{insert_holding, insert_lot};
use tauri_app_lib::test_support::{seed_account, seed_instrument};

fn market_price_of(conn: &Connection, instrument_id: &str) -> Option<i64> {
    conn.query_row(
        "SELECT price_cents FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .ok()
}

impl ScopedSession for Connection {
    fn with_connection<R, F>(
        &self,
        use_connection: F,
    ) -> impl std::future::Future<Output = Result<R>> + Send
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        std::future::ready(use_connection(self))
    }
}

#[test]
fn orchestration_takes_connection_only_outside_fetch_closures() {
    struct RecordingSession<'a> {
        conn: &'a Connection,
        log: &'a Mutex<Vec<&'static str>>,
    }

    impl ScopedSession for RecordingSession<'_> {
        fn with_connection<R, F>(
            &self,
            use_connection: F,
        ) -> impl std::future::Future<Output = Result<R>> + Send
        where
            F: FnOnce(&Connection) -> Result<R> + Send + 'static,
            R: Send + 'static,
        {
            self.log.lock().unwrap().push("take");
            let result = use_connection(self.conn);
            self.log.lock().unwrap().push("release");
            std::future::ready(result)
        }
    }

    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let log: Mutex<Vec<&'static str>> = Mutex::new(vec![]);
    let session = RecordingSession {
        conn: &conn,
        log: &log,
    };

    let prices = [("600519", Some(13_028_000))];
    let mut fetch = mock_fetch(&prices);
    let mut logging_fetch = |queries: &[QuoteQuery]| {
        log.lock().unwrap().push("fetch:start");
        let items = fetch(queries);
        log.lock().unwrap().push("fetch:end");
        items
    };

    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &session,
        &mut logging_fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);

    let log = log.lock().unwrap();
    let mut in_fetch = false;
    for &event in log.iter() {
        match event {
            "fetch:start" => {
                assert!(!in_fetch, "抓取嵌套");
                in_fetch = true;
            }
            "fetch:end" => {
                assert!(in_fetch, "抓取起止不成对");
                in_fetch = false;
            }
            "take" | "release" => assert!(
                !in_fetch,
                "抓取闭包执行期间不应有会话被取出（网络期间不持连接，issue #1275）"
            ),
            other => panic!("未知事件 {other}"),
        }
    }
    assert!(!in_fetch, "抓取起止不成对");
    assert_eq!(
        *log,
        vec![
            "take",
            "release",
            "fetch:start",
            "fetch:end",
            "take",
            "release",
        ],
        "读写只在会话内、抓取只在会话外的交织形状（issue #1275）"
    );
}

fn query_log_line(queries: &[QuoteQuery]) -> String {
    queries
        .iter()
        .map(|q| format!("{}:{}", q.market, q.code))
        .collect::<Vec<_>>()
        .join(",")
}

fn mock_fetch<'a>(
    prices: &'a [(&'a str, Option<i64>)],
) -> impl FnMut(&[QuoteQuery]) -> FetchFuture<Vec<QuoteItem>> + Send + 'a {
    move |queries: &[QuoteQuery]| {
        let mut items = Vec::new();
        for query in queries {
            let code = query.code.clone();
            if let Some((_, price)) = prices.iter().find(|(c, _)| *c == query.code.as_str()) {
                items.push(quote_item(query, &format!("名称-{code}"), *price, None));
            }
        }
        super::ready(Ok(items))
    }
}

fn quote_item(
    query: &QuoteQuery,
    name: &str,
    price_cents: Option<i64>,
    price_date: Option<&str>,
) -> QuoteItem {
    QuoteItem {
        code: query.code.clone(),
        name: name.to_string(),
        price_cents,
        price_date: price_date.map(str::to_string),
    }
}

fn quote_items_for(queries: &[QuoteQuery]) -> Vec<QuoteItem> {
    queries
        .iter()
        .map(|query| quote_item(query, &format!("名称-{}", query.code), Some(100_000), None))
        .collect()
}

#[test]
fn incremental_sync_normalizes_symbol_suffix() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519.SH", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-hk", "00700.HK", "stock", "HKD", "hk");

    let prices = [("600519", Some(13_028_000)), ("00700", Some(4_454_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 2);
    assert_eq!(result.skipped, 0);
    assert_eq!(
        market_price_of(&conn, "inst-sh"),
        Some(13028000),
        "A 股 f2 × 100 得万分之一元"
    );
    assert_eq!(
        market_price_of(&conn, "inst-hk"),
        Some(4454000),
        "港股 f2 × 10 得万分之一元"
    );
}

#[test]
fn incremental_sync_all_missing_response_counts_all_skipped() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sh");

    let mut fetch = mock_fetch(&[]);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 2);
    assert_eq!(market_price_of(&conn, "inst-a"), None);
    assert_eq!(market_price_of(&conn, "inst-b"), None);
}

#[test]
fn incremental_sync_empty_library_returns_message() {
    let conn = tauri_app_lib::test_support::open();
    let mut fetch = mock_fetch(&[]);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.message, "暂无标的可同步");
}

#[test]
fn incremental_sync_updates_holding_prices_only() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sz", "000001", "stock", "CNY", "sz");
    insert_holding(&conn, "acc-3", "inst-hk", "00700", "stock", "HKD", "hk");
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-sh",
            price_cents: 999,
            currency_code: "CNY",
            priced_at: tauri_app_lib::test_support::FIXED_NOW,
            nav_date: None,
            source: Some("eastmoney"),
        },
    )
    .unwrap();

    let prices = [
        ("600519", Some(13_028_000)),
        ("000001", Some(117_300)),
        ("00700", Some(4_454_000)),
    ];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 3);
    assert_eq!(result.skipped, 0);
    assert_eq!(result.message, "已同步 3 只，跳过 0 只");

    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(market_price_of(&conn, "inst-sz"), Some(117300));
    assert_eq!(market_price_of(&conn, "inst-hk"), Some(4454000));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    let (name, market): (Option<String>, String) = conn
        .query_row(
            "SELECT name, market FROM instruments WHERE id='inst-sh'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("名称-600519"));
    assert_eq!(market, "sh");
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM instruments", [], |r| r.get(0))
        .unwrap();
    assert_eq!(total, 3);
}

#[test]
fn incremental_sync_skips_holdings_without_quote_source() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(
        &conn,
        "acc-2",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-3",
        "inst-other",
        "稳稳地幸福",
        "other",
        "CNY",
        "unknown",
    );

    let prices = [("600519", Some(13_028_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(
        result.skipped, 2,
        "无行情来源类型（债券/其他）计入跳过统计（基金走净值通道另测）"
    );
    assert_eq!(result.message, "已同步 1 只，跳过 2 只");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(
        market_price_of(&conn, "inst-bond"),
        None,
        "无行情来源持仓不写价格"
    );
    assert_eq!(market_price_of(&conn, "inst-other"), None);
}

#[test]
fn incremental_sync_keeps_old_price_when_suspended() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sz", "000001", "stock", "CNY", "sz");
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-sz",
            price_cents: 888,
            currency_code: "CNY",
            priced_at: tauri_app_lib::test_support::FIXED_NOW,
            nav_date: None,
            source: Some("eastmoney"),
        },
    )
    .unwrap();

    let prices = [("600519", Some(13_028_000)), ("000001", None)];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1, "停牌/无效价应计入跳过且不中断同步");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(
        market_price_of(&conn, "inst-sz"),
        Some(888),
        "停牌应保留旧价"
    );
}

#[test]
fn incremental_sync_counts_missing_response_as_skipped() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sh");

    let prices = [("600001", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1);
    assert_eq!(market_price_of(&conn, "inst-a"), Some(100000));
    assert_eq!(market_price_of(&conn, "inst-b"), None);
}

#[test]
fn incremental_sync_skips_unknown_market() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-ok", "600519", "stock", "CNY", "sh");
    insert_holding(
        &conn, "acc-2", "inst-unk", "NVDA", "stock", "USD", "unknown",
    );

    let prices = [("600519", Some(13_028_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 1, "市场未知应计入跳过");
    assert_eq!(market_price_of(&conn, "inst-unk"), None);
}

#[test]
fn incremental_sync_is_idempotent() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let prices = [("600519", Some(13_028_000))];
    let mut fetch = mock_fetch(&prices);
    let first = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    assert_eq!(first.synced, 1);

    let mut fetch = mock_fetch(&prices);
    let second = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    assert_eq!(second.synced, 1);

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
}

#[test]
fn incremental_sync_dedupes_same_instrument_across_accounts() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    seed_account(&conn, "acc-2", "账户-acc-2", "investment", "CNY", 0);
    insert_lot(&conn, "acc-2", "inst-sh", "CNY");

    let prices = [("600519", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(100000));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn incremental_sync_pulls_a_daily_ledgers_quotes_in_one_batch() {
    let conn = tauri_app_lib::test_support::open();
    let total = 233;
    for i in 0..total {
        let symbol = format!("{:06}", 600000 + i);
        insert_holding(
            &conn,
            &format!("acc-{i}"),
            &format!("inst-{i}"),
            &symbol,
            "stock",
            "CNY",
            "sh",
        );
    }

    let mut batch_sizes: Vec<usize> = Vec::new();
    let mut fetch = |queries: &[QuoteQuery]| {
        batch_sizes.push(queries.len());
        super::ready(Ok(quote_items_for(queries)))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, total);
    assert_eq!(
        batch_sizes,
        vec![total],
        "账本量级的行情标的应在一次批量报价请求内取回"
    );
}

#[test]
fn incremental_sync_hands_all_quote_queries_to_the_channel_in_one_call() {
    let conn = tauri_app_lib::test_support::open();
    let legacy_eastmoney_batch_size = 300;
    let total = legacy_eastmoney_batch_size + 5;
    for i in 0..total {
        let symbol = format!("{:06}", 600000 + i);
        insert_holding(
            &conn,
            &format!("acc-{i}"),
            &format!("inst-{i}"),
            &symbol,
            "stock",
            "CNY",
            "sh",
        );
    }

    let mut batch_sizes: Vec<usize> = Vec::new();
    let mut fetch = |queries: &[QuoteQuery]| {
        batch_sizes.push(queries.len());
        super::ready(Ok(quote_items_for(queries)))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, total);
    assert_eq!(batch_sizes, vec![total], "全部查询单元在一次通道调用内递交");
}

#[test]
fn swapping_quote_channel_keeps_prices_landing_without_source_key_in_orchestration() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let mut fetch = |queries: &[QuoteQuery]| {
        let items = queries
            .iter()
            .filter(|q| q.market == QuoteMarket::Sh && q.code == "600519")
            .map(|q| quote_item(q, "贵州茅台", Some(130_280), None))
            .collect();
        super::ready(Ok(items))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(
        market_price_of(&conn, "inst-sh"),
        Some(130_280),
        "换一个自己构造查询键的通道实现，价格照常落库（编排不含数据源键）"
    );
}

#[test]
fn incremental_sync_propagates_fetch_error() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    let mut fetch = |_: &[QuoteQuery]| super::ready(Err(AppError::Io("模拟网络失败".into())));
    let mut witness = WriteWitness::default();
    let err = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut witness,
    ))
    .unwrap_err();
    assert!(err.to_string().contains("模拟网络失败"));
    assert!(
        !witness.any_written(),
        "零写入失败：见证器保持清白（库未变，零证据零副作用，#1277）"
    );
}

#[test]
fn witness_survives_mid_run_failure_after_write() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(
        &conn,
        "acc-2",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let prices = [("600001", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let mut nav = |_: &str| super::ready(Err(AppError::Io("模拟净值网络失败".into())));
    let mut witness = WriteWitness::default();
    let err = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut witness,
    ))
    .unwrap_err();
    assert!(err.to_string().contains("模拟净值网络失败"));
    assert!(
        witness.any_written(),
        "实际写过之后中途失败：见证器应存活（#1277：失败 ≠ 未写过）"
    );
    assert_eq!(market_price_of(&conn, "inst-a"), Some(100000));
}

#[test]
fn witness_mirrors_result_any_written_on_success() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    let prices = [("600001", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let mut witness = WriteWitness::default();
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut witness,
    ))
    .unwrap();
    assert!(result.any_written());
    assert_eq!(
        witness.any_written(),
        result.any_written(),
        "有写入成功：见证器与结果统计同口径"
    );

    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );
    let watermark = beijing_today()
        .checked_sub_days(chrono::Days::new(7))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-fund",
            price_cents: 30000,
            currency_code: "CNY",
            priced_at: &watermark,
            nav_date: Some(&watermark),
            source: Some(EASTMONEY_PRICE_SOURCE),
        },
    )
    .unwrap();
    upsert_price_history(
        &conn,
        "inst-fund",
        &watermark,
        30000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let mut witness = WriteWitness::default();
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut witness,
    ))
    .unwrap();
    assert!(!result.any_written());
    assert_eq!(
        witness.any_written(),
        result.any_written(),
        "零写入成功（基金已最新 + 名称无变化）：见证器与结果统计同口径"
    );
}

fn no_nav(_: &str) -> FetchFuture<Vec<NavPoint>> {
    super::ready(Ok(vec![]))
}

fn no_name(_: &str) -> FetchFuture<String> {
    super::ready(Ok(String::new()))
}

fn no_confirm(_: &str) -> FetchFuture<bool> {
    super::ready(Ok(false))
}

fn no_progress(_progress: SyncProgress) {}

fn no_bulk() -> BulkFetchSurfaces {
    BulkFetchSurfaces::absent()
}

fn progress_recorder<'a>(log: &'a Mutex<Vec<(usize, usize)>>) -> impl FnMut(SyncProgress) + 'a {
    move |progress| {
        log.lock().unwrap().push((progress.done, progress.total));
    }
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

#[test]
fn sync_writes_no_price_history_quote_only() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    let prices = [("600519", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1, "无历史不中断同步");
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(100000));
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "同步不回填历史（归价格历史后台补全）");
}

#[test]
fn sync_lands_current_week_point_for_instrument_with_existing_history() {
    let today_date = beijing_today();
    let today = today_date.format("%Y-%m-%d").to_string();
    let old_date = (today_date - chrono::Duration::days(14))
        .format("%Y-%m-%d")
        .to_string();
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    upsert_price_history(
        &conn,
        "inst-sh",
        &old_date,
        90000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();
    let prices = [("600519", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(100000));
    assert_eq!(
        price_history_rows(&conn, "inst-sh"),
        vec![
            (old_date, 90000, "CNY".into()),
            (today, 100000, "CNY".into()),
        ],
        "当周采样点由现价刷新直落：上一周点与当周点并存",
    );
}

#[test]
fn week_key_matches_sqlite_week_start_column() {
    use crate::weekly::week_monday;
    use chrono::NaiveDate;

    let conn = tauri_app_lib::test_support::open();
    let mut d = NaiveDate::from_ymd_opt(2025, 1, 1).unwrap();
    let end = NaiveDate::from_ymd_opt(2027, 12, 31).unwrap();
    while d <= end {
        let iso = d.format("%Y-%m-%d").to_string();
        let rust_key = week_monday(d).format("%Y-%m-%d").to_string();
        let sql_key: String = conn
            .query_row(
                "SELECT date(?1, '-6 days', 'weekday 1')",
                params![iso],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(rust_key, sql_key, "{iso} 的周键两侧不一致");
        d += chrono::Duration::days(1);
    }
}

fn nav_points(points: &[(&str, f64)]) -> Vec<NavPoint> {
    points
        .iter()
        .map(|(d, n)| NavPoint {
            date: d.to_string(),
            nav: *n,
        })
        .collect()
}

fn mock_nav<'a>(
    history_by_code: &'a [(&'a str, Vec<NavPoint>)],
    requested: &'a Mutex<Vec<String>>,
) -> impl FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send + 'a {
    move |code: &str| {
        requested.lock().unwrap().push(code.to_string());
        super::ready(Ok(history_by_code
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, points)| points.clone())
            .unwrap_or_default()))
    }
}

#[test]
fn beijing_date_shifts_utc_by_plus_8h() {
    use chrono::TimeZone;
    let utc = chrono::Utc;
    let at = |y: i32, m: u32, d: u32, h: u32, min: u32, s: u32| {
        utc.with_ymd_and_hms(y, m, d, h, min, s).unwrap()
    };
    assert_eq!(
        beijing_date(at(2026, 1, 30, 15, 59, 59)),
        chrono::NaiveDate::from_ymd_opt(2026, 1, 30).unwrap(),
        "UTC 15:59:59 = 北京 23:59:59，仍属同一日"
    );
    assert_eq!(
        beijing_date(at(2026, 1, 30, 16, 0, 0)),
        chrono::NaiveDate::from_ymd_opt(2026, 1, 31).unwrap(),
        "UTC 16:00 即北京 00:00，进入次日"
    );
    assert_eq!(
        beijing_date(at(2026, 1, 30, 2, 0, 0)),
        chrono::NaiveDate::from_ymd_opt(2026, 1, 30).unwrap(),
        "UTC 上午 = 北京同日傍晚"
    );
}

fn fund_price_of(conn: &Connection, instrument_id: &str) -> Option<(i64, Option<String>)> {
    conn.query_row(
        "SELECT price_cents, nav_date FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .ok()
}

#[test]
fn fund_first_sync_without_nav_counts_skipped() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 1);
    assert_eq!(result.written, 0);
    assert_eq!(fund_price_of(&conn, "inst-fund"), None);
    assert_eq!(result.message, "已同步 0 只，跳过 1 只");
}

#[test]
fn fund_rows_without_real_code_skip_without_fetch() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-namefund",
        "华夏成长混合",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-2",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );

    let requested = Mutex::new(Vec::new());
    let mut fetch = mock_fetch(&[]);
    let mut nav = mock_nav(&[], &requested);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 2);
    assert_eq!(result.written, 0);
    assert!(
        requested.lock().unwrap().is_empty(),
        "不可拉取行不得发起净值请求"
    );
}

#[test]
fn fund_nav_fetch_error_propagates() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let mut fetch = mock_fetch(&[]);
    let mut nav = |_: &str| super::ready(Err(AppError::Io("模拟净值请求失败".into())));
    let err = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap_err();
    assert!(err.to_string().contains("模拟净值请求失败"));
}

#[test]
fn etf_holding_syncs_quote_only_without_history() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-etf", "510300", "etf", "CNY", "sh");

    let mut fetch = |_: &[QuoteQuery]| {
        super::ready(Ok(vec![QuoteItem {
            code: "510300".into(),
            name: "沪深300ETF华泰柏瑞".into(),
            price_cents: Some(46_340),
            price_date: None,
        }]))
    };

    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1, "ETF 持仓走行情通道计入同步成功");
    assert_eq!(result.skipped, 0);
    assert_eq!(result.written, 1, "实际落价计入写入（信号判定）");
    assert_eq!(
        market_price_of(&conn, "inst-etf"),
        Some(46_340),
        "ETF 报价按精度位换算（f1=3，三位小数报价）"
    );
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0, "同步不回填历史（归价格历史后台补全）");
}

#[test]
fn etf_holding_unknown_market_counts_skipped_without_requests() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn, "acc-1", "inst-etf", "510300", "etf", "CNY", "unknown",
    );

    let query_log = Mutex::new(Vec::new());
    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        super::ready(Ok(vec![]))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 1, "市场未知在行情分区内仍计入跳过");
    assert!(
        query_log.lock().unwrap().is_empty(),
        "不可查询行不得发起报价请求"
    );
    assert_eq!(market_price_of(&conn, "inst-etf"), None);
}

#[test]
fn three_type_partitions_roll_up_into_one_result() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-etf", "510300", "etf", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(
        &conn,
        "acc-3",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-4",
        "inst-namefund",
        "华夏成长混合",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-5",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-6",
        "inst-other",
        "稳稳地幸福",
        "other",
        "CNY",
        "unknown",
    );

    let query_log = Mutex::new(Vec::new());
    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        super::ready(Ok(vec![
            QuoteItem {
                code: "510300".into(),
                name: "沪深300ETF华泰柏瑞".into(),
                price_cents: Some(46_340),
                price_date: None,
            },
            QuoteItem {
                code: "600519".into(),
                name: "贵州茅台".into(),
                price_cents: Some(13_160_100),
                price_date: None,
            },
        ]))
    };
    let today_s = beijing_today().format("%Y-%m-%d").to_string();
    let history = [("110022", nav_points(&[(today_s.as_str(), 3.348)]))];
    let nav_requested = Mutex::new(Vec::new());
    let mut nav = mock_nav(&history, &nav_requested);

    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 3);
    assert_eq!(result.skipped, 3);
    assert_eq!(result.written, 3);
    assert_eq!(result.message, "已同步 3 只，跳过 3 只");
    assert_eq!(
        *query_log.lock().unwrap(),
        vec!["sh:510300,sh:600519".to_string()],
        "股票与 ETF 同走行情分区、同批查询（收集按 symbol 升序）；编排只递市场 + 代码"
    );
    let nav_codes: Vec<String> = nav_requested.lock().unwrap().clone();
    assert_eq!(
        nav_codes,
        vec!["110022".to_string()],
        "净值通道只发起基金请求"
    );
    assert_eq!(market_price_of(&conn, "inst-etf"), Some(46_340));
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13_160_100));
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some(today_s)))
    );
    assert_eq!(
        market_price_of(&conn, "inst-bond"),
        None,
        "无行情来源持仓不写价格"
    );
    assert_eq!(market_price_of(&conn, "inst-other"), None);
    assert_eq!(market_price_of(&conn, "inst-namefund"), None);
}

#[test]
fn us_stock_holding_syncs_quote_and_rerun_overwrites() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-usd",
        "inst-aapl",
        "AAPL",
        "stock",
        "USD",
        "nasdaq",
    );

    let query_log = Mutex::new(Vec::new());
    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        super::ready(Ok(vec![QuoteItem {
            code: "AAPL".into(),
            name: "苹果".into(),
            price_cents: Some(3_199_700),
            price_date: None,
        }]))
    };

    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    assert_eq!(result.synced, 1, "美股持仓应计入同步成功");
    assert_eq!(result.skipped, 0);
    assert_eq!(result.written, 1, "实际落价应计入写入（信号判定）");

    assert_eq!(*query_log.lock().unwrap(), vec!["nasdaq:AAPL".to_string()]);

    assert_eq!(market_price_of(&conn, "inst-aapl"), Some(3_199_700));
    let (price_ccy, source): (String, String) = conn
        .query_row(
            "SELECT currency_code, source FROM market_prices WHERE instrument_id='inst-aapl'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(price_ccy, "USD");
    assert_eq!(source, TENCENT_PRICE_SOURCE);

    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);

    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        super::ready(Ok(vec![QuoteItem {
            code: "AAPL".into(),
            name: "苹果".into(),
            price_cents: Some(3_200_000),
            price_date: None,
        }]))
    };
    tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    assert_eq!(
        market_price_of(&conn, "inst-aapl"),
        Some(3_200_000),
        "现价覆盖更新为最新价"
    );
}

#[test]
fn us_stock_holdings_route_exact_market_per_instrument() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-nq", "AAPL", "stock", "USD", "nasdaq");
    insert_holding(&conn, "acc-2", "inst-ny", "BABA", "stock", "USD", "nyse");
    insert_holding(&conn, "acc-3", "inst-am", "SPY", "stock", "USD", "amex");

    let query_log = Mutex::new(Vec::new());
    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        super::ready(Ok(vec![]))
    };
    tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        *query_log.lock().unwrap(),
        vec!["nasdaq:AAPL,nyse:BABA,amex:SPY".to_string()],
        "三市场持仓同批查询，各按精确市场路由（收集按 symbol 升序）"
    );
}

fn tencent_a_share_line(code: &str, name: &str, price: &str, timestamp: &str) -> String {
    let mut fields = [""; 88];
    fields[1] = name;
    fields[2] = code;
    fields[3] = price;
    fields[30] = timestamp;
    fields[61] = "GP-A";
    fields[82] = "CNY";
    format!("v_sh{code}=\"{}\";", fields.join("~"))
}

#[test]
fn production_quote_channel_requests_tencent_batch_endpoint() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600000", "stock", "CNY", "sh");

    let body = tencent_a_share_line("600000", "浦发银行", "9.07", "20260918161458");
    let gbk = encoding_rs::GBK.encode(&body).0.into_owned();
    let (url, requests) = super::spawn_header_capture_server(gbk);
    let mut channels = SyncFetchChannels::production_lane_on_pacer(
        Lane::Foreground,
        SyncFetchHosts {
            quote: vec![url],
            kline: vec![],
            fund_batch: vec![],
            fund_history: vec![],
            disclosure: vec![],
        },
        super::zero_pacer(),
    )
    .expect("生产束应可构造");

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(90_700));
    assert_eq!(instrument_name(&conn, "inst-sh"), "浦发银行");
    let captured = requests.lock().unwrap().clone();
    assert_eq!(
        captured.len(),
        1,
        "场内现价刷新一次批量报价请求（无其余请求）"
    );
    assert!(
        captured[0].starts_with("GET /q=sh600000 "),
        "批量报价打到腾讯路径段 q=<代码逗号串>，实际 {}",
        captured[0].lines().next().unwrap_or("")
    );
}

#[test]
fn production_fund_batch_channel_requests_sina_batch_endpoint() {
    let conn = tauri_app_lib::test_support::open();
    let today = beijing_today();
    let today_s = today.format("%Y-%m-%d").to_string();
    let yesterday = (today - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    seed_fund(&conn, "inst-fund", "000001", "陈旧名称");
    seed_fund_history(&conn, "inst-fund", &yesterday);

    let body =
        format!("var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,{today_s},24.0598\";\n");
    let gbk = encoding_rs::GBK.encode(&body).0.into_owned();
    let (url, requests) = super::spawn_header_capture_server(gbk);
    let mut channels = SyncFetchChannels::production_lane_on_pacer(
        Lane::Foreground,
        SyncFetchHosts {
            quote: vec![],
            kline: vec![],
            fund_batch: vec![url],
            fund_history: vec![],
            disclosure: vec![],
        },
        super::zero_pacer(),
    )
    .expect("生产束应可构造");

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1);
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((13_330, Some(today_s.clone()))),
        "现价由批量面的最新单位净值落库"
    );
    assert_eq!(instrument_name(&conn, "inst-fund"), "华夏成长混合A");
    let source: String = conn
        .query_row(
            "SELECT source FROM market_prices WHERE instrument_id='inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, SINA_PRICE_SOURCE, "价格来源标记为新来源值");

    let captured = requests.lock().unwrap().clone();
    assert_eq!(
        captured.len(),
        1,
        "场外基金现价刷新一次批量面请求（无其余请求）"
    );
    assert!(
        captured[0].starts_with("GET /list=f_000001 "),
        "批量面打到新浪路径段 list=f_<代码逗号串>，实际 {}",
        captured[0].lines().next().unwrap_or("")
    );
    assert!(
        captured[0]
            .to_ascii_lowercase()
            .contains("referer: https://finance.sina.com.cn/"),
        "批量面必须携带 Referer（缺省 403）：{}",
        captured[0]
    );
}

#[test]
fn production_fund_name_channel_falls_back_to_disclosure_host() {
    let conn = tauri_app_lib::test_support::open();
    let yesterday = (beijing_today() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    seed_fund(&conn, "inst-fund", "000001", "陈旧名称");

    let batch_body = "var hq_str_f_000001=\"\";\n";
    let gbk = encoding_rs::GBK.encode(batch_body).0.into_owned();
    let (batch_url, batch_requests) = super::spawn_header_capture_server(gbk);
    let disclosure_body = format!(
        r#"{{"sEcho":1,"iTotalRecords":1,"iTotalDisplayRecords":1,"aaData":[
{{"code":"000001","shortName":"披露权威名称","shareNetValue":"1.333","totalNetValue":"1.552","valuationDate":"{yesterday}","gainPer":"","yearSevenDayYieldRatePercent":""}}]}}"#
    );
    let (disclosure_url, disclosure_requests) = super::spawn_header_capture_server(disclosure_body);
    let nav_body = format!(
        r#"{{"result":{{"status":{{"code":0}},"data":{{"data":[
{{"fbrq":"{yesterday} 00:00:00","jjjz":"1.408","ljjz":"1.552"}}],"total_num":"1"}}}}}}"#
    );
    let (nav_url, _nav_requests) = super::spawn_header_capture_server(nav_body);

    let mut channels = SyncFetchChannels::production_lane_on_pacer(
        Lane::Foreground,
        SyncFetchHosts {
            quote: vec![],
            kline: vec![],
            fund_batch: vec![batch_url],
            fund_history: vec![nav_url],
            disclosure: vec![disclosure_url],
        },
        super::zero_pacer(),
    )
    .expect("生产束应可构造");

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(instrument_name(&conn, "inst-fund"), "披露权威名称");
    assert_eq!(result.renamed, 1);
    assert_eq!(result.synced, 1);
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((14_080, Some(yesterday))),
        "逐只历史面的近端净值落现价"
    );

    let captured = disclosure_requests.lock().unwrap().clone();
    assert_eq!(
        captured.len(),
        2,
        "判定门 1 次 + 名称兜底 1 次都应打到注入的披露面，实际：{captured:?}"
    );
    for head in &captured {
        let line = head.lines().next().unwrap_or_default();
        assert!(
            line.contains("/fund/disclose/getPublicFundJZInfoMore.do"),
            "披露请求应打到披露端点路径，实际 {line}"
        );
        assert!(
            line.contains("000001"),
            "披露请求应携带基金代码，实际 {line}"
        );
    }
    assert_eq!(
        batch_requests.lock().unwrap().len(),
        2,
        "批量取数面 1 次 + 名称闭包新浪臂 1 次都应打到注入的批量面"
    );
}

#[test]
fn production_confirm_channel_requests_disclosure_host() {
    let conn = tauri_app_lib::test_support::open();
    let today = beijing_today().format("%Y-%m-%d").to_string();
    seed_fund(&conn, "inst-fund", "000001", "陈旧名称");

    let batch_body =
        format!("var hq_str_f_000001=\"天弘余额宝货币,0.2229,0.824,,{today},6799.46\";\n");
    let gbk = encoding_rs::GBK.encode(&batch_body).0.into_owned();
    let (batch_url, _batch_requests) = super::spawn_header_capture_server(gbk);
    let disclosure_body = format!(
        r#"{{"sEcho":1,"iTotalRecords":1,"iTotalDisplayRecords":1,"aaData":[
{{"code":"000001","shortName":"天弘余额宝货币","shareNetValue":"","totalNetValue":"","valuationDate":"{today}","gainPer":"0.2274","yearSevenDayYieldRatePercent":"0.8230%"}}]}}"#
    );
    let (disclosure_url, disclosure_requests) = super::spawn_header_capture_server(disclosure_body);

    let mut channels = SyncFetchChannels::production_lane_on_pacer(
        Lane::Foreground,
        SyncFetchHosts {
            quote: vec![],
            kline: vec![],
            fund_batch: vec![batch_url],
            fund_history: vec![],
            disclosure: vec![disclosure_url],
        },
        super::zero_pacer(),
    )
    .expect("生产束应可构造");

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    let constant_unit_price: Option<i64> = conn
        .query_row(
            "SELECT constant_unit_price FROM instruments WHERE id='inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        constant_unit_price,
        Some(10_000),
        "货基自报形态确认即打标（恒定单位净值 1.0000）"
    );
    assert_eq!(fund_price_of(&conn, "inst-fund"), Some((10_000, None)));
    assert_eq!(instrument_name(&conn, "inst-fund"), "天弘余额宝货币");
    assert_eq!((result.synced, result.written, result.renamed), (1, 1, 1));

    let captured = disclosure_requests.lock().unwrap().clone();
    assert_eq!(
        captured.len(),
        1,
        "货基形态确认应恰好一次打到注入的披露面，实际：{captured:?}"
    );
    let line = captured[0].lines().next().unwrap_or_default();
    assert!(
        line.contains("/fund/disclose/getPublicFundJZInfoMore.do"),
        "判定请求应打到披露端点路径，实际 {line}"
    );
    assert!(
        line.contains("000001"),
        "判定请求应携带基金代码，实际 {line}"
    );
}

#[test]
fn quote_date_is_exchange_local_and_source_is_tencent() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-usd",
        "inst-aapl",
        "AAPL",
        "stock",
        "USD",
        "nasdaq",
    );
    let old = (beijing_today() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    upsert_price_history(&conn, "inst-aapl", &old, 100, "USD", EASTMONEY_PRICE_SOURCE).unwrap();

    let exchange_date = (beijing_today() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let mut fetch = |queries: &[QuoteQuery]| {
        super::ready(Ok(queries
            .iter()
            .map(|q| quote_item(q, "苹果", Some(3_361_300), Some(&exchange_date)))
            .collect()))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    assert_eq!(result.synced, 1);

    let source: String = conn
        .query_row(
            "SELECT source FROM market_prices WHERE instrument_id='inst-aapl'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(source, TENCENT_PRICE_SOURCE, "价格来源标记为新来源值");

    let (trade_date, price_cents, history_source): (String, i64, String) = conn
        .query_row(
            "SELECT trade_date, price_cents, source FROM price_history \
             WHERE instrument_id='inst-aapl' ORDER BY trade_date DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(trade_date, exchange_date, "当周采样日取交易所当地交易日");
    assert_eq!(price_cents, 3_361_300);
    assert_eq!(
        history_source, TENCENT_PRICE_SOURCE,
        "历史来源标记为新来源值"
    );
}

fn clear_position(conn: &Connection, instrument_id: &str) {
    conn.execute(
        "UPDATE security_lots SET remaining_quantity=0 WHERE instrument_id=?1",
        params![instrument_id],
    )
    .unwrap();
}

#[test]
fn incremental_sync_includes_cleared_instrument() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-held", "600519", "stock", "CNY", "sh");
    insert_holding(
        &conn,
        "acc-2",
        "inst-cleared",
        "000001",
        "stock",
        "CNY",
        "sz",
    );
    clear_position(&conn, "inst-cleared");

    let prices = [("600519", Some(13_028_000)), ("000001", Some(117_300))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        result.synced, 2,
        "清仓标的与持仓标的同批收集（覆盖面不再以持仓为界）"
    );
    assert_eq!(result.skipped, 0);
    assert_eq!(
        market_price_of(&conn, "inst-cleared"),
        Some(117300),
        "清仓标的恢复刷现价"
    );
}

#[test]
fn incremental_sync_includes_never_traded_instrument() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument(&conn, "inst-archived", "600000", "浦发银行", "CNY", "sh");

    let prices = [("600000", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(result.synced, 1, "未交易标的首次同步照常刷价");
    assert_eq!(market_price_of(&conn, "inst-archived"), Some(100000));
}

#[test]
fn incremental_sync_refreshes_names_from_quote_batch() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument(&conn, "inst-stale", "600519", "贵州茅台旧名", "CNY", "sh");
    let version_before: i64 = conn
        .query_row(
            "SELECT version FROM instruments WHERE id='inst-stale'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let prices = [("600519", Some(13_028_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    let (name, version): (String, i64) = conn
        .query_row(
            "SELECT name, version FROM instruments WHERE id='inst-stale'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        name, "名称-600519",
        "名称被数据源权威名称覆盖（随用随修 + 同步随行刷新）"
    );
    assert_eq!(version, version_before + 1, "名称刷新虚增 version 一次");
    assert_eq!(result.renamed, 1, "名称刷新计入 renamed 统计");
    assert!(
        result.any_written(),
        "仅名称刷新也应视为实际写入（信号判定）"
    );
}

#[test]
fn incremental_sync_skips_name_write_when_unchanged() {
    let conn = tauri_app_lib::test_support::open();
    seed_instrument(&conn, "inst-fresh", "600519", "名称-600519", "CNY", "sh");
    let version_before: i64 = conn
        .query_row(
            "SELECT version FROM instruments WHERE id='inst-fresh'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let prices = [("600519", Some(13_028_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    let version: i64 = conn
        .query_row(
            "SELECT version FROM instruments WHERE id='inst-fresh'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version, version_before, "名称未变化不写库");
    assert_eq!(result.renamed, 0);
}

#[test]
fn fund_name_refresh_via_detail_lookup() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );
    let version_before: i64 = conn
        .query_row(
            "SELECT version FROM instruments WHERE id='inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let mut fund_name = |code: &str| {
        assert_eq!(code, "110022");
        super::ready(Ok("易方达优质精选混合".to_string()))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut fund_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    let (name, version): (String, i64) = conn
        .query_row(
            "SELECT name, version FROM instruments WHERE id='inst-fund'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name, "易方达优质精选混合", "基金名称经详情通道随行刷新");
    assert_eq!(version, version_before + 1);
    assert_eq!(result.renamed, 1);
}

#[test]
fn fund_name_refresh_degrades_deterministic_not_found_to_skip() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "002503",
        "fund",
        "CNY",
        "unknown",
    );
    let version_before: i64 = conn
        .query_row(
            "SELECT version FROM instruments WHERE id='inst-fund'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let mut fund_name = |code: &str| {
        assert_eq!(code, "002503");
        super::ready(Err(AppError::codedp(
            "sync.fund-not-found",
            "查无基金代码 002503，请核对后重试",
            &["002503"],
        )))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut fund_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .expect("确定性查无不得中断整次同步");

    let (name, version): (String, i64) = conn
        .query_row(
            "SELECT name, version FROM instruments WHERE id='inst-fund'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name, "名称-002503", "确定性查无时保留原名称");
    assert_eq!(version, version_before, "未取到名称不写库、不虚增 version");
    assert_eq!(result.renamed, 0);
}

#[test]
fn orchestration_does_not_sniff_fund_not_found_code_string() {
    let source = include_str!("../incremental.rs");
    assert!(
        !source.contains("sync.fund-not-found"),
        "编排（incremental）不得出现查无此码的错误码字面量——名称刷新降级改回 \
         is_code 字符串嗅探即红；识别收口在取数侧谓词 is_fund_not_found（spec #1674）"
    );
}

#[test]
fn fund_name_refresh_still_propagates_network_failure() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let mut fund_name = |_: &str| super::ready(Err(AppError::Io("HTTP 请求失败: 连接超时".into())));
    let error = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut fund_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .expect_err("网络失败仍应上抛中断");

    assert!(
        !error.is_code("sync.fund-not-found"),
        "网络失败不得被误降级为跳过: {error:?}"
    );
}

#[test]
fn fund_name_lookup_skips_name_as_code_rows_and_empty_names() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-namefund",
        "华夏成长混合",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-2",
        "inst-coded",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let name_requests = Mutex::new(Vec::new());
    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let mut fund_name = |code: &str| {
        name_requests.lock().unwrap().push(code.to_string());
        super::ready(Ok(String::new()))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut fund_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        *name_requests.lock().unwrap(),
        vec!["110022".to_string()],
        "仅 6 位代码的基金行发起名称查询"
    );
    assert_eq!(result.renamed, 0, "空名称（未取到）不落库");
}

#[test]
fn any_written_covers_name_only_and_price_only_writes() {
    use crate::SyncInstrumentInfoResult;
    let base = |written: usize, renamed: usize| SyncInstrumentInfoResult {
        synced: 1,
        skipped: 0,
        message: String::new(),
        written,
        renamed,
        bulk_degraded: false,
        bulk_gaps: 0,
    };
    assert!(base(1, 0).any_written(), "价格写入");
    assert!(
        base(0, 1).any_written(),
        "仅名称刷新（基金已最新 + 名称变化）"
    );
    assert!(
        !base(0, 0).any_written(),
        "零变化（全部跳过/已是最新且名称无变化）"
    );
}

#[test]
fn progress_sequence_total_first_then_per_instrument_advance() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sz");

    let events = Mutex::new(Vec::new());
    let mut fetch = |queries: &[QuoteQuery]| {
        events
            .lock()
            .unwrap()
            .push(format!("fetch:{}", query_log_line(queries)));
        super::ready(Ok(quote_items_for(queries)))
    };
    let mut progress = |progress: SyncProgress| {
        events
            .lock()
            .unwrap()
            .push(format!("progress:{}/{}", progress.done, progress.total));
    };
    tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "progress:0/2".to_string(),
            "fetch:sh:600001,sz:600002".to_string(),
            "progress:1/2".to_string(),
            "progress:2/2".to_string(),
        ],
        "total 先于首个请求发出；报价批完成后逐标的推进一格"
    );
}

#[test]
fn progress_denominator_counts_channel_capable_instruments_only() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    insert_holding(
        &conn, "acc-2", "inst-unk", "NVDA", "stock", "USD", "unknown",
    );
    insert_holding(
        &conn,
        "acc-3",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-4",
        "inst-namefund",
        "华夏成长混合",
        "fund",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-5",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let requested = Mutex::new(Vec::new());
    let mut fetch = mock_fetch(&[("600519", Some(100_000))]);
    let today_s = beijing_today().format("%Y-%m-%d").to_string();
    let fund_history = [("110022", nav_points(&[(today_s.as_str(), 3.3)]))];
    let mut nav = mock_nav(&fund_history, &requested);
    let mut fund_name = |code: &str| super::ready(Ok(format!("权威-{code}")));
    let log = Mutex::new(Vec::new());
    let mut progress = progress_recorder(&log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut fund_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        *log.lock().unwrap(),
        vec![(0, 2), (1, 2), (2, 2)],
        "分母只含有通道标的（行情 1 + 有码基金 1），跳过行不进分母、零推进"
    );
    assert_eq!(
        result.skipped, 3,
        "市场未知 + 债券 + 名称充代码计入跳过统计（与分母口径同源）"
    );
    assert_eq!(result.synced, 2);
}

#[test]
fn progress_advances_even_when_quote_invalid_or_missing() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-ok", "600001", "stock", "CNY", "sh");
    insert_holding(
        &conn,
        "acc-2",
        "inst-suspended",
        "600002",
        "stock",
        "CNY",
        "sz",
    );
    insert_holding(
        &conn,
        "acc-3",
        "inst-missing",
        "600003",
        "stock",
        "CNY",
        "sh",
    );

    let prices = [("600001", Some(100_000)), ("600002", None)];
    let mut fetch = mock_fetch(&prices);
    let log = Mutex::new(Vec::new());
    let mut progress = progress_recorder(&log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        *log.lock().unwrap(),
        vec![(0, 3), (1, 3), (2, 3), (3, 3)],
        "停牌/查询无果的行情标的照常推进"
    );
    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 2);
}

#[test]
fn fund_up_to_date_still_advances_progress() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );
    let watermark = beijing_today()
        .checked_sub_days(chrono::Days::new(7))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string();
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-fund",
            price_cents: 30000,
            currency_code: "CNY",
            priced_at: &watermark,
            nav_date: Some(&watermark),
            source: Some(EASTMONEY_PRICE_SOURCE),
        },
    )
    .unwrap();

    upsert_price_history(
        &conn,
        "inst-fund",
        &watermark,
        30000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let mut fund_name = |code: &str| super::ready(Ok(format!("权威-{code}")));
    let log = Mutex::new(Vec::new());
    let mut progress = progress_recorder(&log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut fund_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        *log.lock().unwrap(),
        vec![(0, 1), (1, 1)],
        "「已是最新」的基金推进一格，进度不卡在 0%"
    );
    assert_eq!(result.synced, 1);
    assert_eq!(result.written, 0);
}

#[test]
fn fund_progress_advances_after_nav_and_name_complete() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-fund",
        "110022",
        "fund",
        "CNY",
        "unknown",
    );

    let events = Mutex::new(Vec::new());
    let mut fetch = mock_fetch(&[]);
    let today_s = beijing_today().format("%Y-%m-%d").to_string();
    let mut nav = |code: &str| {
        events.lock().unwrap().push(format!("nav:{code}"));
        let date = today_s.clone();
        super::ready(Ok(nav_points(&[(date.as_str(), 3.3)])))
    };
    let mut fund_name = |code: &str| {
        events.lock().unwrap().push(format!("name:{code}"));
        super::ready(Ok(format!("权威-{code}")))
    };
    let mut progress = |progress: SyncProgress| {
        events
            .lock()
            .unwrap()
            .push(format!("progress:{}/{}", progress.done, progress.total));
    };
    tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut fund_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        *events.lock().unwrap(),
        vec![
            "progress:0/1".to_string(),
            "nav:110022".to_string(),
            "name:110022".to_string(),
            "progress:1/1".to_string(),
        ],
        "先发 total；净值与名称都完成后才推进该基金的一格"
    );
}

#[test]
fn progress_not_emitted_for_empty_library() {
    let conn = tauri_app_lib::test_support::open();
    let mut fetch = mock_fetch(&[]);
    let log = Mutex::new(Vec::new());
    let mut progress = progress_recorder(&log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert!(log.lock().unwrap().is_empty(), "空库不发 total");
    assert_eq!(result.message, "暂无标的可同步");
}

#[test]
fn progress_not_emitted_when_no_channel_capable_instrument() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-1",
        "inst-bond",
        "019547",
        "bond",
        "CNY",
        "unknown",
    );
    insert_holding(
        &conn,
        "acc-2",
        "inst-namefund",
        "华夏成长混合",
        "fund",
        "CNY",
        "unknown",
    );

    let mut fetch = mock_fetch(&[]);
    let mut nav = no_nav;
    let log = Mutex::new(Vec::new());
    let mut progress = progress_recorder(&log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert!(log.lock().unwrap().is_empty(), "分母为 0 不发 total");
    assert_eq!(result.skipped, 2);
}

fn seed_fund(conn: &Connection, instrument_id: &str, code: &str, name: &str) {
    insert_holding(
        conn,
        &format!("acc-{instrument_id}"),
        instrument_id,
        code,
        "fund",
        "CNY",
        "unknown",
    );
    conn.execute(
        "UPDATE instruments SET name=?1 WHERE id=?2",
        params![name, instrument_id],
    )
    .unwrap();
}

fn seed_fund_history(conn: &Connection, instrument_id: &str, watermark: &str) {
    upsert_price_history(
        conn,
        instrument_id,
        watermark,
        30000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();
    upsert_market_price(
        conn,
        &MarketPriceWrite {
            instrument_id,
            price_cents: 30000,
            currency_code: "CNY",
            priced_at: watermark,
            nav_date: Some(watermark),
            source: Some(EASTMONEY_PRICE_SOURCE),
        },
    )
    .unwrap();
}

fn instrument_name(conn: &Connection, instrument_id: &str) -> String {
    conn.query_row(
        "SELECT name FROM instruments WHERE id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

fn instrument_version(conn: &Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT version FROM instruments WHERE id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

#[derive(Clone, Default)]
struct QuoteChannelCalls {
    quote: Arc<AtomicUsize>,
    kline: Arc<AtomicUsize>,
}

impl QuoteChannelCalls {
    fn total(&self) -> usize {
        self.quote.load(Ordering::SeqCst) + self.kline.load(Ordering::SeqCst)
    }
}

fn fund_channels(
    quote_calls: QuoteChannelCalls,
    fetch_nav_history: FetchNavHistory,
    fetch_fund_name: FetchFundName,
    confirm_money_fund_form: FetchMoneyFundForm,
    bulk: BulkFetchSurfaces,
) -> SyncFetchChannels {
    SyncFetchChannels {
        fetch_quotes: Box::new({
            let calls = quote_calls.quote.clone();
            move |_: &[QuoteQuery]| {
                calls.fetch_add(1, Ordering::SeqCst);
                unreachable!("用例现场无行情通道标的")
            }
        }),
        fetch_kline: Box::new({
            let calls = quote_calls.kline.clone();
            move |_: &QuoteQuery| {
                calls.fetch_add(1, Ordering::SeqCst);
                unreachable!("用例现场无行情通道标的")
            }
        }),
        fetch_nav_history,
        fetch_fund_name,
        confirm_money_fund_form,
        bulk,
    }
}

fn batch_stub(names: FundNameDictionary, nav: FundNavTable) -> FetchFundBatch {
    Box::new(move |_codes: &[String]| {
        let names = names.clone();
        let nav = nav.clone();
        Box::pin(async move { Ok(FundBatch { names, nav }) })
    })
}

fn counting_batch_stub(
    calls: Arc<AtomicUsize>,
    names: FundNameDictionary,
    nav: FundNavTable,
) -> FetchFundBatch {
    Box::new(move |_codes: &[String]| {
        calls.fetch_add(1, Ordering::SeqCst);
        let names = names.clone();
        let nav = nav.clone();
        Box::pin(async move { Ok(FundBatch { names, nav }) })
    })
}

fn bulk_surfaces(
    funds: FetchFundBatch,
    circuit: Arc<Mutex<BulkFetchCircuit>>,
) -> BulkFetchSurfaces {
    BulkFetchSurfaces { funds, circuit }
}

fn batch_surfaces(names: FundNameDictionary, nav: FundNavTable) -> BulkFetchSurfaces {
    bulk_surfaces(
        batch_stub(names, nav),
        Arc::new(Mutex::new(BulkFetchCircuit::new())),
    )
}

fn counting_nav(calls: Arc<AtomicUsize>, date: String, nav: f64) -> FetchNavHistory {
    Box::new(move |_: &str| {
        let calls = calls.clone();
        let date = date.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(nav_points(&[(date.as_str(), nav)]))
        })
    })
}

fn empty_nav(calls: Arc<AtomicUsize>) -> FetchNavHistory {
    Box::new(move |_: &str| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        })
    })
}

fn counting_name(calls: Arc<AtomicUsize>) -> FetchFundName {
    Box::new(move |code: &str| {
        let calls = calls.clone();
        let name = format!("权威名称-{code}");
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(name)
        })
    })
}

#[derive(Debug, PartialEq, Eq)]
struct RequestCounts {
    quote: usize,
    per_fund_nav: usize,
    per_fund_name: usize,
    bulk_funds: usize,
}

#[test]
fn bulk_surfaces_pin_daily_sync_request_count_to_a_constant() {
    let today_date = beijing_today();
    let yesterday = (today_date - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let today = today_date.format("%Y-%m-%d").to_string();
    let mut observed = Vec::new();

    for fund_count in [4usize, 232] {
        let conn = tauri_app_lib::test_support::open();
        let mut codes = Vec::new();
        for i in 0..fund_count {
            let code = format!("{:06}", 100000 + i);
            let instrument_id = format!("inst-fund-{i:03}");
            let name = if i == 0 {
                "陈旧名称".to_string()
            } else {
                format!("权威名称-{code}")
            };
            seed_fund(&conn, &instrument_id, &code, &name);
            seed_fund_history(&conn, &instrument_id, &yesterday);
            codes.push(code);
        }
        seed_instrument(&conn, "inst-bond", "BOND-X", "自建债券", "CNY", "unknown");
        conn.execute(
            "UPDATE instruments SET instrument_type='bond' WHERE id='inst-bond'",
            [],
        )
        .unwrap();

        let versions_before: Vec<i64> = (0..fund_count)
            .map(|i| instrument_version(&conn, &format!("inst-fund-{i:03}")))
            .collect();
        let bulletin_names: FundNameDictionary = codes
            .iter()
            .map(|code| (code.clone(), format!("权威名称-{code}")))
            .collect();
        let bulletin_nav: FundNavTable = codes
            .iter()
            .map(|code| {
                (
                    code.clone(),
                    BulkNavPoint {
                        date: today.clone(),
                        nav: 3.5,
                    },
                )
            })
            .collect();

        let quote_calls = QuoteChannelCalls::default();
        let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
        let per_fund_name_calls = Arc::new(AtomicUsize::new(0));
        let bulk_calls = Arc::new(AtomicUsize::new(0));
        let mut channels = fund_channels(
            quote_calls.clone(),
            {
                let calls = per_fund_nav_calls.clone();
                Box::new(move |_: &str| {
                    let calls = calls.clone();
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(vec![])
                    })
                })
            },
            {
                let calls = per_fund_name_calls.clone();
                Box::new(move |_: &str| {
                    let calls = calls.clone();
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Ok(String::new())
                    })
                })
            },
            Box::new(no_confirm),
            bulk_surfaces(
                counting_batch_stub(bulk_calls.clone(), bulletin_names, bulletin_nav),
                Arc::new(Mutex::new(BulkFetchCircuit::new())),
            ),
        );

        let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
            &conn,
            &mut channels,
            &mut |_| {},
            &mut WriteWitness::default(),
        ))
        .unwrap();

        let counts = RequestCounts {
            quote: quote_calls.total(),
            per_fund_nav: per_fund_nav_calls.load(Ordering::SeqCst),
            per_fund_name: per_fund_name_calls.load(Ordering::SeqCst),
            bulk_funds: bulk_calls.load(Ordering::SeqCst),
        };
        assert_eq!(
            counts,
            RequestCounts {
                quote: 0,
                per_fund_nav: 0,
                per_fund_name: 0,
                bulk_funds: 1,
            },
            "取数请求数应为常数级（批量面 1 次），实际 {counts:?}"
        );
        observed.push(counts);

        assert_eq!(result.synced, fund_count);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.written, fund_count, "当周新净值由批量面落现价");
        assert_eq!(
            fund_price_of(&conn, "inst-fund-000"),
            Some((35000, Some(today.clone()))),
            "现价由批量面的最新单位净值落库"
        );
        assert_eq!(result.renamed, 1, "只有名称实际变化的那只落库");
        assert_eq!(result.bulk_gaps, 0);
        assert!(!result.bulk_degraded, "批量面命中即不带降级事实");

        assert_eq!(instrument_name(&conn, "inst-fund-000"), "权威名称-100000");
        assert_eq!(
            instrument_version(&conn, "inst-fund-000"),
            versions_before[0] + 1,
            "名称实际变化才落库（版本 +1）"
        );
        if fund_count > 1 {
            assert_eq!(instrument_name(&conn, "inst-fund-001"), "权威名称-100001");
            assert_eq!(
                instrument_version(&conn, "inst-fund-001"),
                versions_before[1],
                "零名称变化不写库、不虚增版本"
            );
        }
    }

    assert_eq!(
        observed[0], observed[1],
        "请求数与标的数无关：4 只与 232 只账本的取数面调用次数相同"
    );
}

#[test]
fn bulk_surface_failure_falls_back_per_instrument_and_counts_toward_the_circuit() {
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let yesterday = (beijing_today() - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let conn = tauri_app_lib::test_support::open();
    let mut codes = Vec::new();
    for i in 0..3 {
        let code = format!("{:06}", 100000 + i);
        seed_fund(&conn, &format!("inst-fund-{i}"), &code, "陈旧名称");
        seed_fund_history(&conn, &format!("inst-fund-{i}"), &yesterday);
        codes.push(code);
    }

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let per_fund_name_calls = Arc::new(AtomicUsize::new(0));
    let bulk_calls = Arc::new(AtomicUsize::new(0));
    let circuit = Arc::new(Mutex::new(BulkFetchCircuit::new()));
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        counting_nav(per_fund_nav_calls.clone(), today.clone(), 5.0),
        counting_name(per_fund_name_calls.clone()),
        Box::new(no_confirm),
        bulk_surfaces(
            {
                let calls = bulk_calls.clone();
                Box::new(move |_: &[String]| {
                    let calls = calls.clone();
                    Box::pin(async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        Err(AppError::Io("场外基金批量面被风控拦截".into()))
                    })
                })
            },
            circuit.clone(),
        ),
    );

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        bulk_calls.load(Ordering::SeqCst),
        1,
        "批量面每次同步最多试一次"
    );
    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        3,
        "逐只净值通道兜底（每只一页增量）"
    );
    assert_eq!(
        per_fund_name_calls.load(Ordering::SeqCst),
        3,
        "逐只名称通道兜底"
    );
    assert!(result.bulk_degraded, "回退逐只通道即带出降级事实");
    assert_eq!(result.synced, 3);
    for (i, code) in codes.iter().enumerate() {
        let instrument_id = format!("inst-fund-{i}");
        assert_eq!(
            fund_price_of(&conn, &instrument_id),
            Some((50000, Some(today.clone()))),
            "第 {code} 只价格应由逐只通道补齐"
        );
        assert_eq!(
            instrument_name(&conn, &instrument_id),
            format!("权威名称-{code}")
        );
    }
    assert_eq!(
        circuit.lock().unwrap().consecutive_failures(),
        1,
        "失败记入跨同步记忆"
    );
}

#[test]
fn bulk_coverage_gaps_fall_back_per_item_without_tripping_the_circuit() {
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    let covered = [("inst-fund-0", "100000"), ("inst-fund-1", "100001")];
    let gap = ("inst-fund-2", "002503");
    for (instrument_id, code) in covered.iter().chain(std::iter::once(&gap)) {
        seed_fund(&conn, instrument_id, code, "陈旧名称");
        seed_fund_history(&conn, instrument_id, &today);
    }

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let per_fund_name_calls = Arc::new(AtomicUsize::new(0));
    let bulk_calls = Arc::new(AtomicUsize::new(0));
    let circuit = Arc::new(Mutex::new(BulkFetchCircuit::new()));
    let bulletin_names: FundNameDictionary = covered
        .iter()
        .map(|(_, code)| (code.to_string(), format!("权威名称-{code}")))
        .collect();
    let bulletin_nav: FundNavTable = covered
        .iter()
        .map(|(_, code)| {
            (
                code.to_string(),
                BulkNavPoint {
                    date: today.clone(),
                    nav: 3.0,
                },
            )
        })
        .collect();

    let run = || {
        let mut channels = fund_channels(
            QuoteChannelCalls::default(),
            counting_nav(per_fund_nav_calls.clone(), today.clone(), 3.0),
            counting_name(per_fund_name_calls.clone()),
            Box::new(no_confirm),
            bulk_surfaces(
                counting_batch_stub(
                    bulk_calls.clone(),
                    bulletin_names.clone(),
                    bulletin_nav.clone(),
                ),
                circuit.clone(),
            ),
        );
        tauri::async_runtime::block_on(do_incremental_sync_channels(
            &conn,
            &mut channels,
            &mut |_| {},
            &mut WriteWitness::default(),
        ))
        .unwrap()
    };

    let result = run();

    assert_eq!(bulk_calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        1,
        "只有缺口那只走逐只净值通道"
    );
    assert_eq!(
        per_fund_name_calls.load(Ordering::SeqCst),
        1,
        "缺口那只的名称也走逐只通道兜底"
    );
    assert_eq!(result.bulk_gaps, 1, "缺口计入缺口统计");
    assert!(!result.bulk_degraded, "缺口不是失败：不带降级事实");
    assert_eq!(
        circuit.lock().unwrap().consecutive_failures(),
        0,
        "缺口不触发熔断"
    );
    assert_eq!(instrument_name(&conn, gap.0), "权威名称-002503");
    assert_eq!(
        fund_price_of(&conn, gap.0),
        Some((30000, Some(today.clone())))
    );
    assert_eq!(
        price_history_rows(&conn, gap.0),
        vec![(today.clone(), 30000, "CNY".into())],
        "缺口标的的当周采样照常落库"
    );

    let second = run();
    assert_eq!(
        bulk_calls.load(Ordering::SeqCst),
        2,
        "缺口不熔断：下一次同步照试批量面"
    );
    assert_eq!(per_fund_nav_calls.load(Ordering::SeqCst), 2);
    assert_eq!(second.bulk_gaps, 1);
    assert!(!second.bulk_degraded);
}

#[test]
fn bulk_surfaces_stay_disabled_after_threshold_failures_and_half_open_after_the_period() {
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-fund-0", "100000", "权威名称-100000");
    seed_fund_history(&conn, "inst-fund-0", &today);

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let bulk_calls = Arc::new(AtomicUsize::new(0));
    let circuit = Arc::new(Mutex::new(BulkFetchCircuit::new()));
    let failing = Arc::new(AtomicUsize::new(0));
    let bulletin_names: FundNameDictionary =
        [("100000".to_string(), "权威名称-100000".to_string())]
            .into_iter()
            .collect();
    let bulletin_nav: FundNavTable = [(
        "100000".to_string(),
        BulkNavPoint {
            date: today.clone(),
            nav: 3.0,
        },
    )]
    .into_iter()
    .collect();

    let run = || {
        let mut channels = fund_channels(
            QuoteChannelCalls::default(),
            empty_nav(per_fund_nav_calls.clone()),
            Box::new(|_: &str| super::ready(Ok(String::new()))),
            Box::new(no_confirm),
            bulk_surfaces(
                {
                    let calls = bulk_calls.clone();
                    let failing = failing.clone();
                    let names = bulletin_names.clone();
                    let table = bulletin_nav.clone();
                    Box::new(move |_: &[String]| {
                        calls.fetch_add(1, Ordering::SeqCst);
                        let failing = failing.clone();
                        let names = names.clone();
                        let table = table.clone();
                        Box::pin(async move {
                            if failing.load(Ordering::SeqCst) < BULK_FAILURE_THRESHOLD as usize {
                                Err(AppError::Io("场外基金批量面连续失败".into()))
                            } else {
                                Ok(FundBatch { names, nav: table })
                            }
                        })
                    })
                },
                circuit.clone(),
            ),
        );
        tauri::async_runtime::block_on(do_incremental_sync_channels(
            &conn,
            &mut channels,
            &mut |_| {},
            &mut WriteWitness::default(),
        ))
        .unwrap()
    };

    for round in 1..=BULK_FAILURE_THRESHOLD {
        let result = run();
        assert!(
            result.bulk_degraded,
            "round {round}: bulk_calls={} per_fund_nav={} synced={} skipped={}",
            bulk_calls.load(Ordering::SeqCst),
            per_fund_nav_calls.load(Ordering::SeqCst),
            result.synced,
            result.skipped
        );
        failing.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            bulk_calls.load(Ordering::SeqCst),
            round as usize,
            "第 {round} 次同步应各试一次批量面"
        );
    }
    assert!(circuit.lock().unwrap().is_disabled(Instant::now()));
    let degraded_fallback_calls = per_fund_nav_calls.load(Ordering::SeqCst);
    assert_eq!(
        degraded_fallback_calls, BULK_FAILURE_THRESHOLD as usize,
        "阈值内每轮都逐只兜底"
    );

    let result = run();
    assert_eq!(
        bulk_calls.load(Ordering::SeqCst),
        BULK_FAILURE_THRESHOLD as usize,
        "停用期内不再撞批量面"
    );
    assert!(result.bulk_degraded, "停用期同样是降级形态");
    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        degraded_fallback_calls + 1
    );

    circuit.lock().unwrap().record_failure(
        Instant::now()
            .checked_sub(BULK_DISABLE_PERIOD + Duration::from_secs(1))
            .expect("测试时钟应可回拨"),
    );
    let result = run();
    assert_eq!(
        bulk_calls.load(Ordering::SeqCst),
        BULK_FAILURE_THRESHOLD as usize + 1,
        "到期先半开试一次"
    );
    assert!(!result.bulk_degraded);
    assert!(
        !circuit.lock().unwrap().is_disabled(Instant::now()),
        "半开试探成功即解除停用"
    );
    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        degraded_fallback_calls + 1,
        "半开成功后不再需要逐只兜底"
    );

    let result = run();
    assert_eq!(
        bulk_calls.load(Ordering::SeqCst),
        BULK_FAILURE_THRESHOLD as usize + 2,
        "恢复后每轮同步照常走批量面"
    );
    assert!(!result.bulk_degraded);
}

#[test]
fn bulk_nav_point_of_the_current_week_lands_price_and_weekly_sample_without_per_instrument_requests()
 {
    let today_date = beijing_today();
    let watermark_date = today_date - chrono::Duration::days(1);
    let today = today_date.format("%Y-%m-%d").to_string();
    let watermark = watermark_date.format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-fund-0", "100000", "权威名称-100000");
    seed_fund_history(&conn, "inst-fund-0", &watermark);

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        empty_nav(per_fund_nav_calls.clone()),
        counting_name(Arc::new(AtomicUsize::new(0))),
        Box::new(no_confirm),
        bulk_surfaces(
            batch_stub(
                FundNameDictionary::from([("100000".to_string(), "权威名称-100000".to_string())]),
                FundNavTable::from([(
                    "100000".to_string(),
                    BulkNavPoint {
                        date: today.clone(),
                        nav: 3.5,
                    },
                )]),
            ),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    );

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        0,
        "当周新净值由批量取数落库：逐只净值通道不应被触达"
    );
    assert_eq!(result.written, 1);
    assert_eq!(
        fund_price_of(&conn, "inst-fund-0"),
        Some((35000, Some(today.clone())))
    );
    let expected =
        if crate::weekly::week_monday(watermark_date) == crate::weekly::week_monday(today_date) {
            vec![(today.clone(), 35000, "CNY".into())]
        } else {
            vec![
                (watermark.clone(), 30000, "CNY".into()),
                (today.clone(), 35000, "CNY".into()),
            ]
        };
    assert_eq!(price_history_rows(&conn, "inst-fund-0"), expected);
}

#[test]
fn bulk_week_gap_beyond_one_week_falls_back_per_instrument_to_fill_missing_weeks() {
    let today_date = beijing_today();
    let watermark_date = today_date - chrono::Duration::days(14);
    let today = today_date.format("%Y-%m-%d").to_string();
    let watermark = watermark_date.format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-fund-0", "100000", "权威名称-100000");
    seed_fund_history(&conn, "inst-fund-0", &watermark);

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let requested = Arc::new(Mutex::new(Vec::new()));
    let requested_clone = requested.clone();
    let per_fund_calls = per_fund_nav_calls.clone();
    let page_date = today.clone();
    let stale_date = (watermark_date - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        Box::new(move |code: &str| {
            per_fund_calls.fetch_add(1, Ordering::SeqCst);
            requested_clone.lock().unwrap().push(code.to_string());
            let page_date = page_date.clone();
            let stale_date = stale_date.clone();
            super::ready(Ok(nav_points(&[
                (stale_date.as_str(), 2.0),
                (page_date.as_str(), 3.5),
            ])))
        }),
        counting_name(Arc::new(AtomicUsize::new(0))),
        Box::new(no_confirm),
        bulk_surfaces(
            batch_stub(
                FundNameDictionary::from([("100000".to_string(), "权威名称-100000".to_string())]),
                FundNavTable::from([(
                    "100000".to_string(),
                    BulkNavPoint {
                        date: today.clone(),
                        nav: 3.5,
                    },
                )]),
            ),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    );

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        1,
        "水位落后超过一周即逐只补齐缺失周点"
    );
    assert_eq!(
        requested.lock().unwrap().as_slice(),
        ["100000"],
        "逐只回退即新浪全历史通道：每只一次请求、不携带窗口参数"
    );
    assert_eq!(result.written, 1);
    assert_eq!(
        fund_price_of(&conn, "inst-fund-0"),
        Some((35000, Some(today.clone())))
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund-0"),
        vec![
            (watermark, 30000, "CNY".into()),
            (today, 35000, "CNY".into()),
        ]
    );
}

#[test]
fn fund_bulk_hit_without_history_writes_price_but_no_weekly_point() {
    let today_date = beijing_today();
    let yesterday = (today_date - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let today = today_date.format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-fund-0", "100000", "权威名称-100000");
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-fund-0",
            price_cents: 30000,
            currency_code: "CNY",
            priced_at: &yesterday,
            nav_date: Some(&yesterday),
            source: Some(EASTMONEY_PRICE_SOURCE),
        },
    )
    .unwrap();

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        empty_nav(per_fund_nav_calls.clone()),
        counting_name(Arc::new(AtomicUsize::new(0))),
        Box::new(no_confirm),
        bulk_surfaces(
            batch_stub(
                FundNameDictionary::from([("100000".to_string(), "权威名称-100000".to_string())]),
                FundNavTable::from([(
                    "100000".to_string(),
                    BulkNavPoint {
                        date: today.clone(),
                        nav: 3.5,
                    },
                )]),
            ),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    );

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut WriteWitness::default(),
    ))
    .unwrap();

    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        0,
        "批量面命中：零逐只请求"
    );
    assert_eq!(result.written, 1);
    assert_eq!(
        fund_price_of(&conn, "inst-fund-0"),
        Some((35000, Some(today.clone())))
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund-0"),
        vec![],
        "无历史序列不落采样点：单点会冒充历史完整、永久破坏首刷判据",
    );
}

fn mark_constant(conn: &Connection, instrument_id: &str) {
    conn.execute(
        "UPDATE instruments SET constant_unit_price = 10000 WHERE id = ?1",
        params![instrument_id],
    )
    .unwrap();
}

fn constant_unit_price_of(conn: &Connection, instrument_id: &str) -> Option<i64> {
    conn.query_row(
        "SELECT constant_unit_price FROM instruments WHERE id = ?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn constant_price_fund_gets_no_requests_and_is_excluded_from_denominator_and_gaps() {
    let today = beijing_today();
    let yesterday = (today - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-const", "000198", "权威名称-000198");
    mark_constant(&conn, "inst-const");
    seed_fund_history(&conn, "inst-const", &yesterday);
    let version_before: i64 = conn
        .query_row(
            "SELECT version FROM market_prices WHERE instrument_id = 'inst-const'",
            [],
            |r| r.get(0),
        )
        .unwrap();

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let per_fund_name_calls = Arc::new(AtomicUsize::new(0));
    let bulk_calls = Arc::new(AtomicUsize::new(0));
    let mut witness = WriteWitness::default();
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        counting_nav(
            per_fund_nav_calls.clone(),
            today.format("%Y-%m-%d").to_string(),
            1.0,
        ),
        counting_name(per_fund_name_calls.clone()),
        Box::new(no_confirm),
        bulk_surfaces(
            counting_batch_stub(
                bulk_calls.clone(),
                FundNameDictionary::new(),
                FundNavTable::new(),
            ),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    );

    let log = Mutex::new(Vec::new());
    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut progress_recorder(&log),
        &mut witness,
    ))
    .unwrap();

    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        0,
        "删除逐只豁免 → 恒定标的重新发起逐只请求，本断言变红"
    );
    assert_eq!(
        per_fund_name_calls.load(Ordering::SeqCst),
        0,
        "名称随行刷新挂在基金分区循环里：恒定标的不触发逐只名称请求"
    );
    assert_eq!(
        bulk_calls.load(Ordering::SeqCst),
        0,
        "净值分区为空：批量面零请求（无标的可刷，不白撞数据源）"
    );
    assert!(log.lock().unwrap().is_empty(), "分母为 0：不发任何进度事件");
    assert_eq!(
        result.bulk_gaps, 0,
        "删除缺口排除 → 恒定标的被计为批量面缺口，本断言变红"
    );
    assert!(!result.bulk_degraded, "零请求不是降级");
    assert_eq!(result.synced, 0);
    assert_eq!(
        result.skipped, 1,
        "恒定标的计入跳过统计（同步不为它取价，与无通道行同桶）"
    );
    assert_eq!(result.written, 0, "恒定标的不落任何价格行");
    assert!(!witness.any_written(), "零写入不置脏不发价格失效信号");
    assert_eq!(constant_unit_price_of(&conn, "inst-const"), Some(10_000));
    assert_eq!(
        fund_price_of(&conn, "inst-const"),
        Some((30_000, Some(yesterday))),
        "现价缓存保留建档一条、不再随同步更新"
    );
    let version_after: i64 = conn
        .query_row(
            "SELECT version FROM market_prices WHERE instrument_id = 'inst-const'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(version_after, version_before, "现价缓存行零触碰");
    assert_eq!(
        price_history_rows(&conn, "inst-const").len(),
        1,
        "不再为恒定标的落周采样点（存量行不删也不新增）"
    );
}

#[test]
fn money_fund_signal_marks_instrument_and_lands_nothing() {
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-money", "000198", "余额宝");

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let calls_outer = per_fund_nav_calls.clone();
    let date = today.clone();
    let nav: FetchNavHistory = Box::new(move |_: &str| {
        let date = date.clone();
        let calls = calls_outer.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(nav_points(&[(date.as_str(), 0.2229)]))
        })
    });
    let confirm_calls = Arc::new(AtomicUsize::new(0));
    let confirm_calls_clone = confirm_calls.clone();
    let confirm: FetchMoneyFundForm = Box::new(move |_: &str| {
        confirm_calls_clone.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    });
    let mut witness = WriteWitness::default();
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        nav,
        counting_name(Arc::new(AtomicUsize::new(0))),
        confirm,
        bulk_surfaces(
            batch_stub(
                FundNameDictionary::from([("000198".to_string(), "余额宝".to_string())]),
                FundNavTable::new(),
            ),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    );

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut witness,
    ))
    .unwrap();

    assert_eq!(
        constant_unit_price_of(&conn, "inst-money"),
        Some(10_000),
        "官方披露自报形态确认即打标（单向）"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-money"),
        Some((10_000, None)),
        "建档常量价 1.0000 落现价缓存、净值日期为空（水位语义退出）"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-money"),
        vec![],
        "确认即收尾：平坦周点不再生长"
    );
    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        0,
        "确认即收尾：零逐只净值请求（万份收益取值位永不落库）"
    );
    assert_eq!(confirm_calls.load(Ordering::SeqCst), 1, "一次确认请求");
    assert_eq!(
        result.bulk_gaps, 0,
        "货基被批量面收录（有名称）、只是不给价格点：不是缺口"
    );
    assert_eq!(result.synced, 1);
    assert_eq!(result.written, 1, "建档常量价是实际价格写入");
    assert!(witness.any_written(), "常量价首落按价格写入广播");
}

#[test]
fn money_fund_absent_signal_lands_nav_without_marking() {
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-fund", "110022", "消费行业");

    let nav_date = today.clone();
    let nav: FetchNavHistory = Box::new(move |_: &str| {
        let date = nav_date.clone();
        Box::pin(async move { Ok(nav_points(&[(date.as_str(), 2.811)])) })
    });
    let confirm_calls = Arc::new(AtomicUsize::new(0));
    let confirm_calls_clone = confirm_calls.clone();
    let confirm: FetchMoneyFundForm = Box::new(move |_: &str| {
        confirm_calls_clone.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(false) })
    });
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        nav,
        counting_name(Arc::new(AtomicUsize::new(0))),
        confirm,
        batch_surfaces(FundNameDictionary::new(), FundNavTable::new()),
    );
    let mut witness = WriteWitness::default();
    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut witness,
    ))
    .unwrap();

    assert_eq!(
        constant_unit_price_of(&conn, "inst-fund"),
        None,
        "缺信号不打标（更不清空既有标记）"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((28_110, Some(today))),
        "普通净值照常落现价（净值日期兼任水位）"
    );
    assert_eq!(result.synced, 1);
    assert_eq!(result.written, 1);
}

#[test]
fn money_fund_disclosure_unavailable_lands_nothing() {
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-money", "000198", "余额宝");

    let nav_calls = Arc::new(AtomicUsize::new(0));
    let nav_calls_clone = nav_calls.clone();
    let nav: FetchNavHistory = Box::new(move |_: &str| {
        let calls = nav_calls_clone.clone();
        let date = today.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(nav_points(&[(date.as_str(), 0.2229)]))
        })
    });
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        nav,
        Box::new(|_| Box::pin(async { Ok("余额宝".to_string()) })),
        Box::new(|_| {
            Box::pin(async {
                Err(AppError::coded(
                    "sync.disclosure-source-malformed",
                    "基金官方披露数据源返回了无法解析的内容，请稍后重试同步",
                ))
            })
        }),
        bulk_surfaces(
            batch_stub(
                FundNameDictionary::from([("000198".to_string(), "余额宝".to_string())]),
                FundNavTable::new(),
            ),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    );
    let mut witness = WriteWitness::default();
    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut witness,
    ))
    .unwrap();

    assert_eq!(
        constant_unit_price_of(&conn, "inst-money"),
        None,
        "披露源不可信不打标"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-money"),
        None,
        "本轮整只不落：万份收益不得经取数面冒充单位净值"
    );
    assert_eq!(nav_calls.load(Ordering::SeqCst), 0, "判定门在抓取前收尾");
    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 1, "与被拦截同桶计入跳过");
    assert_eq!(result.written, 0);
    assert_eq!(result.renamed, 0);
    assert!(!witness.any_written(), "零价格零名称写入：见证器不标记");
}

#[test]
fn confirmed_money_fund_generates_no_per_round_requests() {
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-money", "000198", "余额宝");

    let confirm_calls = Arc::new(AtomicUsize::new(0));
    let confirm_calls_clone = confirm_calls.clone();
    let confirm: FetchMoneyFundForm = Box::new(move |_: &str| {
        confirm_calls_clone.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    });
    let nav_calls = Arc::new(AtomicUsize::new(0));
    let nav_calls_clone = nav_calls.clone();
    let nav: FetchNavHistory = Box::new(move |_: &str| {
        let calls = nav_calls_clone.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        })
    });
    let mut channels = fund_channels(
        QuoteChannelCalls::default(),
        nav,
        counting_name(Arc::new(AtomicUsize::new(0))),
        confirm,
        bulk_surfaces(
            batch_stub(
                FundNameDictionary::from([("000198".to_string(), "余额宝".to_string())]),
                FundNavTable::new(),
            ),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    );
    let mut witness = WriteWitness::default();
    tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut witness,
    ))
    .unwrap();
    assert_eq!(
        constant_unit_price_of(&conn, "inst-money"),
        Some(10_000),
        "首轮确认打标"
    );

    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut |_| {},
        &mut witness,
    ))
    .unwrap();
    assert_eq!(
        confirm_calls.load(Ordering::SeqCst),
        1,
        "确认后零逐轮确认请求（判定不进日常热路径）"
    );
    assert_eq!(nav_calls.load(Ordering::SeqCst), 0, "确认后零逐只净值请求");
    assert_eq!(result.synced, 0, "恒定标的计入跳过桶（不为常量发请求）");
}

#[test]
fn mixed_ledger_keeps_constant_fund_out_of_requests_denominator_and_gaps() {
    let today = beijing_today();
    let today_s = today.format("%Y-%m-%d").to_string();
    let last_week = (today - chrono::Duration::days(7))
        .format("%Y-%m-%d")
        .to_string();
    let conn = tauri_app_lib::test_support::open();
    insert_holding(
        &conn,
        "acc-stock",
        "inst-stock",
        "600519",
        "stock",
        "CNY",
        "sh",
    );
    seed_fund(&conn, "inst-fund", "100001", "陈旧名称-基金");
    seed_fund_history(&conn, "inst-fund", &last_week);
    seed_fund(&conn, "inst-const", "000198", "余额宝");
    mark_constant(&conn, "inst-const");

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let per_fund_name_calls = Arc::new(AtomicUsize::new(0));
    let bulk_calls = Arc::new(AtomicUsize::new(0));
    let bulletin_names: FundNameDictionary =
        [("100001".to_string(), "权威名称-100001".to_string())]
            .into_iter()
            .collect();
    let bulletin_nav: FundNavTable = [(
        "100001".to_string(),
        BulkNavPoint {
            date: today_s.clone(),
            nav: 3.0,
        },
    )]
    .into_iter()
    .collect();
    let mut channels = SyncFetchChannels {
        fetch_quotes: Box::new(mock_fetch(&[("600519", Some(100_000))])),
        fetch_kline: Box::new(|_| {
            Box::pin(async { unreachable!("历史日 K 已移出现价刷新编排") })
        }),
        fetch_nav_history: counting_nav(per_fund_nav_calls.clone(), today_s.clone(), 3.0),
        fetch_fund_name: counting_name(per_fund_name_calls.clone()),
        confirm_money_fund_form: Box::new(|_| Box::pin(async { Ok(false) })),
        bulk: bulk_surfaces(
            counting_batch_stub(bulk_calls.clone(), bulletin_names, bulletin_nav),
            Arc::new(Mutex::new(BulkFetchCircuit::new())),
        ),
    };

    let log = Mutex::new(Vec::new());
    let mut witness = WriteWitness::default();
    let result = tauri::async_runtime::block_on(do_incremental_sync_channels(
        &conn,
        &mut channels,
        &mut progress_recorder(&log),
        &mut witness,
    ))
    .unwrap();

    assert_eq!(
        *log.lock().unwrap(),
        vec![(0, 2), (1, 2), (2, 2)],
        "删除分母排除 → 恒定标的进分母（3 格），本断言变红；行情 + 基金照常推进"
    );
    assert_eq!(
        per_fund_nav_calls.load(Ordering::SeqCst),
        0,
        "删除逐只豁免 → 恒定标的（批量面未覆盖）退逐只请求，本断言变红；普通基金由批量面直落零逐只请求"
    );
    assert_eq!(
        per_fund_name_calls.load(Ordering::SeqCst),
        0,
        "名称字典覆盖普通基金、恒定标的不进基金循环：零逐只名称请求"
    );
    assert_eq!(bulk_calls.load(Ordering::SeqCst), 1, "批量面照试一次");
    assert_eq!(
        result.bulk_gaps, 0,
        "删除缺口排除 → 批量面未覆盖的恒定标的被计为缺口，本断言变红"
    );
    assert!(!result.bulk_degraded);
    assert_eq!(result.synced, 2, "行情 + 普通基金照常计入处理成功");
    assert_eq!(result.written, 2, "行情报价 + 基金批量面净值照常落库");
    assert_eq!(
        result.skipped, 1,
        "恒定标的计入跳过统计（同步不为它取价，与无通道行同桶）"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((30_000, Some(today_s.clone()))),
        "普通基金的批量面直落照旧"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![
            (last_week.clone(), 30_000, "CNY".into()),
            (today_s.clone(), 30_000, "CNY".into()),
        ],
        "普通基金的当周采样点照旧：上周采样点保留、本周点由批量面直落"
    );
    assert_eq!(
        instrument_name(&conn, "inst-fund"),
        "权威名称-100001",
        "普通基金的名称随行刷新照旧"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-const"),
        None,
        "恒定标的零触碰（建档前无现价行，也不被兜底）"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-const"),
        vec![],
        "恒定标的不落周采样点"
    );
    assert_eq!(
        instrument_name(&conn, "inst-const"),
        "余额宝",
        "恒定标的不进名称随行刷新"
    );
    assert_eq!(
        constant_unit_price_of(&conn, "inst-const"),
        Some(10_000),
        "恒定标记原样（单向，不因批量面未覆盖而改写）"
    );
}
