//! 价格历史后台补全测试（ADR-0122 / issue #1375）：派生事实队列（有价格通道但
//! 历史不完整，持仓优先）、一轮排空（进度序列、基金首刷页级明细、单只失败继续
//! 排空、零写入不置见证）、队列随补全自然排空。编排经注入 mock 闭包驱动，不
//! 依赖真实网络；断言对准用户可观察结果（抓取了谁、落了什么、进度怎么推进），
//! 不内省私有队列结构。

use std::cell::RefCell;

use chrono::Duration as ChronoDuration;
use rusqlite::{Connection, params};

use crate::SyncProgress;
use crate::fund_nav::{LsjzPage, NavPoint, NavQuery};
use crate::history::{HistoryBackfillStats, run_history_backfill_round};
use crate::http::KlineBar;
use crate::incremental::{beijing_today, week_monday};
use crate::model::WriteWitness;
use crate::tests::insert_holding;
use ledger_infra::error::{AppError, Result};
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, MarketPriceWrite, price_value_to_cents, upsert_market_price,
    upsert_price_history,
};
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
        crate::http::wait_foreground_idle();
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
