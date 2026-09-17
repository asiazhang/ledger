//! 价格历史后台补全测试（ADR-0122 / issue #1375）：派生事实队列（有价格通道但
//! 历史不完整，持仓优先）、一轮排空（进度序列、基金首刷页级明细、单只失败继续
//! 排空、零写入不置见证）、队列随补全自然排空。编排经注入 mock 闭包驱动，不
//! 依赖真实网络；断言对准用户可观察结果（抓取了谁、落了什么、进度怎么推进），
//! 不内省私有队列结构。
//!
//! 基金历史回填单元（`backfill_one_fund_history`）的用例自
//! `instrument_info_sync.rs` 迁入（issue #1377 现价与历史解耦）：历史回填不再经
//! 手动同步编排，改为直接驱动逐只回填入口断言（首刷近两年、单请求全量通道优先
//! 与回退、水位增量、空响应/触顶整只不落库、单只一事务原子）。

use std::cell::RefCell;

use chrono::{Datelike, Days, Duration as ChronoDuration, Months, NaiveDate};
use rusqlite::{Connection, params};

use crate::SyncProgress;
use crate::fund_backfill::{BackfillOutcome, backfill_one_fund_history};
use crate::fund_nav::{LsjzPage, NavPoint, NavQuery};
use crate::history::{HistoryBackfillStats, run_history_backfill_round};
use crate::http::KlineBar;
use crate::incremental::{SyncInstrument, beijing_today, week_monday};
use crate::model::WriteWitness;
use crate::tests::insert_holding;
use ledger_infra::error::{AppError, Result};
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, price_value_to_cents, upsert_market_price,
    upsert_price_history,
};
use ledger_investment::{InstrumentType, derive_price_channel};
use tauri_app_lib::test_support::seed_instrument;

fn bar(date: &str, close: f64) -> KlineBar {
    KlineBar {
        date: date.to_string(),
        close,
    }
}

fn nav_page(total: u64, points: &[(&str, f64)]) -> LsjzPage {
    LsjzPage {
        total,
        blocked: false,
        points: points
            .iter()
            .map(|(d, n)| NavPoint {
                date: d.to_string(),
                nav: *n,
            })
            .collect(),
    }
}

/// 直插一条无持仓标的（seed_instrument 固定 stock 类型；kind ≠ stock 时就地修正
/// ——类型是通道派生的行为输入）。
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

/// 给标的落一条历史周点 + 现价缓存（nav_date 兼任基金水位）。
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

/// 相对今天偏移 N 天的 ISO 日期（缺周点判据的测试基线）。
fn date_offset(days: i64) -> String {
    (beijing_today() - ChronoDuration::days(days))
        .format("%Y-%m-%d")
        .to_string()
}

fn history_rows(conn: &Connection, instrument_id: &str) -> i64 {
    conn.query_row(
        "SELECT count(*) FROM price_history WHERE instrument_id=?1",
        params![instrument_id],
        |r| r.get(0),
    )
    .unwrap()
}

/// 进度事件的断言投影：标的级 done/total + 可缺省页级明细三元组（消除
/// 断言处的复杂类型表达式）。
type ObservedProgress = (usize, usize, Option<(String, u64, u64)>);

/// 一轮补全的注入桩集合：按 secid / 基金代码应答并记录「谁被抓取了」——队列
/// 成员与排队顺序都经这份可观察日志断言。
struct Harness {
    /// 处理序日志：行情标的记 `kline:<secid>`，基金页记 `nav:<code>`，基金单
    /// 请求全量通道记 `full:<code>`，汇率记 `fx:<pair>`。
    log: RefCell<Vec<String>>,
    /// secid → 日线样本；未命中 = 空表（零有效周点）。
    klines: Vec<(&'static str, Vec<KlineBar>)>,
    /// 注入单只失败：命中该 secid 的日 K 请求返回 Err。
    fail_kline: Option<&'static str>,
    /// 基金代码 → (服务端总条数, 按页的净值页序列)；未收录 = 空页。
    nav_pages: Vec<(&'static str, u64, Vec<LsjzPage>)>,
    /// 币种对 → 汇率日线。
    fx: Vec<(&'static str, Vec<KlineBar>)>,
}

impl Harness {
    fn new() -> Self {
        Self {
            log: RefCell::new(vec![]),
            klines: vec![],
            fail_kline: None,
            nav_pages: vec![],
            fx: vec![],
        }
    }

    fn with_klines(mut self, klines: Vec<(&'static str, Vec<KlineBar>)>) -> Self {
        self.klines = klines;
        self
    }

    fn with_failing_kline(mut self, secid: &'static str) -> Self {
        self.fail_kline = Some(secid);
        self
    }

    fn with_nav_pages(mut self, pages: Vec<(&'static str, u64, Vec<LsjzPage>)>) -> Self {
        self.nav_pages = pages;
        self
    }

    fn with_fx(mut self, fx: Vec<(&'static str, Vec<KlineBar>)>) -> Self {
        self.fx = fx;
        self
    }

    fn requested(&self) -> Vec<String> {
        self.log.borrow().clone()
    }

    fn nav_page_hits(&self, code: &str) -> usize {
        self.requested()
            .iter()
            .filter(|entry| entry.as_str() == format!("nav:{code}").as_str())
            .count()
    }

    fn fetch_kline(&self, secid: &str) -> Result<Vec<KlineBar>> {
        self.log.borrow_mut().push(format!("kline:{secid}"));
        if self.fail_kline.map(|f| f == secid).unwrap_or(false) {
            return Err(AppError::Io("日 K 抓取失败".into()));
        }
        Ok(self
            .klines
            .iter()
            .find(|(key, _)| *key == secid)
            .map(|(_, bars)| bars.clone())
            .unwrap_or_default())
    }

    fn fetch_nav(&self, query: &NavQuery) -> Result<LsjzPage> {
        self.log.borrow_mut().push(format!("nav:{}", query.code));
        let page_index = (query.page - 1) as usize;
        Ok(self
            .nav_pages
            .iter()
            .find(|(code, _, _)| *code == query.code)
            .and_then(|(_, _, pages)| pages.get(page_index).cloned())
            .unwrap_or(LsjzPage {
                points: vec![],
                total: 0,
                blocked: false,
            }))
    }

    fn fetch_nav_full(&self, code: &str) -> Result<Vec<NavPoint>> {
        self.log.borrow_mut().push(format!("full:{code}"));
        // 单请求全量通道统一失败：让首刷走分页通道，页级明细由此可观察（与
        // 既有首刷用例同路——fail-closed 回退分页）。
        Err(AppError::Io("全量通道不可用".into()))
    }

    fn fetch_fx(&self, pair: &str) -> Result<Vec<KlineBar>> {
        self.log.borrow_mut().push(format!("fx:{pair}"));
        Ok(self
            .fx
            .iter()
            .find(|(key, _)| *key == pair)
            .map(|(_, bars)| bars.clone())
            .unwrap_or_default())
    }
}

/// 驱动一轮补全：返回 (统计, 进度序列, 是否实际写过)。
fn run_round(
    conn: &Connection,
    harness: &Harness,
) -> (Result<HistoryBackfillStats>, Vec<SyncProgress>, bool) {
    let progress_log: RefCell<Vec<SyncProgress>> = RefCell::new(vec![]);
    let mut witness = WriteWitness::default();
    let mut fetch_kline = |secid: &str| harness.fetch_kline(secid);
    let mut fetch_fx = |pair: &str| harness.fetch_fx(pair);
    let mut fetch_nav = |query: &NavQuery| harness.fetch_nav(query);
    let mut fetch_nav_full = |code: &str| harness.fetch_nav_full(code);
    let mut progress = |p: SyncProgress| progress_log.borrow_mut().push(p);
    let result = run_history_backfill_round(
        conn,
        &mut fetch_kline,
        &mut fetch_fx,
        &mut fetch_nav,
        &mut fetch_nav_full,
        &mut progress,
        &mut witness,
    );
    (result, progress_log.into_inner(), witness.any_written())
}

// ---------------------------------------------------------------------------
// 队列 = 派生事实（有价格通道但历史不完整，持仓优先）
// ---------------------------------------------------------------------------

/// 队列成员与排队顺序：历史完整 / 水位新鲜的通道内标的不进队，手动报价与
/// 无来源行不进队；持仓标的前于非持仓标的（symbol 更小也不插队），同级按
/// symbol 升序。
#[test]
fn queue_keeps_only_incomplete_and_holding_comes_first() {
    let conn = tauri_app_lib::test_support::open();
    let fresh = date_offset(2);
    let stale = date_offset(14);

    // ① 持仓股票、无历史（首刷 → 进队）。
    insert_holding(&conn, "acc-1", "inst-held", "600519", "stock", "CNY", "sh");
    // ② 非持仓股票、历史新鲜（不进队）。
    insert_plain_instrument(&conn, "inst-fresh", "000002", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-fresh", "CNY", &fresh);
    // ③ 非持仓股票、历史缺周点（进队）。
    insert_plain_instrument(&conn, "inst-stale", "000003", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-stale", "CNY", &stale);
    // ④ 非持仓基金、无历史（首刷 → 进队；symbol 最小但不插持仓的队）。
    insert_plain_instrument(&conn, "inst-fund", "000001", "fund", "CNY", "unknown");
    // ⑤ 基金、水位新鲜（不进队）。
    insert_plain_instrument(&conn, "inst-fund-fresh", "000004", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund-fresh", "CNY", &fresh);
    // ⑥ 基金、水位缺周点（进队）。
    insert_plain_instrument(&conn, "inst-fund-stale", "000005", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund-stale", "CNY", &stale);
    // ⑦ 手动报价通道（债券）与 ⑧ 无来源行（股票、市场未知）不进队。
    insert_plain_instrument(&conn, "inst-bond", "BOND-X", "bond", "CNY", "unknown");
    insert_plain_instrument(&conn, "inst-nosrc", "NO-MKT", "stock", "CNY", "unknown");

    let harness = Harness::new()
        .with_klines(vec![
            ("1.600519", vec![bar(&date_offset(1), 13.0)]),
            ("0.000003", vec![bar(&date_offset(1), 7.0)]),
        ])
        .with_nav_pages(vec![
            ("000001", 1, vec![nav_page(1, &[(&date_offset(1), 1.5)])]),
            ("000005", 1, vec![nav_page(1, &[(&date_offset(1), 2.5)])]),
        ]);

    let (result, _progress, _written) = run_round(&conn, &harness);
    let stats = result.unwrap();

    // 队列成员 = 首刷持仓股 + 缺周点股 + 首刷基金 + 缺周点基金 = 4 只。
    assert_eq!(stats.queued, 4, "只有历史不完整的通道内标的进队");
    assert_eq!(stats.failed, 0);
    // 排队顺序持仓优先：600519（持仓）先于全部非持仓标的（其中 000001 的
    // symbol 最小也不插队），非持仓同级按 symbol 升序。基金首刷先试单请求
    // 全量通道、失败 fail-closed 回退分页（000001 是 full: → nav: 相邻）；
    // 已有历史者直接走水位增量分页（000005 无 full: 条目）。
    assert_eq!(
        harness.requested(),
        vec![
            "kline:1.600519".to_string(),
            "full:000001".to_string(),
            "nav:000001".to_string(),
            "kline:0.000003".to_string(),
            "nav:000005".to_string(),
        ],
        "抓取序 = 持仓优先 + symbol 升序"
    );
    // 全部落库（首刷一只一事务，issue #1373）：已有历史者新增本周邻域的新周点
    // （种子周点仍在），首刷者从零落一根。
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

/// 基金「已是最新」的补全幂等：水位缺周点触发进队，但增量窗口内无新净值时
/// 整只零落库、不计失败（处理完成照常推进，与手动同步同口径）。
#[test]
fn fund_with_no_new_nav_completes_without_writing() {
    let conn = tauri_app_lib::test_support::open();
    let stale = date_offset(14);
    insert_plain_instrument(&conn, "inst-fund", "000001", "fund", "CNY", "unknown");
    seed_history_point(&conn, "inst-fund", "CNY", &stale);

    // 增量窗口（水位次日 → 今天）空页且非 blocked：「窗口内确实无新净值」。
    let harness = Harness::new().with_nav_pages(vec![("000001", 0, vec![nav_page(0, &[])])]);
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

/// 队列随补全自然排空：本轮补齐的标的按派生事实退出队列——再跑一轮零队列、
/// 零进度、零请求（「关掉应用再打开会接着补，而不是每次从头来」的域内根据）。
#[test]
fn queue_drains_and_stays_empty_once_histories_complete() {
    let conn = tauri_app_lib::test_support::open();
    insert_holding(&conn, "acc-1", "inst-held", "600519", "stock", "CNY", "sh");
    let harness = Harness::new().with_klines(vec![(
        "1.600519",
        vec![bar(&date_offset(8), 12.0), bar(&date_offset(1), 13.0)],
    )]);

    let (result, progress, written) = run_round(&conn, &harness);
    result.unwrap();
    assert_eq!(progress.len(), 2, "首轮 0/1、1/1 两格");
    assert!(written, "首轮实际落库");

    // 第二轮：最新周点已在本周邻域 → 队列空 → 零动作。
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
        "第二轮零日 K 请求"
    );
}

// ---------------------------------------------------------------------------
// 一轮排空的可观察行为
// ---------------------------------------------------------------------------

/// 进度序列：收集完成立即发 {0, total}，此后逐只推进到 {total, total}。
#[test]
fn round_emits_instrument_level_progress_sequence() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    insert_plain_instrument(&conn, "inst-b", "000003", "stock", "CNY", "sz");
    let harness = Harness::new().with_klines(vec![
        ("0.000002", vec![bar(&date_offset(1), 5.0)]),
        ("0.000003", vec![bar(&date_offset(1), 6.0)]),
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

/// 基金首刷的页级明细（issue #1061 明细随首刷深回填迁入本任务）：分页通道
/// 翻页期间进度事件带出「基金代码 + 已完成页/总页数」，标的级 done/total 不因
/// 页推进而变。
#[test]
fn fund_first_fill_carries_page_detail_progress() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "000001", "fund", "CNY", "unknown");
    // 总条数 40 → 2 页（服务端页大小 20）：页 1 抓取返回后报告 1/2，页 2 返回
    // 后报告 2/2；两页净值点攒齐后一只一事务落库。
    let harness = Harness::new().with_nav_pages(vec![(
        "000001",
        40,
        vec![
            nav_page(40, &[(&date_offset(20), 1.0)]),
            nav_page(40, &[(&date_offset(2), 1.2)]),
        ],
    )]);
    let (result, progress, _written) = run_round(&conn, &harness);
    result.unwrap();
    let with_fund: Vec<ObservedProgress> = progress
        .iter()
        .map(|p| {
            (
                p.done,
                p.total,
                p.fund.as_ref().map(|f| (f.code.clone(), f.page, f.pages)),
            )
        })
        .collect();
    assert_eq!(
        with_fund,
        vec![
            (0, 1, None),
            (0, 1, Some(("000001".into(), 1, 2))),
            (0, 1, Some(("000001".into(), 2, 2))),
            (1, 1, None),
        ],
        "页级明细只在页抓取返回后发出，done/total 保持标的级口径"
    );
    assert_eq!(harness.nav_page_hits("000001"), 2, "按服务端总数翻两页");
}

/// 单只失败不中断本轮：第一只的日 K 抓取失败，第二只照常补齐、进度照常推进、
/// 实际写过照常入见证（成败同判的证据源）。
#[test]
fn round_continues_after_single_instrument_failure() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    insert_plain_instrument(&conn, "inst-b", "000003", "stock", "CNY", "sz");
    let harness = Harness::new()
        .with_failing_kline("0.000002")
        .with_klines(vec![("0.000003", vec![bar(&date_offset(1), 6.0)])]);
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

/// 全部无新点的重采（停牌/退市股持续在队的每日形态）：日 K 返回与库内完全
/// 相同的周点 → 零写入、不置见证（收尾裁决口径「全部无新点不置脏不广播」，
/// ADR-0122）；返回同周不同值或新增周点 → 照常落库并置见证。
#[test]
fn stock_unchanged_kline_writes_nothing_but_changed_value_rewrites() {
    let conn = tauri_app_lib::test_support::open();
    let stale = date_offset(14);
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    seed_history_point(&conn, "inst-a", "CNY", &stale);

    // ① 同日同值：零写入、零见证。
    let harness = Harness::new().with_klines(vec![("0.000002", vec![bar(&stale, 10.0)])]);
    let (result, _progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!((stats.queued, stats.failed), (1, 0));
    assert!(!written, "全部无新点不置见证");

    // ② 同周不同值：有新值 → 照常落库（整周覆盖幂等）并置见证。
    let harness = Harness::new().with_klines(vec![("0.000002", vec![bar(&stale, 10.5)])]);
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

    // ③ 新增周点：照常落库。
    let fresh = date_offset(1);
    let harness = Harness::new().with_klines(vec![(
        "0.000002",
        vec![bar(&stale, 10.5), bar(&fresh, 11.0)],
    )]);
    let (result, _progress, written) = run_round(&conn, &harness);
    result.unwrap();
    assert!(written);
    assert_eq!(history_rows(&conn, "inst-a"), 2);
}

/// 空 K 线（窗口内无有效样本）：单只处理完成但零落库，不置见证（零写入不广播）。
#[test]
fn empty_kline_completes_without_writing() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-a", "000002", "stock", "CNY", "sz");
    let harness = Harness::new(); // kline 未命中 = 空表
    let (result, _progress, written) = run_round(&conn, &harness);
    let stats = result.unwrap();
    assert_eq!(stats.queued, 1);
    assert_eq!(stats.failed, 0);
    assert!(!written, "零有效周点 = 零写入");
    assert_eq!(history_rows(&conn, "inst-a"), 0);
}

/// 汇率 K 线同期补齐：队列内标的的非本位币币种对按对拉取（折算序列与价格
/// 历史同期段采集，与手动同步同一单元）；零外币队列不触发汇率请求。
#[test]
fn round_backfills_fx_pairs_for_queued_currencies() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-us", "AAPL", "stock", "USD", "nasdaq");
    let harness = Harness::new()
        .with_klines(vec![("105.AAPL", vec![bar(&date_offset(1), 319.97)])])
        .with_fx(vec![("USDCNY", vec![bar(&date_offset(1), 7.02)])]);
    let (result, _progress, _written) = run_round(&conn, &harness);
    result.unwrap();
    let fx_rows: i64 = conn
        .query_row(
            "SELECT count(*) FROM fx_rate_history WHERE base_code='USD' AND quote_code='CNY'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(fx_rows, 1, "USDCNY 周点落 fx_rate_history");
    assert!(harness.requested().contains(&"fx:USDCNY".to_string()));
}

/// 缺周点判据的边界自证：6 天前的周点按 ISO 周差至多落后一周（≤ 7 天），
/// 不进队列——常态跨周不触发补采；14 天前必然跨两周，进队列。
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

// ---------------------------------------------------------------------------
// 车道机制（issue #1375 额度让路）
// ---------------------------------------------------------------------------

/// 让行语义（车道机制单元面）：前台在途守卫生效期间，后台让行等待不放行；
/// 守卫释放（RAII）后放行。删除前台计数（守卫不计数）本测试红——等待永不过
/// 界却立即放行。
#[test]
fn background_lane_yields_while_foreground_in_flight() {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    let foreground = crate::http::ForegroundGuard::enter();
    let (done_tx, done_rx) = mpsc::channel();
    let waiter = thread::spawn(move || {
        crate::http::block_on(crate::http::wait_foreground_idle());
        done_tx.send(()).expect("放行通知应可送达");
    });
    // 前台在途：给后台足够时间误判归零，仍未放行即让行成立。
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

/// 共享限速器为进程级单例：两条车道束（手动同步 / 后台补全各自构造）拿到的
/// 是同一份 Pacer——「与前台共享同一全局限速器」的机制前提。
#[test]
fn shared_pacer_is_process_wide_singleton() {
    use std::sync::Arc;
    assert!(Arc::ptr_eq(
        &crate::http::shared_pacer(),
        &crate::http::shared_pacer(),
    ));
}

// ---------------------------------------------------------------------------
// 基金历史回填单元（backfill_one_fund_history，issue #1377 自
// instrument_info_sync.rs 迁入）：首刷近两年、单请求全量通道优先与 fail-closed
// 回退、水位次日增量、同周整周覆盖幂等、空响应/页数触顶整只不落库、单只一
// 事务原子。原经手动同步编排的 synced/skipped 统计断言改为直接断言
// BackfillOutcome 与库内行（price_history / market_prices）。
// ---------------------------------------------------------------------------

/// 基金标的的同步投影（直驱 [`backfill_one_fund_history`] 的入参构造）：场外
/// 基金常态 CNY + 市场未知，通道由投资域单点派生。
fn fund_instrument(id: &str, code: &str) -> SyncInstrument {
    SyncInstrument {
        instrument_id: id.to_string(),
        symbol: code.to_string(),
        market: "unknown".to_string(),
        currency: "CNY".to_string(),
        channel: derive_price_channel(InstrumentType::Fund, "unknown", code),
    }
}

/// 直驱逐只回填：页级推进回调置空（页级明细断言归上方 run_history_backfill_round
/// 的用例，此处不重复），返回结局。
fn run_fund_backfill<N, S>(
    conn: &Connection,
    fund: &SyncInstrument,
    fetch_nav: &mut N,
    fetch_full: &mut S,
) -> Result<BackfillOutcome>
where
    N: FnMut(&NavQuery) -> Result<LsjzPage>,
    S: FnMut(&str) -> Result<Vec<NavPoint>>,
{
    backfill_one_fund_history(
        conn,
        fund,
        fetch_nav,
        fetch_full,
        &mut |_done: u64, _total: u64| {},
    )
}

/// 空实现：既有用例只关心单请求全量通道时注入（分页通道最小桩，形态 = 窗口
/// 内确实无净值、非拦截）。
fn empty_nav(_: &NavQuery) -> Result<LsjzPage> {
    Ok(LsjzPage {
        points: vec![],
        total: 0,
        blocked: false,
    })
}

/// 模拟历史净值页抓取：按代码返回页序列（下标 = 页码 − 1，越界页返回空），
/// 并记录全部查询（断言水位窗口、翻页与「非可拉取行零请求」）。
fn mock_nav<'a>(
    pages_by_code: &'a [(&'a str, Vec<LsjzPage>)],
    requested: &'a RefCell<Vec<NavQuery>>,
) -> impl FnMut(&NavQuery) -> Result<LsjzPage> + 'a {
    move |query: &NavQuery| {
        requested.borrow_mut().push(query.clone());
        Ok(pages_by_code
            .iter()
            .find(|(c, _)| *c == query.code)
            .and_then(|(_, pages)| pages.get((query.page - 1) as usize))
            .cloned()
            .unwrap_or(LsjzPage {
                points: vec![],
                total: 0,
                blocked: false,
            }))
    }
}

/// 日序列（日期升序的 (日期, 单位净值)）→ 单请求全量通道的净值点序列。
fn full_series(series: &[(String, f64)]) -> Vec<NavPoint> {
    series
        .iter()
        .map(|(date, nav)| NavPoint {
            date: date.clone(),
            nav: *nav,
        })
        .collect()
}

/// 模拟单请求全量净值通道：按代码返回整只基金的**全部历史**单位净值，并记录
/// 请求的代码（断言首刷一次请求、增量不触碰本通道）。
fn mock_full_nav<'a>(
    series_by_code: &'a [(&'a str, Vec<NavPoint>)],
    requested: &'a RefCell<Vec<String>>,
) -> impl FnMut(&str) -> Result<Vec<NavPoint>> + 'a {
    move |code: &str| {
        requested.borrow_mut().push(code.to_string());
        Ok(series_by_code
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, points)| points.clone())
            .unwrap_or_default())
    }
}

/// 逐交易日净值序列（周一至周五各一条、日期升序、单位净值温和上抬）：真实净值
/// 按日公布，周线回填的覆盖深度由降采样后每周一点体现。
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

/// 窗口敏感的历史净值页 mock（页大小 = 服务端硬上限 20，返回前按日期降序）：
/// 按 `[start_date, end_date]` 闭区间从固定序列里过滤，`total` = 窗口内条数。
/// 窗口起点错（如把「添加时写入的净值日期」当增量水位）会直接少采或不采净值点，
/// 于是「首刷回填补齐两年」的断言对准同步后可查询到的周点覆盖深度，而不是函数
/// 或调用形状（issue #1059 负向条目）。
fn mock_nav_series<'a>(
    series: &'a [(String, f64)],
    requested: &'a RefCell<Vec<NavQuery>>,
) -> impl FnMut(&NavQuery) -> Result<LsjzPage> + 'a {
    move |query: &NavQuery| {
        requested.borrow_mut().push(query.clone());
        let mut in_window: Vec<&(String, f64)> = series
            .iter()
            .filter(|(date, _)| {
                date.as_str() >= query.start_date.as_str()
                    && date.as_str() <= query.end_date.as_str()
            })
            .collect();
        in_window.sort_by(|a, b| b.0.cmp(&a.0));
        let total = in_window.len() as u64;
        let points = in_window
            .into_iter()
            .skip(((query.page - 1) * 20) as usize)
            .take(20)
            .map(|(date, nav)| NavPoint {
                date: date.clone(),
                nav: *nav,
            })
            .collect();
        Ok(LsjzPage {
            points,
            total,
            blocked: false,
        })
    }
}

/// 近两年首刷窗口起点（与 nav_window 同式，测试侧独立重算）。
fn expected_first_sync_start() -> String {
    beijing_today()
        .checked_sub_months(Months::new(24))
        .unwrap()
        .format("%Y-%m-%d")
        .to_string()
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

/// 注入「第 3 个周点（2026-01-19 当周）写入失败」的测试侧故障，供 ADR-0122
/// 决策 8 / issue #1373 的负向判据共用：`BEFORE INSERT` 触发器
/// `RAISE(ABORT)`，产品代码零 hook。降采样按周升序落库，故前两个周点先写、
/// 第三个失败——逐周点提交会留下前两行，整只一次提交回滚后零行。
fn inject_week_write_failure(conn: &Connection) {
    conn.execute_batch(
        "CREATE TRIGGER inject_week_write_failure BEFORE INSERT ON price_history \
         WHEN NEW.trade_date='2026-01-19' \
         BEGIN SELECT RAISE(ABORT, '注入周点写入失败'); END;",
    )
    .unwrap();
}

/// 种下「添加基金路径的产物」现场：现价缓存有净值日期（水位），PriceHistory
/// 可选带一根历史点（issue #1059 的现场表达）。
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
fn fund_first_sync_backfills_two_years_with_cross_page_weekly() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    // 首刷（无水位）：窗口 = 近两年。total=45 → 3 页；净值页按日期降序返回，
    // 第 1/2 页跨页同属 ISO 周（2026-01-26 起）——攒齐后一次降采样必须取该周
    // 最后一个净值日（01-30 周五），逐页落库会被后页的更早日期覆盖。
    let pages = [(
        "110022",
        vec![
            nav_page(45, &[("2026-01-30", 3.348), ("2026-01-28", 3.293)]),
            nav_page(45, &[("2026-01-26", 3.25), ("2025-12-31", 3.1)]),
            nav_page(45, &[]),
        ],
    )];
    let requested = RefCell::new(Vec::new());
    let full_requested = RefCell::new(Vec::new());
    let mut nav = mock_nav(&pages, &requested);
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert!(outcome.written, "首刷跨页回填应落库");
    assert!(!outcome.inconclusive);

    // 翻页：首页起点 = 近两年窗口起点，共 3 页、全部同窗口。
    let requested = requested.borrow();
    assert_eq!(requested.len(), 3);
    for q in requested.iter() {
        assert_eq!(q.code, "110022");
        assert_eq!(q.start_date, expected_first_sync_start());
    }
    assert_eq!(requested[0].page, 1);
    assert_eq!(requested[1].page, 2);
    assert_eq!(requested[2].page, 3);

    // 周采样：跨页同周取最后净值日；单位净值 ×10000 得万分之一元（ADR-0038）。
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![
            ("2025-12-31".into(), 31000, "CNY".into()),
            ("2026-01-30".into(), 33480, "CNY".into()),
        ],
    );

    // 现价 = 窗口内最新公布单位净值，priced_at = nav_date = 净值日期（下次水位）。
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
    );
}

#[test]
fn fund_first_sync_prefers_single_request_full_series() {
    // issue #1062：首刷一次请求拿整只基金历史净值并裁剪到近两年窗口，替代约 25
    // 次分页请求；窗口外更早的点被裁剪掉（回填深度语义不变）。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let window_start = beijing_today().checked_sub_months(Months::new(24)).unwrap();
    let five_years_ago = beijing_today().checked_sub_months(Months::new(60)).unwrap();
    let series = daily_nav_series(five_years_ago, beijing_today());
    let full_by_code = [("110022", full_series(&series))];
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&full_by_code, &full_requested);
    let page_requested = RefCell::new(Vec::new());
    let mut nav = mock_nav(&[], &page_requested);

    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    {
        let requested = full_requested.borrow();
        assert_eq!(requested.len(), 1, "首刷每次一条请求");
        assert_eq!(requested[0], "110022", "按基金代码取全量");
    }
    assert!(
        page_requested.borrow().is_empty(),
        "单请求通道命中后不得再分页（一次请求取代约 25 次）"
    );

    // 落库覆盖深度 = 近两年周线；窗口外更早的点被裁剪掉。
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
}

#[test]
fn fund_first_sync_full_series_failure_falls_back_to_pages() {
    // 单请求通道不可信（解析失败——生产就是「数据文件缺少可信单位净值序列」这条
    // 错误）：fail-closed 回退既有分页通道，分页结果照常落库——不静默丢数据。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let mut full = |_: &str| -> Result<Vec<NavPoint>> {
        Err(AppError::Parse(
            "基金 110022 详情页数据文件缺少可信的单位净值序列".into(),
        ))
    };
    let requested = RefCell::new(Vec::new());
    let pages = [(
        "110022",
        vec![nav_page(2, &[("2026-01-30", 3.348), ("2026-01-29", 3.42)])],
    )];
    let mut nav = mock_nav(&pages, &requested);

    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(requested.borrow().len(), 1, "回退分页通道");
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![("2026-01-30".into(), 33480, "CNY".into())],
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
    );
}

#[test]
fn fund_first_sync_full_series_empty_falls_back_to_pages() {
    // 单请求通道结构完好但为空（新基金未公布净值 / 裁剪后无窗口内点）：同样回退
    // 分页通道，不让一条不确定的空结果直接决定「无净值」。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let mut full = |_: &str| -> Result<Vec<NavPoint>> { Ok(vec![]) };
    let requested = RefCell::new(Vec::new());
    let pages = [("110022", vec![nav_page(1, &[("2026-01-30", 3.348)])])];
    let mut nav = mock_nav(&pages, &requested);

    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(requested.borrow().len(), 1, "空结果回退分页通道");
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
    );
}

#[test]
fn fund_first_sync_full_series_without_window_points_falls_back_to_pages() {
    // 退市 / 清仓多年的基金：单请求通道返回的点全在近两年窗口外——裁剪为空后
    // 回退分页通道，不在窗口内凭空造点。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let stale = vec![NavPoint {
        date: "2010-08-20".into(),
        nav: 1.0,
    }];
    let full_by_code = [("110022", stale)];
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&full_by_code, &full_requested);
    let requested = RefCell::new(Vec::new());
    let pages = [("110022", vec![nav_page(0, &[])])];
    let mut nav = mock_nav(&pages, &requested);

    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert_eq!(requested.borrow().len(), 1, "窗口外点回退分页通道");
    assert_eq!(full_requested.borrow().len(), 1, "首刷先查单请求通道");
    assert_eq!(price_history_rows(&conn, "inst-fund"), vec![]);
    assert!(!outcome.written, "查无窗口内净值不落库（原断言：计入跳过）");
    assert!(
        !outcome.inconclusive,
        "查无窗口内净值结局确定（非「窗口不完整」，原断言：计跳过而非失败）"
    );
}

#[test]
fn fund_incremental_does_not_touch_single_request_full_series() {
    // 日常增量仍走既有历史净值接口：有历史序列的基金不发起单请求全量查询，
    // 即使单请求通道返回别值也不被消费（水位语义与 #1059 一致）。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");
    seed_fund_price(&conn, "inst-fund", 30000, "2026-01-28", true);

    let full_by_code = [(
        "110022",
        vec![NavPoint {
            date: "2026-01-30".into(),
            nav: 9.99,
        }],
    )];
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&full_by_code, &full_requested);
    let requested = RefCell::new(Vec::new());
    let pages = [(
        "110022",
        vec![nav_page(2, &[("2026-01-30", 3.348), ("2026-01-29", 3.42)])],
    )];
    let mut nav = mock_nav(&pages, &requested);

    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert!(outcome.written);
    assert_eq!(requested.borrow().len(), 1, "增量走分页通道");
    assert!(
        full_requested.borrow().is_empty(),
        "有历史序列的增量不触碰单请求全量通道"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
        "取分页通道的净值，不采信单请求通道的另一值"
    );
}

#[test]
fn fund_incremental_fetches_from_watermark_and_overwrites_same_week() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    // 水位 = 现价缓存的净值日期 01-28（周三），上一轮已把该周采样写到周三。
    seed_fund_price(&conn, "inst-fund", 30000, "2026-01-28", true);
    // 更早一周的历史点应原样保留（增量不回看）。
    upsert_price_history(
        &conn,
        "inst-fund",
        "2026-01-23",
        31000,
        "CNY",
        EASTMONEY_PRICE_SOURCE,
    )
    .unwrap();

    // 窗口 = 水位次日起，单页两行（total=2 → 1 页）：周四、周五新净值；
    // 周五与水位同周——该周采样整周覆盖为周五。
    let pages = [(
        "110022",
        vec![nav_page(2, &[("2026-01-30", 3.348), ("2026-01-29", 3.42)])],
    )];
    let requested = RefCell::new(Vec::new());
    let mut nav = mock_nav(&pages, &requested);
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);

    let requested = requested.borrow();
    assert_eq!(requested.len(), 1, "常态增量每只一页");
    assert_eq!(requested[0].start_date, "2026-01-29", "从水位次日起");

    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![
            ("2026-01-23".into(), 31000, "CNY".into()),
            ("2026-01-30".into(), 33480, "CNY".into()),
        ],
        "水位当日不重拉；同周新净值整周覆盖采样日"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((33480, Some("2026-01-30".into()))),
    );
}

#[test]
fn fund_incremental_up_to_date_counts_synced_without_write() {
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    // 水位较新（一周内），窗口内无新净值（mock 返回空页）；已有历史序列：
    // 水位只在「已回填过」的基金上作增量起点（issue #1059）。
    let watermark = date_offset(7);
    seed_fund_price(&conn, "inst-fund", 30000, &watermark, true);

    let requested = RefCell::new(Vec::new());
    let mut nav = mock_nav(&[], &requested);
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    // 「已是最新」= 结局确定但不落库：written 为 false、非「窗口不完整」
    //（原断言：synced 计入、written 为 0、零变化不广播）。
    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: false
        },
        "已是最新：结局确定且零写入"
    );
    assert!(full_requested.borrow().is_empty(), "有历史序列不碰全量通道");
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
fn fund_with_nav_date_but_no_history_backfills_two_years() {
    // issue #1059：添加基金 / AI 导入在「按代码即拉」时已把最新净值日期写进
    // 现价缓存（水位有值），但这只基金没有任何历史序列——首刷判据必须是
    // 「磁盘上有无历史序列」，不是「水位是否存在」。#303 的首刷回填验收在真实
    // 账本上未成立，根因即在此。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    // 添加基金路径的产物：现价缓存有净值日期（水位），PriceHistory 为空。
    let watermark = date_offset(1);
    seed_fund_price(&conn, "inst-fund", 35000, &watermark, false);
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![],
        "前置：无任何历史序列（水位冒充已回填的现场）"
    );

    // 近两年逐交易日净值序列 + 窗口敏感 mock：起点若按水位增量取，窗口会被压成
    // 一天、几乎采不到净值点（现价也就不再更新）。
    let start = beijing_today().checked_sub_months(Months::new(24)).unwrap();
    let series = daily_nav_series(start, beijing_today());
    let requested = RefCell::new(Vec::new());
    let mut nav = mock_nav_series(&series, &requested);
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    assert!(outcome.written);
    assert!(!outcome.inconclusive);
    assert_eq!(full_requested.borrow().len(), 1, "首刷先试单请求全量通道");

    // 回填后可查询到的净值周点覆盖深度 = 近两年（约 104 周），而不是「水位之后
    // 的那几天」——断言对准周点覆盖深度，不对准函数或调用形状。
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

    // 现价 = 窗口内最新公布净值（序列末点），与 #301 添加基金同形。
    let latest = series.last().unwrap();
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((price_value_to_cents(latest.1), Some(latest.0.clone()))),
    );
}

#[test]
fn fund_blocked_empty_response_with_watermark_is_not_counted_synced() {
    // issue #1059：有水位、有历史序列，但历史净值接口返回空响应（Data 缺省 /
    // 非对象，如缺 Referer 被拦截 / 风控）——不得按「已是最新」静默计成功。
    // 与 fund_incremental_up_to_date_counts_synced_without_write（同样是空窗口，
    // 但报文形态正常、空表可信）在结局上区分开。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let watermark = date_offset(7);
    seed_fund_price(&conn, "inst-fund", 30000, &watermark, true);

    let mut nav = |_: &NavQuery| -> Result<LsjzPage> {
        Ok(LsjzPage {
            points: vec![],
            total: 0,
            blocked: true,
        })
    };
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    // 空响应不可信：结局 = 窗口不完整待重试（原断言：不计 synced、计跳过）。
    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: true
        },
        "空响应不得按成功/无数据计，结局按待重试"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((30000, Some(watermark.clone()))),
        "空响应不动现价、水位不前进"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund").len(),
        1,
        "空响应零落库（原有历史点原样保留）"
    );
}

#[test]
fn fund_backfill_write_failure_leaves_no_history() {
    // ADR-0122 决策 8 负向条目（issue #1373）：单只回填整只一次提交——第 N 个周点
    // 写入失败时整只回滚，磁盘上不留半根历史，下次运行仍是首刷重新采集。
    // 失败注入见 [`inject_week_write_failure`]；逐周点提交会留下前两个周点使本例变红。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");
    inject_week_write_failure(&conn);

    // 单请求全量通道返回跨四周的单位净值序列（升序）：第 3 周写入触发注入失败。
    let series = [
        ("2026-01-05".to_string(), 1.000),
        ("2026-01-12".to_string(), 1.100),
        ("2026-01-19".to_string(), 1.200),
        ("2026-01-30".to_string(), 1.300),
    ];
    let full = [("110022", full_series(&series))];
    let full_requested = RefCell::new(Vec::new());
    let mut full_nav = mock_full_nav(&full, &full_requested);
    let mut nav = empty_nav;
    let err = run_fund_backfill(&conn, &fund, &mut nav, &mut full_nav).unwrap_err();

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

    // 「且下次运行会重新采集它」（AC 第二条）：解除注入后重跑，仍按首刷全量通道
    // 把整只历史补回——零行 → 首刷判据为真，正是原子性要保住的等价。
    conn.execute_batch("DROP TRIGGER inject_week_write_failure;")
        .unwrap();
    let mut full_nav_retry = mock_full_nav(&full, &full_requested);
    let mut nav_retry = empty_nav;
    let retry = run_fund_backfill(&conn, &fund, &mut nav_retry, &mut full_nav_retry).unwrap();
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
fn fund_partial_blocked_page_skips_whole_instrument() {
    // ADR-0122 决策 8（issue #1373）：部分页被拦截（空响应）时本轮窗口不完整，
    // 整只不落库并在日志标注——不再沿用「记警告后继续落已采点」的旧行为
    //（半根历史会让「有历史序列」冒充「历史完整」）。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    // 首页有净值点、次页被拦截（空响应、非零 TotalCount）：total=45 → 3 页。
    let pages = [(
        "110022",
        vec![
            nav_page(45, &[("2026-01-30", 3.348), ("2026-01-28", 3.293)]),
            LsjzPage {
                points: vec![],
                total: 45,
                blocked: true,
            },
        ],
    )];
    let requested = RefCell::new(Vec::new());
    let mut nav = mock_nav(&pages, &requested);
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    // 部分页被拦截不得计成功：结局 = 窗口不完整待重试（原断言：计跳过）。
    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: true
        },
        "部分页被拦截：本轮窗口不完整，结局按待重试"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![],
        "整只不落库：被拦截的部分页宁可整只留空重试，不留半根历史"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        None,
        "整只不落库时不写现价"
    );
}

#[test]
fn fund_incremental_partial_blocked_page_keeps_existing_history_and_watermark() {
    // ADR-0122 决策 8（issue #1373）的日间常态分支：已有历史序列的基金在增量窗口
    // 内被部分拦截时同样整只不落库——本轮已采净值点丢弃、既有历史点原样保留、
    // 水位不前进（下次从同一水位重取），不留下半根新历史。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    let watermark = "2026-01-20".to_string();
    seed_fund_price(&conn, "inst-fund", 30000, &watermark, true);

    // 增量窗口（水位次日 2026-01-21 起）：首页有净值点、次页被拦截（total=45 → 3 页）。
    let pages = [(
        "110022",
        vec![
            nav_page(45, &[("2026-01-30", 3.348)]),
            LsjzPage {
                points: vec![],
                total: 45,
                blocked: true,
            },
        ],
    )];
    let requested = RefCell::new(Vec::new());
    let mut nav = mock_nav(&pages, &requested);
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    // 部分页被拦截不得计成功：结局 = 窗口不完整待重试（原断言：计跳过）。
    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: true
        },
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![(watermark.clone(), 30000, "CNY".to_string())],
        "本轮已采点不落库，既有历史点原样保留"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        Some((30000, Some(watermark.clone()))),
        "整只不落库则水位不前进（下次从同一水位重取）"
    );
    assert_eq!(requested.borrow()[0].start_date, "2026-01-21");
}

#[test]
fn fund_page_cap_truncation_skips_whole_instrument() {
    // ADR-0122 决策 8（issue #1373）：页数触顶（服务端 TotalCount 异常，窗口已知
    // 未采全）与部分页被拦截同待遇——整只不落库；不再沿用「记警告后
    // 继续落已采点」。触顶若照旧落库，缺失段会因「有历史序列」为真而永久化。
    let conn = tauri_app_lib::test_support::open();
    insert_plain_instrument(&conn, "inst-fund", "110022", "fund", "CNY", "unknown");
    let fund = fund_instrument("inst-fund", "110022");

    // total=1000 → raw_pages=50 > MAX_NAV_PAGES(40)：触顶，首页已采净值点也不落库。
    let pages = [("110022", vec![nav_page(1000, &[("2026-01-30", 3.348)])])];
    let requested = RefCell::new(Vec::new());
    let mut nav = mock_nav(&pages, &requested);
    let full_requested = RefCell::new(Vec::new());
    let mut full = mock_full_nav(&[], &full_requested);
    let outcome = run_fund_backfill(&conn, &fund, &mut nav, &mut full).unwrap();

    // 页数触顶不得计成功：结局 = 窗口已知未采全，按待重试（原断言：计跳过）。
    assert_eq!(
        outcome,
        BackfillOutcome {
            written: false,
            inconclusive: true
        },
        "页数触顶：窗口已知未采全，结局按待重试"
    );
    assert_eq!(
        price_history_rows(&conn, "inst-fund"),
        vec![],
        "整只不落库：触顶的窗口宁可整只留空重试，不留半根历史"
    );
    assert_eq!(
        fund_price_of(&conn, "inst-fund"),
        None,
        "整只不落库时不写现价"
    );
}
