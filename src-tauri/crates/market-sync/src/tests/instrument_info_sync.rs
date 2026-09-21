//! 标的信息同步（InstrumentInfoSync，issue #103 / #137 / ADR-0019；覆盖面放开
//! 至库内全部标的 + 名称随行刷新 issue #827；确定进度序列 issue #897 / ADR-0095）：
//! 查询单元路由（市场 + 代码）、日 K / 汇率 K 报文解析、现价 upsert、K 线周采样回填、名称
//! 刷新与幂等语义。
//! 编排经注入 mock 查询 / kline / fx / 净值 / 基金名称闭包与进度回调驱动，不依赖
//! 真实网络。

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
use crate::http::{KlineBar, KlineResponse, fx_secid_candidates, parse_klines};
use crate::incremental::{beijing_date, beijing_today, do_incremental_sync_with};
use crate::model::WriteWitness;
use crate::session::ScopedSession;
use crate::{FetchFundName, FetchNavHistory};
use ledger_infra::error::{AppError, Result};
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, SINA_PRICE_SOURCE, TENCENT_PRICE_SOURCE,
    upsert_market_price, upsert_price_history,
};

use super::{insert_holding, insert_lot};
use tauri_app_lib::test_support::{seed_account, seed_instrument};

// ---------------------------------------------------------------------------
// 持仓价格增量同步（issue #103）：查询单元路由（市场 + 代码）、批量报价条目消费、编排、跳过规则、
// 结果统计与幂等。编排经注入 mock 查询函数驱动，不依赖真实网络。
// ---------------------------------------------------------------------------

fn market_price_of(conn: &Connection, instrument_id: &str) -> Option<i64> {
    conn.query_row(
        "SELECT price_cents FROM market_prices WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .ok()
}

/// 测试态作用域会话（issue #1275）：把测试手里的连接直通给编排——与生产
/// 「写入口已持连接的直通会话」（壳层 `commands::sync`）同语义，锁行为不在
/// 本层测试面（会话自身的结构钉见下方 `orchestration_takes_connection_only_` 用例）。
/// 有了它，既有用例的调用点零改动。
impl ScopedSession for Connection {
    fn with_connection<R, F>(
        &self,
        use_connection: F,
    ) -> impl std::future::Future<Output = Result<R>> + Send
    where
        F: FnOnce(&Connection) -> Result<R> + Send + 'static,
        R: Send + 'static,
    {
        // 作业内联完成（测试态直通）：连接引用不进 future 状态。
        std::future::ready(use_connection(self))
    }
}

/// 新接缝自身的接口测试（issue #1275 结构钉）：记录型会话 + 记录型抓取闭包
/// 驱动一次完整同步，断言「抓取闭包执行期间没有任何会话被取出」——网络
/// 期间不持连接；事件全序同时钉住「读写只在会话内、抓取只在会话外」的
/// 交织形状。这是作用域会话接缝的结构性质，不重复覆盖编排行为。
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
            // 作业内联完成（测试态直通）：take/release 与连接读写在同步段完成，
            // 连接引用不进 future 状态（Send 由就绪 future 的载荷保证）。
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
        &mut no_fx,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    // 编排真实跑完（行为侧锚：与既有用例同口径）。
    assert_eq!(result.synced, 1);
    assert_eq!(result.skipped, 0);

    // 结构钉：全序 = 逐段「取连接→还」与「会话外抓取」的交替；抓取起止之间
    // 不出现任何 take/release（网络期间不持连接）。一次行情标的事件顺序：
    // 收集 → 批量报价（外） → 名称+现价落库（含当周采样点判定）。
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
            "release", // 收集库内标的
            "fetch:start",
            "fetch:end", // 批量报价（会话外）
            "take",
            "release", // 名称随行刷新 + 现价 upsert + 当周采样点判定
            "take",
            "release", // 本位币读取（汇率回填；无非本位币币种对则零抓取）
        ],
        "读写只在会话内、抓取只在会话外的交织形状（issue #1275）"
    );
}

/// 查询单元 → 断言用字符串（`市场:代码` 逗号串，issue #1555）：测试断言不依赖
/// 数据源查询键（secid）形态。
fn query_log_line(queries: &[QuoteQuery]) -> String {
    queries
        .iter()
        .map(|q| format!("{}:{}", q.market, q.code))
        .collect::<Vec<_>>()
        .join(",")
}

/// 模拟批量报价：对每个查询单元生成条目。`prices` 为 code → 现价
///（万分之一元刻度；None 表示停牌/无效价），不在映射中的代码不返回
///（模拟查询无果）。行情日期不携带（None）——落库走北京日内兜底，与既有断言语义一致。
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

/// 测试用场内报价条目（万分之一元已换算、行情日期可空）。
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

/// 每查询单元一条默认报价（名称随代码、价 10.00 元）：拆批与
/// 进度用例的应答形状，避免各处重抄同一 `QuoteItem` 映射体。
fn quote_items_for(queries: &[QuoteQuery]) -> Vec<QuoteItem> {
    queries
        .iter()
        .map(|query| quote_item(query, &format!("名称-{}", query.code), Some(100_000), None))
        .collect()
}

#[test]
fn quote_channel_derivation_matches_quote_key_construction() {
    // 价格通道收口（issue #1060）：行情通道派生（投资域单点 `derive_price_channel`）
    // 与行情查询键构造能力（同步域 `tencent_query_key`，issue #1560 接线后）恒等
    // ——判「可行情」的市场必须恰是可构造查询键的市场，否则 Quote 行进不了查询
    //（静默跳过）或无通道行混进行情分区。
    use ledger_investment::{InstrumentType, PriceChannel, derive_price_channel};
    for market in ["sh", "sz", "hk", "nasdaq", "nyse", "amex", "unknown"] {
        assert_eq!(
            derive_price_channel(InstrumentType::Stock, market, "600000", None)
                == PriceChannel::Quote,
            crate::tencent::tencent_query_key(market, "600000").is_some(),
            "行情通道判定与行情查询键构造能力漂移：{market}"
        );
    }
}

#[test]
fn incremental_sync_normalizes_symbol_suffix() {
    let conn = tauri_app_lib::test_support::open();
    // schema 注释示例格式：symbol 带市场后缀（"600519.SH"），查询单元取裸代码
    // "600519"（市场前缀由通道内部与代码组合，issue #1555）。
    insert_holding(&conn, "acc-1", "inst-sh", "600519.SH", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-hk", "00700.HK", "stock", "HKD", "hk");

    // mock 按响应侧裸代码（f12）返回：归一化后应能匹配并写入价格。
    let prices = [("600519", Some(13_028_000)), ("00700", Some(4_454_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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

    // 查询全部无果（如整批代码无效、响应 data:null）：不报错、全部计入跳过。
    let mut fetch = mock_fetch(&[]);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
        &mut no_fx,
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
    // 预先存在的旧价（应被覆盖更新，不产生新行）
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-sh",
            price_cents: 999,
            currency_code: "CNY",
            // 预置旧价的时间点为夹具簿记，引用工厂固定时刻常量（ADR-0084 决策 5）。
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
        &mut no_fx,
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

    // 价格覆盖更新：A 股直接得分、港股 ÷10
    assert_eq!(market_price_of(&conn, "inst-sh"), Some(13028000));
    assert_eq!(market_price_of(&conn, "inst-sz"), Some(117300));
    assert_eq!(market_price_of(&conn, "inst-hk"), Some(4454000));
    // 每标的一条价格（无重复）
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM market_prices", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 3);

    // 标的字典（名称/市场）不变
    let (name, market): (Option<String>, String) = conn
        .query_row(
            "SELECT name, market FROM instruments WHERE id='inst-sh'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(name.as_deref(), Some("名称-600519"));
    assert_eq!(market, "sh");
    // 未新增标的
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
        &mut no_fx,
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
    // 停牌股已有旧价
    upsert_market_price(
        &conn,
        &MarketPriceWrite {
            instrument_id: "inst-sz",
            price_cents: 888,
            currency_code: "CNY",
            // 预置旧价的时间点为夹具簿记，引用工厂固定时刻常量（ADR-0084 决策 5）。
            priced_at: tauri_app_lib::test_support::FIXED_NOW,
            nav_date: None,
            source: Some("eastmoney"),
        },
    )
    .unwrap();

    // 600519 正常价；000001 停牌（f2 无效 → None）
    let prices = [("600519", Some(13_028_000)), ("000001", None)];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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

    // mock 只返回 600001：600002 查询无果（响应缺失）→ 计入跳过
    let prices = [("600001", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
    // 市场未知的持仓股票（如手动创建未设市场）：无行情通道，计入跳过
    insert_holding(
        &conn, "acc-2", "inst-unk", "NVDA", "stock", "USD", "unknown",
    );

    let prices = [("600519", Some(13_028_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
        &mut no_fx,
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
        &mut no_fx,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    assert_eq!(second.synced, 1);

    // 重复调用不产生重复价格行（每标的一条，覆盖更新）
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
    // 同一标的在另一账户也有持仓：应去重为一只、只查一次
    seed_account(&conn, "acc-2", "账户-acc-2", "investment", "CNY", 0);
    insert_lot(&conn, "acc-2", "inst-sh", "CNY");

    let prices = [("600519", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
    // ADR-0121 验收 8（可观察面是请求数，不是常量本身）：233 只行情标的的账本，
    // 批量报价的请求数是 1 次——小于旧的批大小 50 会把同一账本拆成 5 次请求。
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
        &mut no_fx,
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
    // 编排不再按数据源批大小切割查询（issue #1560）：全部行情标的作为一次
    // 通道调用递交，单次请求的批量承载量由取数层自行分批（见腾讯取数层用例）。
    // 标数取 **旧东财报批大小 300 再加 5**：跨越那个边界后仍是一次调用，
    // 证明编排层不再保留任何源相关批大小。
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
        &mut no_fx,
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

/// 「替换通道实现即换源」负向接线证明（issue #1555）：现价刷新编排只递
/// 「市场 + 代码」查询单元，查询键由批量报价通道在内部构造——本用例注入一个
/// 按自己形态（模拟换源）构造查询键的通道实现，价格照常落库。
///
/// 把查询键构造挪回编排（编排先拼出东财 secid `1.600519` 再交给通道）后，
/// 本桩拿到的 `code` 是 secid 而非裸代码 `600519`，构造不出任何匹配的查询键
/// → 无报价条目 → 价格未落库，本用例变红。
#[test]
fn swapping_quote_channel_keeps_prices_landing_without_source_key_in_orchestration() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");

    // 换源形态的通道桩：自己把（市场，代码）编成自己的查询键，只收自己的形态。
    let mut fetch = |queries: &[QuoteQuery]| {
        let items = queries
            .iter()
            .filter(|q| q.market == "sh" && q.code == "600519")
            .map(|q| quote_item(q, "贵州茅台", Some(130_280), None))
            .collect();
        super::ready(Ok(items))
    };
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
        &mut no_fx,
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

// ---------------------------------------------------------------------------
// 写入见证（issue #1277）：跨分段的「是否实际写过」累积器。分段形态逐段
// autocommit，中途失败的运行结果统计随错误丢失，见证器由调用方持有存活——
// 壳层收尾裁决「实际写过即置脏即发信号」（成败同判）的证据源，本层钉住
// 标记时机与口径。
// ---------------------------------------------------------------------------

/// 实际写过之后中途失败：见证器存活（失败 ≠ 未写过）。前面分段已落库的写入
/// 是既成事实，失败收尾的置脏与信号判定据此归一为「实际写过」（#1277 AC）。
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

    // 批量报价成功（股票落现价），随后基金净值抓取失败——报价写入已 autocommit、
    // 失败回不去；见证器必须仍报告「写过」。现价与历史解耦后（issue #1377），
    // 同步中途的网络失败来自基金净值通道（日 K 已归后台补全）。
    let prices = [("600001", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let mut nav = |_: &str| super::ready(Err(AppError::Io("模拟净值网络失败".into())));
    let mut witness = WriteWitness::default();
    let err = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
    // 行为侧锚：价格确实已落库（分段 autocommit，失败也回不去）。
    assert_eq!(market_price_of(&conn, "inst-a"), Some(100000));
}

/// 成功路径镜像钉：见证器与结果统计同口径——`witness.any_written()` ==
/// `result.any_written()`（有写入 / 零写入两侧），壳层用哪一份判定都不漂移。
#[test]
fn witness_mirrors_result_any_written_on_success() {
    // 侧一：股票有效价 → 有写入，见证器与结果统计同为真。
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    let prices = [("600001", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let mut witness = WriteWitness::default();
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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

    // 侧二：基金「已是最新」（增量窗口无新净值）且名称未取到 → 零写入，
    // 见证器与结果统计同为清白。
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
        &mut no_fx,
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

// ---------------------------------------------------------------------------
// K 线回填（issue #137 / ADR-0019）：现价 upsert 之外，近两年日 K 回填降采样落
// PriceHistory / FxRateHistory。编排经注入 mock kline / mock fx 闭包驱动
// （与批量报价同一接缝），不依赖真实网络。
// ---------------------------------------------------------------------------

/// 构造一根日 K 样本（日期为 ISO 日期，收盘为真实价格值，如 10.40 元）。
fn bar(date: &str, close: f64) -> KlineBar {
    KlineBar {
        date: date.to_string(),
        close,
    }
}

/// 空实现：同 [`no_kline`]，用于汇率回填。
fn no_fx(_: &str) -> FetchFuture<Vec<KlineBar>> {
    super::ready(Ok(vec![]))
}

/// 空实现：既有用例只关心股票/汇率行为时注入（净值通道最小桩，首刷查无净值
/// 形态——基金计入跳过）。
fn no_nav(_: &str) -> FetchFuture<Vec<NavPoint>> {
    super::ready(Ok(vec![]))
}

/// 空实现：既有用例不关心基金名称刷新时注入（返回空串 = 未取到名称，不落库）。
fn no_name(_: &str) -> FetchFuture<String> {
    super::ready(Ok(String::new()))
}

/// 空实现：既有用例不关心货基判定时注入（缺信号形态——不打标、照常取数落库）。
/// 货基判定用例注入自己的确认桩（确认 / 缺信号 / 源不可信三态）。
fn no_confirm(_: &str) -> FetchFuture<bool> {
    super::ready(Ok(false))
}

/// 空实现：既有用例不关心进度序列时注入（进度回调最小桩，issue #897）。
fn no_progress(_progress: SyncProgress) {}

/// 无批量取数面的注入桩（issue #1374）：两面恒定报「零覆盖」，所有标的按缺口走
/// 逐标的通道——既有用例断言的正是逐标的路径（降级兜底路径），行为与改造前逐字节
/// 一致。批量取数面自身的行为断言见 `tests/bulk_fetch.rs` 与本文件下方取数面用例。
fn no_bulk() -> BulkFetchSurfaces {
    BulkFetchSurfaces::absent()
}

/// 进度记录闭包：把**标的级一格的** (done, total) 推进序列攒进测试侧共享缓冲
///（与 [`mock_fx`] 的请求记录同纪律：缓冲由测试持有，断言时 borrow）。事件恒为
/// 标的级两字段（原页级明细随逐只通道换源退役，issue #1571）。
fn progress_recorder<'a>(log: &'a Mutex<Vec<(usize, usize)>>) -> impl FnMut(SyncProgress) + 'a {
    move |progress| {
        log.lock().unwrap().push((progress.done, progress.total));
    }
}

/// 模拟汇率 K 线抓取：按 base+quote 直连串（如 "HKDCNY"）返回汇率日线样本，
/// 并记录被请求的币种对（断言只对非本位币发起抓取）。
fn mock_fx<'a>(
    by_pair: &'a [(&'a str, Vec<KlineBar>)],
    requested: &'a Mutex<Vec<String>>,
) -> impl FnMut(&str) -> FetchFuture<Vec<KlineBar>> + Send + 'a {
    move |pair: &str| {
        requested.lock().unwrap().push(pair.to_string());
        super::ready(Ok(by_pair
            .iter()
            .find(|(p, _)| *p == pair)
            .map(|(_, bars)| bars.clone())
            .unwrap_or_default()))
    }
}

/// 查询某标的的周采样价格历史（trade_date, price_cents, currency_code），按日期升序。
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

/// 查询币种对的周采样汇率历史（trade_date, rate），按日期升序。
fn fx_rows(conn: &Connection, base: &str, quote: &str) -> Vec<(String, f64)> {
    let mut stmt = conn
        .prepare(
            "SELECT trade_date, rate FROM fx_rate_history \
             WHERE base_code=?1 AND quote_code=?2 ORDER BY trade_date",
        )
        .unwrap();
    let rows = stmt
        .query_map(params![base, quote], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
        })
        .unwrap();
    rows.map(|r| r.unwrap()).collect()
}

#[test]
fn sync_writes_no_price_history_quote_only() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600519", "stock", "CNY", "sh");
    let prices = [("600519", Some(100_000))];
    let fx_log = Mutex::new(Vec::new());
    let mut fetch = mock_fetch(&prices);
    let mut fx = mock_fx(&[], &fx_log);
    // 现价与历史解耦（ADR-0122 / issue #1377）：同步只刷现价——无历史序列的
    // 标的也不落任何采样点（单点会冒充历史完整、永久破坏后台补全的首刷判据），
    // 编排的参数表里已无日 K 通道（编译期不可表达）。
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut fx,
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
    // 当周采样点直落（ADR-0122 决策 2 / issue #1377）：已有历史序列的行情标的，
    // 现价刷新携带的当日有效报价即当周采样点——不另发逐只日 K 请求（编排的
    // 参数表已无日 K 通道，编译期不可表达），周采样语义不变（每周至多一条、
    // 同周整周覆盖幂等）；历史深采集仍归后台补全。
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
    let fx_log = Mutex::new(Vec::new());
    let mut fetch = mock_fetch(&prices);
    let mut fx = mock_fx(&[], &fx_log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut fx,
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
fn fx_secid_candidates_cover_onshore_and_reverse_fallback() {
    // HKD→CNY：东财无 119.HKDCNY，主候选为在岸人民币市场 120.HKDCNYC；反向兜底取倒数。
    assert_eq!(
        fx_secid_candidates("HKDCNY"),
        vec![
            ("120.HKDCNYC".to_string(), false),
            ("119.HKDCNY".to_string(), false),
            ("119.CNYHKD".to_string(), true),
        ]
    );
    // 纯全球外汇对：119 直连 + 反向兜底。
    assert_eq!(
        fx_secid_candidates("EURUSD"),
        vec![
            ("119.EURUSD".to_string(), false),
            ("119.USDEUR".to_string(), true),
        ]
    );
    // 美元→人民币（issue #696 美股持仓折算）：在岸中间价 120.USDCNYC 首选
    //（2026-01 实测命中；离岸 119.USDCNY 无直连数据），反向兇底取倒数。
    assert_eq!(
        fx_secid_candidates("USDCNY"),
        vec![
            ("120.USDCNYC".to_string(), false),
            ("119.USDCNY".to_string(), false),
            ("119.CNYUSD".to_string(), true),
        ]
    );
    // 本位币为 base 的反向对：119 直连 + 119 反向 + 120 反向（取倒数）。
    assert_eq!(
        fx_secid_candidates("CNYHKD"),
        vec![
            ("119.CNYHKD".to_string(), false),
            ("119.HKDCNY".to_string(), true),
            ("120.HKDCNYC".to_string(), true),
        ]
    );
}

#[test]
fn kline_response_deserializes_daily_bars_and_skips_invalid() {
    // 真实 push2his 日 K 响应形状（fields2=f51,f53 → 每行 "日期,收盘价"）。
    let json = r#"{"rc":0,"rt":17,"svr":1,"lt":1,"full":0,"data":{"code":"600519","market":1,"name":"贵州茅台","decimal":2,"dktotal":566,"preKPrice":1302.8,"klines":["2026-01-05,1302.80","2026-01-06,1310.00","2026-01-07,-"]}}"#;
    let resp: KlineResponse = serde_json::from_str(json).unwrap();
    let data = resp.data.unwrap();
    let klines = data.klines.unwrap();
    let bars = parse_klines(&klines);
    assert_eq!(bars.len(), 2, "无效收盘样本（'-'）应被过滤");
    assert_eq!(bars[0].date, "2026-01-05");
    assert_eq!(bars[0].close, 1302.80);
    assert_eq!(bars[1].close, 1310.00);

    // 无效代码 / 无数据：data 为 null → 空序列，不报错（优雅降级）。
    let empty: KlineResponse = serde_json::from_str(r#"{"rc":100,"data":null}"#).unwrap();
    assert!(empty.data.is_none());
}

#[test]
fn week_key_matches_sqlite_week_start_column() {
    // Rust 侧降采样周键（week_monday）与 V010 week_start 生成列恒等——这是
    // 「整周覆盖幂等」的隐式契约：周键一旦漂移，ON CONFLICT 落点即错、产生重复周行。
    // 扫描跨年/闰年边界三年，每天与 SQLite 生成表达式比对。
    use crate::incremental::week_monday;
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

// ---------------------------------------------------------------------------
// 基金分区：现价刷新的逐只回退（issue #303 / ADR-0038 决策 6 / issue #1571 换源）。
// 编排经注入 mock 全历史闭包驱动，不依赖真实网络；逐只回退与历史回填同用新浪
// 单只全历史通道（一次请求整只历史），窗口由编排本地裁剪——水位语义（首刷近两年
// / 增量从水位次日起）经「窗口外点不落库」钉住，每只至多一次请求。
// ---------------------------------------------------------------------------

/// 构造整只历史净值序列：points 为 (日期, 单位净值)。
fn nav_points(points: &[(&str, f64)]) -> Vec<NavPoint> {
    points
        .iter()
        .map(|(d, n)| NavPoint {
            date: d.to_string(),
            nav: *n,
        })
        .collect()
}

/// 模拟新浪单只全历史抓取：按代码返回整只历史净值序列（未收录代码回可信空），
/// 并记录全部请求（断言每只至多一次请求与「非可拉取行零请求」）。
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
    // 北京日历 = UTC 时刻 + 8h 后取日期部分：16:00 UTC 是北京午夜边界，
    // 前一瞬仍属同日，后一瞬进入次日（A 股/基金净值日历以北京时间为准）。
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

/// 基金现价缓存的 (price_cents, nav_date)（无行返回 None）。
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
        &mut no_fx,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    // 首刷查无净值（查无此码 / 新基金未公布首期）：计入跳过，不报错不落价。
    assert_eq!(result.synced, 0);
    assert_eq!(result.skipped, 1);
    assert_eq!(result.written, 0);
    assert_eq!(fund_price_of(&conn, "inst-fund"), None);
    assert_eq!(result.message, "已同步 0 只，跳过 1 只");
}

#[test]
fn fund_rows_without_real_code_skip_without_fetch() {
    let conn = tauri_app_lib::test_support::open();
    // 名称充代码的基金行（无真实代码，查不到净值）与债券：计入跳过、零请求。
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
        &mut no_fx,
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
        &mut no_fx,
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

// ---------------------------------------------------------------------------
// ETF 行情分区（issue #695 / spec #690 方案 6）：行情分区从仅 stock 扩为
// stock|etf——场内 ETF 持仓与股票同走批量报价/日 K 通道；fund 仍走净值通道；
// 债券等无行情来源标的仍计入跳过。离线注入桩钉住三分区行为；
// three_type_partitions_roll_up_into_one_result 收编既有 fund+stock 汇总用例。
// ---------------------------------------------------------------------------

#[test]
fn etf_holding_syncs_quote_only_without_history() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-etf", "510300", "etf", "CNY", "sh");

    // 批量报价：ETF 4.634 元 → 46340 万分之一元（取数层已换算）。
    let mut fetch = |_: &[QuoteQuery]| {
        super::ready(Ok(vec![QuoteItem {
            code: "510300".into(),
            name: "沪深300ETF华泰柏瑞".into(),
            price_cents: Some(46_340),
            price_date: None,
        }]))
    };

    // 现价与历史解耦（ADR-0122 / issue #1377）：ETF 同步只刷现价，不回填历史。
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
    // 市场未知的 ETF 持仓（如手动建档未设市场）：无行情通道，
    // 计入跳过且零请求（跳过统计与标的收集同源，不报错）。
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
        &mut no_fx,
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
    // 三分区一次钉住：行情分区 = stock + etf；净值通道 = fund；跳过 =
    // 名称充代码基金行 + bond + other。跳过统计与标的收集出自同一次收集（同源）。
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

    // 报价批次同时携带股票与 ETF（同一分区、同批路由「市场 + 代码」，收集按 symbol 升序）；
    // 价格已按取数层刻度换算为万分之一元：ETF 4.634 元、股票 1316.01 元。
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
        &mut no_fx,
        &mut nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();

    // synced = 行情分区 2（股票+ETF）+ 基金 1；skipped = 名称充代码基金 + 债券 + 其他；
    // written = 实际落价 3（基金短窗净值落现价计入，issue #1377）。
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

// ---------------------------------------------------------------------------
// 美股行情通道（issue #696 / ADR-0081 决策 2）：f2 刻度实测钉住、美股持仓
// 刷价 + 日 K 回填 + USDCNY 汇率同期采集的端到端离线注入。
// ---------------------------------------------------------------------------

#[test]
fn us_stock_holding_syncs_quote_kline_and_usdcny() {
    let conn = tauri_app_lib::test_support::open();
    // 美股持仓：纳斯达克标的、USD 币种（创建增强落库形态）。
    insert_holding(
        &conn,
        "acc-usd",
        "inst-aapl",
        "AAPL",
        "stock",
        "USD",
        "nasdaq",
    );

    // 记录批量报价的查询单元：编排只递「市场 + 代码」，按精确市场路由（nasdaq:AAPL，零候选开销）。
    let query_log = Mutex::new(Vec::new());
    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        // AAPL $319.97 → 3199700 万分之一元（取数层已换算）。
        super::ready(Ok(vec![QuoteItem {
            code: "AAPL".into(),
            name: "苹果".into(),
            price_cents: Some(3_199_700),
            price_date: None,
        }]))
    };

    // USDCNY 汇率同期采集（持仓币种 USD ≠ 本位币 CNY）：记录被请求的币种对。
    let fx_log = Mutex::new(Vec::new());
    let fx_pairs = [(
        "USDCNY",
        vec![bar("2026-01-02", 7.02), bar("2026-01-08", 7.01)],
    )];
    let mut fx = mock_fx(&fx_pairs, &fx_log);

    // 现价与历史解耦（issue #1377）：同步刷现价 + 汇率同期采集，不回填日 K。
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut fx,
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

    // 编排按精确市场路由查询单元：nasdaq:AAPL（#692 已扩 nasdaq/nyse/amex 市场）。
    assert_eq!(*query_log.lock().unwrap(), vec!["nasdaq:AAPL".to_string()]);

    // 现价：f2 319970 × 10 = 3199700 万分之一元（$319.97），币种 USD。
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

    // 同步不回填价格历史（归价格历史后台补全，issue #1377）。
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM price_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 0);

    // USDCNY 汇率同期落 FxRateHistory（候选序钉住见 fx_secid_candidates 测试）。
    assert_eq!(
        *fx_log.lock().unwrap(),
        vec!["USDCNY".to_string()],
        "只对非本位币币种对发起汇率抓取"
    );
    assert_eq!(
        fx_rows(&conn, "USD", "CNY"),
        vec![("2026-01-02".into(), 7.02), ("2026-01-08".into(), 7.01)],
    );

    // 重跑幂等：现价覆盖更新、汇率历史同周整周覆盖，零重复行。
    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        super::ready(Ok(vec![QuoteItem {
            code: "AAPL".into(),
            name: "苹果".into(),
            price_cents: Some(3_200_000),
            price_date: None,
        }]))
    };
    let mut fx = mock_fx(&fx_pairs, &fx_log);
    tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut fx,
        &mut no_nav,
        &mut no_name,
        &mut no_confirm,
        &mut no_bulk(),
        &mut no_progress,
        &mut WriteWitness::default(),
    ))
    .unwrap();
    let fx_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM fx_rate_history", [], |r| r.get(0))
        .unwrap();
    assert_eq!(fx_count, 2, "汇率历史零重复行");
    assert_eq!(
        market_price_of(&conn, "inst-aapl"),
        Some(3_200_000),
        "现价覆盖更新为最新价"
    );
}

#[test]
fn us_stock_holdings_route_exact_market_per_instrument() {
    // 三市场各一持仓：编排按精确市场路由查询单元（nasdaq/nyse/amex），互不串市场；
    // 市场 → 数据源查询键的映射归通道（`quote_query_key` 单测钉住）。
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-nq", "AAPL", "stock", "USD", "nasdaq");
    insert_holding(&conn, "acc-2", "inst-ny", "BABA", "stock", "USD", "nyse");
    insert_holding(&conn, "acc-3", "inst-am", "SPY", "stock", "USD", "amex");

    let query_log = Mutex::new(Vec::new());
    let mut fetch = |queries: &[QuoteQuery]| {
        query_log.lock().unwrap().push(query_log_line(queries));
        super::ready(Ok(vec![]))
    };
    let fx_log = Mutex::new(Vec::new());
    let usdcny = [("USDCNY", vec![bar("2026-01-05", 7.02)])];
    let mut fx = mock_fx(&usdcny, &fx_log);
    tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut fx,
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

// ---------------------------------------------------------------------------
// 场内现价刷新接线腾讯批量报价（issue #1560 / ADR-0130 决策 2/5/7）
// ---------------------------------------------------------------------------

/// 构造一条最小但合法的腾讯 A 股报价语句（88 字段）：只填解析消费的字段
///（名称 @1 / 代码 @2 / 价格 @3 / 行情日期 @30 / 类型码 @61 / 币种 @82），
/// 其余留空——用于驱动**生产通道束**的端到端接线钉。
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

/// 生产接线钉（issue #1560，删除接线即红）：生产通道束的批量报价闭包打到腾讯
/// 批量报价端点（路径段 `q=<市场前缀代码逗号串>`），价格与名称照常落库——
/// 场内现价刷新不再走东财 ulist：把换装改回东财实现，注入的主机就收不到请求，
/// 本用例的「一次腾讯请求」断言即红。
#[test]
fn production_quote_channel_requests_tencent_batch_endpoint() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-sh", "600000", "stock", "CNY", "sh");

    let body = tencent_a_share_line("600000", "浦发银行", "9.07", "20260918161458");
    let gbk = encoding_rs::GBK.encode(&body).0.into_owned();
    let (url, requests) = super::spawn_header_capture_server(gbk);
    let mut channels = SyncFetchChannels::production_lane(
        Lane::Foreground,
        SyncFetchHosts {
            quote: vec![url],
            kline: vec![],
            fund_batch: vec![],
            fund_history: vec![],
        },
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

// ---------------------------------------------------------------------------
// 场外基金现价与名称刷新接线新浪批量面（issue #1565 / ADR-0130 决策 2/6/7）
// ---------------------------------------------------------------------------

/// 生产接线钉（issue #1565，删除接线即红）：生产通道束的场外基金批量面闭包打到
/// 新浪 `f_` 批量面（路径段 `list=f_<代码逗号串>`，GBK、必须带 Referer），价格
/// 与名称照常落库，来源标记记新浪——现价与名称刷新不再走东财：把换装改回东财
/// 实现，注入的主机就收不到请求，本用例的「一次新浪请求」断言即红。
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

    // 新浪批量面真实报文形态（GBK）：普通行带名称与单位净值。
    let body =
        format!("var hq_str_f_000001=\"华夏成长混合A,1.333,3.906,1.298,{today_s},24.0598\";\n");
    let gbk = encoding_rs::GBK.encode(&body).0.into_owned();
    let (url, requests) = super::spawn_header_capture_server(gbk);
    let mut channels = SyncFetchChannels::production_lane(
        Lane::Foreground,
        SyncFetchHosts {
            quote: vec![],
            kline: vec![],
            fund_batch: vec![url],
            fund_history: vec![],
        },
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

/// 行情日期取交易所当地交易日（ADR-0130 决策 5 / issue #1560）：现价缓存
/// 当周采样日取交易所当地交易日（ADR-0130 决策 5 / issue #1560）：现价刷新直落
/// 的当周采样点用取数层给出的行情日期，不做北京时间换算；改回按北京时间
/// （`beijing_today`）本用例变红。现价缓存与价格历史的来源标记均为新来源值
///（ADR-0130 决策 7）。现价时点仍为写入时刻（ADR-0103 决策 4），行情日期由
/// 采样日承载。
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
    // 已有历史序列是当周采样点直落的前提；旧点取上月，与本轮不同周。
    let old = (beijing_today() - chrono::Duration::days(30))
        .format("%Y-%m-%d")
        .to_string();
    upsert_price_history(&conn, "inst-aapl", &old, 100, "USD", EASTMONEY_PRICE_SOURCE).unwrap();

    // 交易所当地交易日（美股收盘日）与北京日历日相差一天——按北京时间换算会记错。
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
        &mut no_fx,
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

// ---------------------------------------------------------------------------
// 覆盖面放开至库内全部标的 + 名称随行刷新（issue #827）：收集不再以「当前有
// 持仓」为界——清仓标的恢复同步、纯建档未交易标的首次同步补历史；有通道的行
// 以数据源权威名称随行刷新（行情通道零额外请求、基金通道逐只详情查询），
// 零变化不虚增 version；无通道行不查名称。
// ---------------------------------------------------------------------------

/// 清仓 helper：把标的全部批次剩余数量清零（模拟卖出匹配后的库内形态）。
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
        &mut no_fx,
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
    // 纯建档未交易标的：只有 instruments 行，无任何交易/批次。
    seed_instrument(&conn, "inst-archived", "600000", "浦发银行", "CNY", "sh");

    let prices = [("600000", Some(100_000))];
    let mut fetch = mock_fetch(&prices);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
    // 既有行名称过时（如手工建错/旧数据源命名）：批量报价随行带回权威名称覆盖。
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
        &mut no_fx,
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
    // 名称与权威名称一致：零变化零写入，不虚增 version。
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
        &mut no_fx,
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
    // 有真实代码的基金行（首刷查无净值的形态）：名称通道照常可达。
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
        &mut no_fx,
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
    // 基金在建成标的后终止：搜索索引与档案通道都不再可达（ADR-0039 修订，issue
    // #1212）——名称刷新降级为保留原名，不得中断整次同步。
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
        &mut no_fx,
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
fn fund_name_refresh_still_propagates_network_failure() {
    // 边界：只有确定性查无降级；网络类失败仍按既有契约上抛中断（ADR-0039 修订）。
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
        &mut no_fx,
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
    // 名称充代码的基金行（非 6 位）：无通道，不发起名称查询；6 位行返回空名称也不写。
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
        &mut no_fx,
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
    // 纯结果类型的证据判定语义：价格与名称任一写入即为真。
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

// ---------------------------------------------------------------------------
// 同步确定进度序列（issue #897 / ADR-0095）：mock 进度回调钉住外部可见的推进
// 行为——收集完成立即发 total、逐有通道标的推进一格、跳过行不进分母、基金
// 「已是最新」照常推进、空库/全跳过不发 total。只测可见序列，不测编排内部。
// ---------------------------------------------------------------------------

#[test]
fn progress_sequence_total_first_then_per_instrument_advance() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-a", "600001", "stock", "CNY", "sh");
    insert_holding(&conn, "acc-2", "inst-b", "600002", "stock", "CNY", "sz");

    // 统一事件日志：total 必须先于任何抓取请求（收集与分区完成立即发）；
    // 每只标的在报价落库后推进一格（现价 + 名称合并计格；历史日 K 已移出同步，
    // issue #1377）。
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
        &mut no_fx,
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
    // 库内五行：沪股 1（行情通道）+ 市场未知股票 1（跳过）+ 债券 1（跳过）+
    // 名称充代码基金 1（跳过）+ 6 位代码基金 1（净值通道）→ 分母 = 2。
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
        &mut no_fx,
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
    // 停牌（f2 无效）与查询无果（响应缺失）的行情标的：有通道即计格，
    // 不以成败计——推进速度对齐真实处理量，而非成功量。
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

    // 600002 停牌（None）、600003 不在响应中（查询无果）。
    let prices = [("600001", Some(100_000)), ("600002", None)];
    let mut fetch = mock_fetch(&prices);
    let log = Mutex::new(Vec::new());
    let mut progress = progress_recorder(&log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
    // 基金「已是最新」（增量窗口内无新净值）：处理成功计入 synced、零写入，
    // 但进度照常推进一格——百分比不卡住（user story 7）。
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

    // 已有历史序列：水位只在「已回填过」的基金上作增量起点（issue #1059）。
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
        &mut no_fx,
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
    // 基金「净值 + 名称」合并为一步：两件事都完成才推进一格。
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
        &mut no_fx,
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
    // 空库：不发任何进度事件，返回既有「暂无标的可同步」提示（user story 15）。
    let conn = tauri_app_lib::test_support::open();
    let mut fetch = mock_fetch(&[]);
    let log = Mutex::new(Vec::new());
    let mut progress = progress_recorder(&log);
    let result = tauri::async_runtime::block_on(do_incremental_sync_with(
        &conn,
        &mut fetch,
        &mut no_fx,
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
    // 全部无通道（债券 + 名称充代码基金）：分母为 0，不发任何进度事件——
    // 空转不伪装成推进（user story 15），既有跳过统计提示照旧。
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
        &mut no_fx,
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

// ---------------------------------------------------------------------------
// 行情批量取数面（ADR-0121 / issue #1374）：日常现价刷新改走批量取数面。
//
// 本组用例经**生产同款入口** `do_incremental_sync_channels`（通道束拆交）驱动：
// 两个批量面就在束里，删掉通道束的取数面接线即断，下面「请求数收敛」「失败逐只
// 兜底」「缺口不熔断」的断言全部变红（负向判据）。
// ---------------------------------------------------------------------------

/// 基金标的的直插种子：账户 + 标的（fund / 6 位代码）+ 一笔买入，再覆写名称
/// （名称随行刷新的输入面：与数据源权威名称不同才应落库）。
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

/// 直插该基金的既有历史序列与现价水位（水位 = 现价缓存的净值日期，兼任历史
/// 增量水位）：已有历史序列即非首刷，逐只通道走的正是水位增量路径。
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

/// 标的行当前名称。
fn instrument_name(conn: &Connection, instrument_id: &str) -> String {
    conn.query_row(
        "SELECT name FROM instruments WHERE id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 标的行当前版本（「零变化不写库不虚增版本」的可观察面）。
fn instrument_version(conn: &Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT version FROM instruments WHERE id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 行情三通道（批量报价 / 日 K / 汇率）的调用计数：用例现场只有场外基金标的，
/// 这三条通道恒不应被触达——被触达即计数 + 断言面（同时硬失败，避免静默）。
#[derive(Clone, Default)]
struct QuoteChannelCalls {
    quote: Arc<AtomicUsize>,
    kline: Arc<AtomicUsize>,
    fx: Arc<AtomicUsize>,
}

impl QuoteChannelCalls {
    fn total(&self) -> usize {
        self.quote.load(Ordering::SeqCst)
            + self.kline.load(Ordering::SeqCst)
            + self.fx.load(Ordering::SeqCst)
    }
}

/// 批量取数面编排用例的通道束：行情三通道计到 [`QuoteChannelCalls`] 上并硬失败
/// （用例现场无行情标的），逐标的基金通道与批量取数面由用例注入。
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
        fetch_fx: Box::new({
            let calls = quote_calls.fx.clone();
            move |_: &str| {
                calls.fetch_add(1, Ordering::SeqCst);
                unreachable!("用例现场无行情通道标的")
            }
        }),
        fetch_nav_history,
        fetch_fund_name,
        // 货基判定确认（issue #1563）：由用例注入（缺信号桩 `no_confirm` 闭包或
        // 判定用例的确认桩）。
        confirm_money_fund_form,
        bulk,
    }
}

/// 批量面注入桩：给定载荷（名称字典 + 净值表），装配成一次取数面应答。
fn batch_stub(names: FundNameDictionary, nav: FundNavTable) -> FetchFundBatch {
    Box::new(move |_codes: &[String]| {
        let names = names.clone();
        let nav = nav.clone();
        Box::pin(async move { Ok(FundBatch { names, nav }) })
    })
}

/// 计数型批量面桩：载荷恒定，调用次数记到注入的计数器上——「批量面零请求」
/// 「批量面照试一次」两类断言的可观察面。
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

/// 批量取数面束：取数面桩 + 跨同步记忆句柄（issue #1565 单面）。
fn bulk_surfaces(
    funds: FetchFundBatch,
    circuit: Arc<Mutex<BulkFetchCircuit>>,
) -> BulkFetchSurfaces {
    BulkFetchSurfaces { funds, circuit }
}

/// 便捷装配：一次命中就返回给定名称字典与净值表，记忆句柄随构造独立发放。
fn batch_surfaces(names: FundNameDictionary, nav: FundNavTable) -> BulkFetchSurfaces {
    bulk_surfaces(
        batch_stub(names, nav),
        Arc::new(Mutex::new(BulkFetchCircuit::new())),
    )
}

/// 逐标的净值桩（新浪全历史通道，issue #1571）：固定返回单点历史（给定日期），
/// 并累加调用次数。
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

/// 空历史桩（窗口内无新净值）：即便被触达也不会写库，只留调用事实。
fn empty_nav(calls: Arc<AtomicUsize>) -> FetchNavHistory {
    Box::new(move |_: &str| {
        let calls = calls.clone();
        Box::pin(async move {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(vec![])
        })
    })
}

/// 逐标的名称桩：返回数据源权威名称（`权威名称-<代码>`），并累加调用次数。
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

/// 一次批量面用例运行的请求计数（「请求数常数级」断言的可观察面）。
#[derive(Debug, PartialEq, Eq)]
struct RequestCounts {
    quote: usize,
    per_fund_nav: usize,
    per_fund_name: usize,
    bulk_funds: usize,
}

#[test]
fn bulk_surfaces_pin_daily_sync_request_count_to_a_constant() {
    // 用户可观察结果（ADR-0121 验收 1）：233 只标的（其中 232 只场外基金）的账本，
    // 单次同步的取数请求数是常数级（新浪 `f_` 批量面 1 次，名称与最新净值同面
    // 返回），不再随标的数线性增长——本用例用 4 只与 232 只两个账本对照同一组断言。
    //
    // 账本取「有新净值的那一天」（水位昨日、批量面报今日）：这是日常最常态，也是
    // 「秒级」必须成立的那一天——当周新净值与当周采样点由批量面直接落库，逐只通道
    // 零请求。只覆盖「无新净值」的账本会漏掉这条性质。
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
            // 第 0 只刻意给陈旧名称：只有实际变化才落库的那条判据要有样本。
            let name = if i == 0 {
                "陈旧名称".to_string()
            } else {
                format!("权威名称-{code}")
            };
            seed_fund(&conn, &instrument_id, &code, &name);
            seed_fund_history(&conn, &instrument_id, &yesterday);
            codes.push(code);
        }
        // 无价格通道的行（自建债券）：同一账本里的跳过行，零请求。
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
                        // 逐只名称通道若被触达，名称不会被刷新——下面「仅变化者落库」
                        // 的断言同时是「这条通道没被用到」的证据。
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

        // 结果统计：基金现价全部由批量面落库，跳过行只算债券。
        assert_eq!(result.synced, fund_count);
        assert_eq!(result.skipped, 1);
        assert_eq!(result.written, fund_count, "当周新净值由批量面落现价");
        // 现价与净值日期取自批量面（来源标记不变，ADR-0121 决策 2）：`priced_at`
        // = `nav_date` = 批量面的净值日期。
        assert_eq!(
            fund_price_of(&conn, "inst-fund-000"),
            Some((35000, Some(today.clone()))),
            "现价由批量面的最新单位净值落库"
        );
        assert_eq!(result.renamed, 1, "只有名称实际变化的那只落库");
        assert_eq!(result.bulk_gaps, 0);
        assert!(!result.bulk_degraded, "批量面命中即不带降级事实");

        // 名称仍以数据源权威名称覆盖标的行名称：仅变化者落库、零变化不虚增版本。
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
    // ADR-0121 验收 3/4：批量面失败一律 fail-closed 回退逐只通道（价格与名称仍
    // 正确落库），失败记入跨同步记忆。
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
    // 价格与名称仍正确落库：逐只通道拿到的是水位次日的新净值与新权威名称。
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
    // ADR-0121 验收 6：批量面不承诺收录全部基金（新成立 / 已终止 / 清盘不在其列）
    // ——未收录的标的按**缺口**逐条回退补齐，价格与名称照常落库，且不触发熔断；
    // 缺口与失败在统计上分开（`bulk_gaps` vs `bulk_degraded`）。**货基不是缺口**：
    // 新浪 `f_` 面含货基，它以名称行在场、只是不产出价格点（issue #1565 /
    // ADR-0130 决策 6）——货基路径的用例见下方货币基金组。
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    // 三只基金：`f_` 面只收录前两只（第三只按「已终止 / 清盘」的现实缺席建模）。
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
    // 面收录的两只水位当日（无新净值，零逐只请求）；缺席的那只名称与净值都不出现。
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
    // 缺口标的的价格与历史仍正确落库（逐条回退补齐）。
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

    // 缺口不熔断：下一次同步仍照试批量面（请求数再 +1，逐只通道仍只服务缺口）。
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
    // ADR-0121 验收 5（跨同步记忆）：连续失败达阈值后停用批量面一个期限，停用期内
    // 零批量请求（避免在风控窗口里每轮都撞一次）；到期先半开试一次，成功即恢复。
    let today = beijing_today().format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-fund-0", "100000", "权威名称-100000");
    seed_fund_history(&conn, "inst-fund-0", &today);

    let per_fund_nav_calls = Arc::new(AtomicUsize::new(0));
    let bulk_calls = Arc::new(AtomicUsize::new(0));
    let circuit = Arc::new(Mutex::new(BulkFetchCircuit::new()));
    // 前 [`BULK_FAILURE_THRESHOLD`] 次同步的批量面失败，其后成功（半开试探与
    // 恢复的样本）。
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

    // 阈值内：每次同步都试一次批量面（失败逐只兜底），连续失败计数累加。
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

    // 停用期内：零批量请求，全部逐只兜底。
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

    // 停用到期（把窗口挪到过去即「期限已过」）：先半开试一次——成功即恢复常态。
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
    // 日常最常态，也是「秒级」必须成立的那一天：批量面报了比水位更新的净值（昨日
    // → 今日）。此时它的最新单位净值就是当日现价与该周采样点（ADR-0122 决策 2），
    // 直接落库、整只零请求——请求量不因「有新净值」而回到随标的数线性增长。
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
    // 当周采样点由批量取数落库：同周则整周覆盖（一条、取该周最新净值日），跨周则
    // 与上一周的采样点并存。
    let expected = if crate::incremental::week_monday(watermark_date)
        == crate::incremental::week_monday(today_date)
    {
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
    // 缺周点补齐（ADR-0122 决策 2）：水位落后批量面最新净值所在自然周超过一周
    // （应用数周未开），中间缺失的周点只有逐只窗口能补——逐只通道接管，从水位次日
    // 起增量补齐；价格与周采样照常落库。
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
    // 逐只通道返回整只历史（与新浪全历史面同形）：窗口外的陈旧点（水位前一周）
    // 与当日新点同在报文里——窗口裁剪把陈旧点挡在落库之外，只补窗口内缺失周点。
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
    // 水位与今日相隔两周（必跨 ISO 周）：窗口裁剪挡掉水位前的陈旧点，两条周采样
    // 点并存（存量水位周点 + 当周新点），新增点落在当周。
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
    // 首刷判据保护（ADR-0038 决策 6 / ADR-0122 决策 2 / issue #1377）：无历史序列
    // 的基金（按代码即拉已落现价缓存、后台补全尚未首刷），批量面的最新净值只
    // 落现价缓存——单点落进 price_history 会让「有历史序列」冒充「历史完整」，
    // 后台补全的近两年首刷将永久落空。
    let today_date = beijing_today();
    let yesterday = (today_date - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let today = today_date.format("%Y-%m-%d").to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-fund-0", "100000", "权威名称-100000");
    // 只落现价缓存（首刷前基金的真实形态）：水位有值、历史序列为空。
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

// ---------------------------------------------------------------------------
// 恒定价格标的（ADR-0126 / issue #1450、#1451）：退出采集链路——不进逐只
// 刷新、不进分母与缺口统计、零请求；未打标货基仍留在净值通道内、由逐只刷
// 新的数据源自报口径确认即打标（单向）、建档常量价兜底；恒定标的的现价缓
// 存不再随同步更新、不落周采样点（读侧按常量取值，落平坦行零信息）。
// ---------------------------------------------------------------------------

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

/// 恒定价格标的退出采集链路（ADR-0126 决策 4 / issue #1451）：不进逐只刷新、
/// 不进进度分母、不计批量面缺口——全库皆为恒定标的时零请求（两个批量面也
/// 不试）、不发进度事件，现价缓存（价格、净值日期、版本）与价格历史全部
/// 保持原样。删除豁免（把恒定通道放回净值分区）即重新发起逐只请求并计入
/// 分母与缺口，本用例变红（ADR-0126 投递纪律的守门判据）。
#[test]
fn constant_price_fund_gets_no_requests_and_is_excluded_from_denominator_and_gaps() {
    let today = beijing_today();
    let yesterday = (today - chrono::Duration::days(1))
        .format("%Y-%m-%d")
        .to_string();
    let conn = tauri_app_lib::test_support::open();
    seed_fund(&conn, "inst-const", "000198", "权威名称-000198");
    mark_constant(&conn, "inst-const");
    // 存量历史与现价缓存（打标前的采集遗留）：同步不得触碰。
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
    // 现价缓存逐位原样（价格 / 净值日期 / 版本）。
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

/// 未打标货基经现价刷新：官方披露自报形态（单位净值为空、万份收益与七日年化
/// 有值）确认即回填恒定标记并兜底建档常量价（1.0000、净值日期空），本轮**零
/// 逐只净值请求**、不落任何取值——万份收益不得经取数面冒充单位净值（#1342）。
///
/// 换源后的具体形态（issue #1565 / ADR-0130 决策 2/6）：新浪 `f_` 面含货基，
/// 它以**名称行**被批量面收录、但不产出价格点（取数层判形为 `MoneyYield`）——
/// 所以它不算缺口、也不落批量直落臂，而是自然落逐只臂由官方披露判定门收尾。
/// **负向接线证明（ADR-0087）**：删除 `refresh_one_fund_price` 的判定门（确认
/// 即打标收尾），货基按普通基金落库——现价被写成万份收益 0.2229 → 本用例的
/// 现价断言变红（市值口径回归 #1342 的错法）；把取数层的形态判别去掉（货基行
/// 产出价格点）同样使批量直落臂写错价 —— 本用例的逐只净值零请求/常量价断言变红。
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
            // 取数面的事实透传形态（判定口径退役后）：货基行的取值位是万份
            // 收益——判定门必须拦在落库前。
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

/// 缺信号（官方披露记录为普通净值形态或无记录）不打标、不清空、照常取数落库
/// ——缺信号不是「不是恒定标的」的反证（ADR-0126 决策 3）。
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

/// 官方披露源不可信（响应异常）：本轮整只不落——信号缺席时落取数面的取值位，
/// 万份收益就回冒充单位净值（#1342）；计入跳过、不报错不中断同步。
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
    // 名称通道返回原名称：本用例隔离判定门行为，名称随行刷新不产生写入。
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

/// 判定不进日常热路径：确认后标的退出采集链路——下一轮同步零确认请求、零逐只
/// 净值请求（编排层分区排除；删除排除即红：确认请求与净值请求重新出现）。
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

    // 下一轮：标的已退出采集链路（分区排除），零确认请求、零逐只净值请求。
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

/// 混合账本（场内行情 / 普通场外基金 / 恒定标的并存，ADR-0126 决策 4 / issue
/// #1451）：前三者的行为与请求数不变——行情批量报价照走、普通基金批量面直落
///（零逐只请求），恒定标的不进分母、不计缺口、零逐只请求（批量面结构上不
/// 覆盖它，也不为它退回逐只通道）。删除任一豁免即变红：恒定通道放回净值分区
/// → 分母 +1、缺口 +1、逐只请求 +1。
#[test]
fn mixed_ledger_keeps_constant_fund_out_of_requests_denominator_and_gaps() {
    let today = beijing_today();
    let today_s = today.format("%Y-%m-%d").to_string();
    // 水位取恰好七个自然日前：恒落在上一 ISO 周（周采样按周唯一键，不与本周点
    // 同周互覆），任意运行日断言确定性成立——种子用「昨天」会在周一跨周多出一行
    //（日历依赖缺陷，#1582 范围外修复）。
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
    // 普通场外基金：水位上周，批量面报今天 → 新净值直落（零逐只请求）。
    seed_fund(&conn, "inst-fund", "100001", "陈旧名称-基金");
    seed_fund_history(&conn, "inst-fund", &last_week);
    // 恒定标的：排行面无货币型桶（ADR-0126 背景），不出现在两个批量面里。
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
        fetch_fx: Box::new(|_| Box::pin(async { unreachable!("全仓 CNY，零汇率抓取") })),
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
    // 前三者的落库行为不变：基金批量面直落现价与当周采样点；恒定标的零触碰。
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
