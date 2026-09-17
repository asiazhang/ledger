//! 价格历史后台补全（ADR-0122 / issue #1375）：把历史采集从用户动作里搬到
//! 后台任务的域内编排。
//!
//! 用户可观察结果：打开应用之后，走势曲线的历史自己长出来——启动后延迟
//! 跑一轮、此后每个自然日窗口各跑一轮，直到队列排空；持仓标的先补；与前台
//! 共享同一全局限速器且前台在途时让行；唯一可见面是静默的标的级完成计数
//!（新事件名 [`HISTORY_BACKFILL_PROGRESS`]，形状沿用既有确定进度事件）。
//!
//! 三块事实收口在本模块：
//! - **队列 = 派生事实**（[`collect_backfill_queue`]）：「有价格通道但历史不
//!   完整」——首刷（磁盘上没有任何历史序列）或缺周点（最新历史点 / 净值水位
//!   落后当前自然周超过一周）。不落任务表、不新增持久状态；进程退出即硬停
//!   （单只原子由 ADR-0122 决策 8 / issue #1373 保证，中断不留半根历史），
//!   下次启动继续。排队顺序**持仓优先**（投资域 [`INVESTED_EXISTS`] 谓词），
//!   同级按 symbol 升序。
//! - **一轮补全**（[`run_history_backfill_round`]）：排空队列——逐只调用
//!   单只历史回填单元（行情标的 = [`backfill_stock_history`]，基金 =
//!   [`backfill_one_fund_history`]，issue #1377 起本两单元为后台补全专用——
//!   「同步标的信息」只刷现价，不再与手动同步共用历史采集）；单只
//!   网络失败**不中断**本轮（记日志继续排空，无用户可打断也无用户可报），
//!   失败者靠派生事实在下一窗口自然重进队列。基金首刷的页级明细随进度事件
//!   带出（issue #1061 的明细随首刷深回填迁入本任务），并登记走势空态三态
//!   的判据输入（轮次计数与尝试结局，投资域 [`ledger_investment::backfill`]）。
//! - **调度**（[`start_history_backfill`]）：启动后延迟一轮 + 每个自然日窗口
//!   各一轮的巡检任务（挂全局运行时的 async 任务，ADR-0125 决策 7 / #1413）；
//!   每轮门检锁定/启动失败（先例：自动备份调度）；启动
//!   接线在壳层后台服务编排单点（issue #961 名单）。
//!
//! 与手动同步的解耦关系（issue #1377 收尾）：手动同步只刷现价（含当周采样点
//! 直落），不再采集历史；写入同是「现价覆盖 + 同周整周覆盖」的幂等 upsert，
//! 数据库互斥由短段取锁天然串行，两路并发不产生半根历史或重复行。
//!
//! 收尾裁决与手动同步同形（issue #1277 成败同判）：本轮实际写过价格数据
//! （写入见证 [`WriteWitness`]）→ 提交点置脏一次 + 发既有价格失效信号；
//! 零写入不置脏不广播。汇率 K 线同期补齐（与价格历史同期段采集，ADR-0019），
//! 与手动同步同口径不计入写入见证。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::NaiveDate;
use rusqlite::{Connection, params};
use tauri::{AppHandle, Manager, Runtime};

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{DbState, DbWriteHandle};
use ledger_infra::error::{AppError, Result};
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};
use ledger_investment::predicates::INVESTED_EXISTS;
use ledger_investment::{InstrumentType, PriceChannel, derive_price_channel};

use super::channels::{FetchFuture, SyncFetchChannels};
use super::fund_backfill::{BackfillOutcome, backfill_one_fund_history};
use super::fund_nav::{LsjzPage, NavPoint, NavQuery};
use super::http::{KlineBar, secid_prefix};
use super::incremental::{
    SyncInstrument, backfill_fx_pairs, beijing_today, downsample_weekly, quote_code, week_monday,
    write_weekly_price_history,
};
use super::model::WriteWitness;
use super::progress::{BackfillProgressEmitter, FundNavProgress, SyncProgress};
use super::session::{FacadeWriteSession, ScopedSession};
use ledger_investment::backfill;
use ledger_investment::prices::{
    EASTMONEY_PRICE_SOURCE, price_value_to_cents, upsert_price_history,
};

/// 应用启动后的首轮延迟：让出启动期（引导、参考数据装载、首屏渲染），再开始
/// 第一轮补全。延迟是可逆工程决策，测试注入 [`BackfillTimings`] 覆写。后台
/// 每日现价刷新（[`super::daily_refresh`]）与同一节奏（同形调度，issue #1377）。
pub(super) const STARTUP_DELAY: Duration = Duration::from_secs(30);

/// 自然日窗口的巡检周期：任务低频醒来比对北京日历日，跨日即跑当天的窗口。
/// 与自动备份调度、多端同步轮询同一「低频」品味（分钟级间隔，代价为零——
/// 每次巡检只做一次日期比对）。
pub(super) const WINDOW_POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// 缺周点判据（ADR-0122 决策 2「逐只采集只服务首刷与缺周点」的队列半边）：
/// 参考点（行情标的历史的最新周点 / 基金水位的净值日期）落后当前自然周**超过
/// 一周**即历史不完整——常态的跨周（≤7 天）不算，与基金分区缺周点补齐判据
///（`week_gap_needs_per_instrument`）同一品味。参考点缺失或不可解析时保守
/// 判缺（宁可多采一轮，不静默丢周点）。
fn week_behind(reference: Option<&str>, today: NaiveDate) -> bool {
    let Some(reference) =
        reference.and_then(|date| NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d").ok())
    else {
        return true;
    };
    week_monday(today) - week_monday(reference) > chrono::Duration::days(7)
}

/// 一只标的的补全目标（通道分区的产物）：行情标的按 secid 拉日 K，基金走
/// 历史净值通道（首刷近两年 / 水位增量；issue #1377 起本通道为后台补全专用）。
enum BackfillTarget {
    Quote { secid: String },
    FundNav,
}

/// 补全队列的一项：标的 + 它的补全目标。
struct BackfillItem {
    instrument: SyncInstrument,
    target: BackfillTarget,
}

/// 补全队列收集（派生事实，一条 SQL）：库内全部标的按投资域单点派生价格通道，
/// 只留行情与净值两通道，再按「历史不完整」过滤——首刷（无任何历史序列）或
/// 缺周点（见 [`week_behind`]）。排序**持仓优先**（[`INVESTED_EXISTS`] 谓词），
/// 同级按 symbol 升序；跳过的行（手动报价 / 无来源通道、历史完整的标的）不进
/// 队列、零请求。
fn collect_backfill_queue(conn: &Connection) -> Result<Vec<BackfillItem>> {
    let today = beijing_today();
    let sql = format!(
        "SELECT i.id, i.symbol, i.market, i.currency_code, i.instrument_type, \
                MAX(ph.trade_date) AS latest_history, \
                MAX(mp.nav_date) AS watermark, \
                CASE WHEN {INVESTED_EXISTS} THEN 1 ELSE 0 END AS invested \
         FROM instruments i \
         LEFT JOIN price_history ph ON ph.instrument_id = i.id \
         LEFT JOIN market_prices mp ON mp.instrument_id = i.id \
         GROUP BY i.id \
         ORDER BY invested DESC, i.symbol ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, InstrumentType>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, i64>(7)? != 0,
        ))
    })?;
    let mut queue = Vec::new();
    for row in rows {
        let (instrument_id, symbol, market, currency, kind, latest_history, watermark, _invested) =
            row?;
        // 价格通道判定消费投资域单点（issue #1060），与标的读投影同源；
        // _invested 已在 SQL 层的 ORDER BY 完成消费（持仓优先），不再取用。
        let channel = derive_price_channel(kind, &market, &symbol);
        let target = match channel {
            PriceChannel::Quote => {
                let Some(prefix) = secid_prefix(&market) else {
                    // 行情通道市场必已知（派生单点保证）；不可构造 secid 的形态
                    // 保守不进队列（零请求），防御派生规则未来漂移。
                    continue;
                };
                let incomplete = match &latest_history {
                    None => true,
                    Some(latest) => week_behind(Some(latest), today),
                };
                if !incomplete {
                    continue;
                }
                BackfillTarget::Quote {
                    secid: format!("{prefix}.{}", quote_code(&symbol)),
                }
            }
            PriceChannel::FundNav => {
                // 首刷判据 = 磁盘上没有任何历史序列（issue #1059，与基金分区
                // 同源）；已有历史者按净值水位判缺周点（水位缺失保守判缺）。
                let incomplete = match &latest_history {
                    None => true,
                    Some(_) => week_behind(watermark.as_deref(), today),
                };
                if !incomplete {
                    continue;
                }
                BackfillTarget::FundNav
            }
            // 手动报价与无来源通道没有可采集的历史序列，不进队列。
            PriceChannel::Manual | PriceChannel::None => continue,
        };
        queue.push(BackfillItem {
            instrument: SyncInstrument {
                instrument_id,
                symbol,
                market,
                currency,
                channel,
            },
            target,
        });
    }
    Ok(queue)
}

/// 单只行情标的的历史回填单元（issue #1375 自增量同步编排抽出；issue #1377
/// 起归后台补全专用——现价刷新不再发逐只日 K 请求）：
/// 一次日 K 请求 + 周采样降采样落 `price_history`，单只整只一次提交
///（ADR-0122 决策 8 / issue #1373，事务经 [`ensure_transaction`] 嵌套感知）。
/// 返回是否实际落库新周点；**全部无新点（已入库且同值）零写入**——返回
/// false，调用方不置脏不广播（收尾裁决口径「全部无新点不置脏不广播」，
/// ADR-0122；停牌/退市股持续在队的每日重采因此不空发信号）。有新点时的
/// 数据结果与无条件重写逐位一致（整周覆盖幂等）。
///
/// 抓取在会话之外（await）、落库短暂取一次连接（issue #1275 / #1412 async 形态，
/// 与抽取前同形）。
pub(super) async fn backfill_stock_history<Q, K>(
    session: &Q,
    fetch_kline: &mut K,
    secid: &str,
    inst: &SyncInstrument,
) -> Result<bool>
where
    Q: ScopedSession,
    K: FnMut(&str) -> FetchFuture<Vec<KlineBar>> + Send,
{
    let bars = fetch_kline(secid).await?;
    let points = downsample_weekly(&bars);
    if points.is_empty() {
        return Ok(false);
    }
    let instrument_id = inst.instrument_id.clone();
    let has_new = session
        .with_connection(move |conn| has_new_weekly_point(conn, &instrument_id, &points))
        .await?;
    if !has_new {
        return Ok(false);
    }
    let (instrument_id, currency) = (inst.instrument_id.clone(), inst.currency.clone());
    session
        .with_connection(move |conn| {
            ensure_transaction(conn, || {
                write_weekly_price_history(conn, &instrument_id, &currency, &bars)
            })
        })
        .await
        .map(|written| written > 0)
}

/// 新点判定：降采样周点中是否存在「库内无此周」或「同周不同值」的行。
/// 值比较按价格刻度换算后的存量列（`price_cents`）直比，浮点展示值不参与。
fn has_new_weekly_point(
    conn: &Connection,
    instrument_id: &str,
    points: &[(String, f64)],
) -> Result<bool> {
    let mut stmt =
        conn.prepare("SELECT trade_date, price_cents FROM price_history WHERE instrument_id = ?1")?;
    let existing: std::collections::HashMap<String, i64> = stmt
        .query_map(params![instrument_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    for (trade_date, close) in points {
        let cents = price_value_to_cents(*close);
        if existing.get(trade_date) != Some(&cents) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 现价刷新直落当周采样点（ADR-0122 决策 2 / issue #1377）：只服务**已有历史
/// 序列**的标的（无序列者落单点会让「有历史序列」冒充「历史完整」，永久破坏
/// 首刷判据——历史由后台补全整根回填）；且仅当该周点确实新（库内无此周、或
/// 同周不同值）才写——「每周至多一条、取该周最后一个有报价交易日、整周覆盖
/// 幂等」语义不变，同周同值零写入。返回是否实际落库（调用方据此决定是否计入
/// 写入见证）。
///
/// `trade_date` 取同步当日（批量报价响应不携带行情日期）：周末 / 节假日同步时
/// 点的价格是最近交易日的收盘价，而日期标签落在同步日——价格正确、周归属正确
///（周键按 ISO 周算），仅横轴标签可能落在非交易日；下一个有报价交易日的同步
/// 以同周覆盖改写回真实交易日（整周覆盖幂等），仓库无交易日历事实源（见价格
/// 过期提示词条），不为标签引入第二口径。
pub(super) fn land_current_week_point(
    conn: &Connection,
    instrument_id: &str,
    currency: &str,
    trade_date: &str,
    price_cents: i64,
) -> Result<bool> {
    if !ledger_investment::backfill::has_any_history(conn, instrument_id)? {
        return Ok(false);
    }
    let date = NaiveDate::parse_from_str(trade_date, "%Y-%m-%d")
        .map_err(|e| AppError::Parse(format!("非法交易日 {trade_date}: {e}")))?;
    let monday = week_monday(date);
    let sunday = monday + chrono::Duration::days(6);
    let existing: Option<i64> = conn
        .query_row(
            "SELECT price_cents FROM price_history \
             WHERE instrument_id = ?1 AND trade_date >= ?2 AND trade_date <= ?3",
            params![
                instrument_id,
                monday.format("%Y-%m-%d").to_string(),
                sunday.format("%Y-%m-%d").to_string(),
            ],
            |row| row.get(0),
        )
        .ok();
    if existing == Some(price_cents) {
        return Ok(false);
    }
    upsert_price_history(
        conn,
        instrument_id,
        trade_date,
        price_cents,
        currency,
        EASTMONEY_PRICE_SOURCE,
    )?;
    Ok(true)
}

/// 一轮补全的统计：`queued` = 进队标的数，`failed` = 单只失败数（已记日志，
/// 靠派生事实在下一窗口重进队列）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct HistoryBackfillStats {
    pub(super) queued: usize,
    pub(super) failed: usize,
}

/// 一轮价格历史补全：收集派生队列 → 排空（逐只回填单元，单只失败继续）→
/// 汇率 K 线同期补齐。进度回调是唯一对外观察点：分母 = 队列长度，收集完成
/// 立即发 `{ done: 0, total }`，此后每只处理完推进一格（成败同计格，与手动
/// 同步口径一致——有通道标的不以成败计格）；基金首刷翻页期间另带页级明细
///（issue #1061 形状）。队列空零动作：不发进度、零网络请求。
///
/// 单只网络失败不中断本轮（记 warn 继续）：无用户在场，失败的标的靠派生事实
/// 在下一窗口自然重进队列；单只原子（issue #1373）保证失败不留半根历史。
/// 汇率失败同样不中断（辅助性折算序列，缺失段由后续窗口的后台补全补齐）。
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_history_backfill_round<Q, K, X, N, S, P>(
    session: &Q,
    fetch_kline: &mut K,
    fetch_fx: &mut X,
    fetch_nav: &mut N,
    fetch_nav_full: &mut S,
    progress: &mut P,
    witness: &mut WriteWitness,
) -> Result<HistoryBackfillStats>
where
    Q: ScopedSession,
    K: FnMut(&str) -> FetchFuture<Vec<KlineBar>> + Send,
    X: FnMut(&str) -> FetchFuture<Vec<KlineBar>> + Send,
    N: FnMut(&NavQuery) -> FetchFuture<LsjzPage> + Send,
    S: FnMut(&str) -> FetchFuture<Vec<NavPoint>> + Send,
    P: FnMut(SyncProgress) + Send,
{
    let queue = session.with_connection(collect_backfill_queue).await?;
    let total = queue.len();
    if total == 0 {
        return Ok(HistoryBackfillStats {
            queued: 0,
            failed: 0,
        });
    }
    progress(SyncProgress::instrument(0, total));
    // 运行态发布（issue #1377 走势空态三态的判据输入）：登记在途计数分母并清空
    // 上一轮的尝试结局表——队列按派生事实重新收集，旧结局不再可信。
    backfill::round_started(total);
    let mut done = 0usize;
    let mut failed = 0usize;

    for item in &queue {
        let result: Result<(bool, bool)> = match &item.target {
            BackfillTarget::Quote { secid } => {
                backfill_stock_history(session, fetch_kline, secid, &item.instrument)
                    .await
                    .map(|written| (written, false))
            }
            BackfillTarget::FundNav => {
                // 页级推进（issue #1061）：done/total 仍是标的级口径，页抓取
                // 返回后才带出本基金的页明细；块作用域限定回调借用。
                let code = item.instrument.symbol.clone();
                let mut on_page = |page: u64, pages: u64| {
                    progress(SyncProgress {
                        done,
                        total,
                        fund: Some(FundNavProgress {
                            code: code.clone(),
                            page,
                            pages,
                        }),
                    });
                };
                // 队列只收「历史不完整」的基金：首刷与缺周点补齐正是逐只通道
                // 的两种接管形态（issue #1377 起本单元为后台补全专用，现价刷新
                // 走 refresh_one_fund_price）。
                backfill_one_fund_history(
                    session,
                    &item.instrument,
                    fetch_nav,
                    fetch_nav_full,
                    &mut on_page,
                )
                .await
                .map(|outcome: BackfillOutcome| (outcome.written, outcome.inconclusive))
            }
        };
        match &result {
            Ok((written, _)) => {
                if *written {
                    witness.mark_written();
                }
            }
            Err(error) => {
                failed += 1;
                tracing::warn!(
                    instrument = %item.instrument.symbol,
                    %error,
                    "价格历史补全单只失败，继续排空队列（下一窗口按派生事实重进）"
                );
            }
        }
        // 尝试结局登记（issue #1377 走势空态三态的判据输入）：仅对**仍无历史
        // 序列**的标的记录——成功落库者不再入空态。结局二分：确定完成且无可采
        //（Ok 且非不可信）= 无数据；失败或窗口不完整（被拦截 ≠ 数据源没有）=
        // 待重试。存在性检查失败不致命（记警告，三态暂按「补全中」处理）。
        let attempt_failed = match &result {
            Ok((_, false)) => backfill::BackfillAttempt::NoData,
            Ok((_, true)) | Err(_) => backfill::BackfillAttempt::Failed,
        };
        let instrument_id = item.instrument.instrument_id.clone();
        match session
            .with_connection(move |conn| {
                ledger_investment::backfill::has_any_history(conn, &instrument_id)
            })
            .await
        {
            Ok(false) => backfill::record_attempt(&item.instrument.instrument_id, attempt_failed),
            Ok(true) => {}
            Err(error) => tracing::warn!(
                instrument = %item.instrument.symbol, %error,
                "补全后历史存在性检查失败，三态判据暂按补全中处理"
            ),
        }
        done += 1;
        backfill::round_progress(done);
        progress(SyncProgress::instrument(done, total));
    }
    // 在途计数收起（尝试结局表保留到下一轮开始，轮间窗口的空态判定消费它）。
    backfill::round_finished();

    // 汇率 K 线同期补齐（与手动同步 §③ 同一单元）：只取本轮队列标的的币种对
    // ——本轮实际在补的曲线才需要同期折算序列；已完整标的的汇率序列已在库。
    let currencies = queue.iter().map(|item| item.instrument.currency.clone());
    if let Err(error) = backfill_fx_pairs(session, fetch_fx, currencies).await {
        tracing::warn!(%error, "历史补全汇率序列补齐失败（不中断本轮，缺失段后续补齐）");
    }

    Ok(HistoryBackfillStats {
        queued: total,
        failed,
    })
}

/// 后台补全的会话（后台车道侧）：门面写槽裸作业会话（[`FacadeWriteSession`]，
/// issue #1412 async 形态）——每轮补全经作用域会话短暂投递门面作业，分钟级
/// 网络等待发生在段与段之外且不占用 DB 线程（与命令壳同一接缝、同一纪律；
/// 先例：多端同步调度侧 `AutoRoundConn`）。后台车道的连接取用不再直锁连接槽
///（ADR-0125 决策 5）；每日现价刷新（[`super::daily_refresh`]）共用同一形态。
/// 车道本体已是挂全局运行时的 async 任务（转 async 任务归 #1413），编排直接
/// 在任务上 `await`。
///
/// 后台补全进度发射的壳层接线（与命令壳 `progress_to_emitter` 同型）：把编排
/// 的进度回调接到后台补全事件发射器（非阻塞投递，发射失败静默）。
pub(super) fn backfill_progress_to_emitter(
    emitter: &dyn BackfillProgressEmitter,
) -> impl FnMut(SyncProgress) + '_ {
    move |progress| emitter.emit_backfill_progress(progress)
}

/// 后台补全通道注入接缝（issue #1375，先例：命令壳 `SyncChannelsSlot`）：生产
/// **不管理**本状态（每轮建后台车道生产束），集成测试 manage 本状态并装入
/// 门控桩束，使「后台补全真实在途」可确定复现。异步互斥体：通道束引用在
/// `.await` 之间存活（issue #1412 通道束闭包 async 化）。
pub struct BackfillChannelsSlot(pub Arc<tokio::sync::Mutex<SyncFetchChannels>>);

/// 调度时机的显式参数（启动延迟与自然日窗口巡检周期）：生产走默认值，接线型
/// 集成测试注入短时机——断言只等窗口到达，不被生产常数拖慢（先例：
/// 多端同步 `TriggerTimings`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BackfillTimings {
    /// 应用启动后到首轮补全的延迟。
    pub startup_delay: Duration,
    /// 自然日窗口的巡检周期（跨北京日历日即跑当天窗口）。
    pub window_poll: Duration,
}

impl Default for BackfillTimings {
    fn default() -> Self {
        Self {
            startup_delay: STARTUP_DELAY,
            window_poll: WINDOW_POLL_INTERVAL,
        }
    }
}

/// 价格历史后台补全的启动入口（壳层后台服务编排单点调用，issue #961 名单）：
/// 进程级单次拉起（原位重引导重复调用幂等，ADR-0080），async 任务自持
/// 「启动延迟一轮 + 每自然日窗口一轮」的巡检循环（挂全局运行时，ADR-0125
/// 决策 7 / issue #1413：启动延迟与自然日窗口用异步定时，不再自建 OS 线程；
/// 执行器 = `tauri::async_runtime`）。
pub fn start_history_backfill<R: Runtime>(app: &AppHandle<R>) {
    start_history_backfill_with(app, BackfillTimings::default());
}

/// 同 [`start_history_backfill`]，调度时机可注入（测试短时机先例
/// `start_sync_scheduler_with`）。
pub fn start_history_backfill_with<R: Runtime>(app: &AppHandle<R>, timings: BackfillTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let gate = EncryptionGate::clone(&app.state::<EncryptionGate>());
    let boot_gate = BootFailureGate::clone(&app.state::<BootFailureGate>());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // 启动延迟（issue #1375）：让出启动期再开始第一轮；应用退出即任务随
        // 进程硬停，无需优雅关闭（单只原子保证中断不留半根历史）。
        tokio::time::sleep(timings.startup_delay).await;
        let mut last_round_date: Option<NaiveDate> = None;
        loop {
            // 每轮门检（先例：自动备份调度，issue #644 / ADR-0080）：锁定/
            // 启动失败期间不触碰占位连接；门开后首个到达的自然日窗口即补跑
            // ——启动轮即当天的窗口（「启动后延迟一次 + 此后每日各一次」）。
            if !gate.is_locked() && !boot_gate.is_failed() {
                let today = beijing_today();
                if last_round_date != Some(today) {
                    last_round_date = Some(today);
                    run_backfill_round_gated(&handle).await;
                }
            }
            tokio::time::sleep(timings.window_poll).await;
        }
    });
}

/// 跑一轮补全（通道束换装 + 会话 + 见证 + 收尾裁决的单轮编排）：测试桩束优先
///（[`BackfillChannelsSlot`] 管理态），生产每轮建后台车道束（请求前让行前台、
/// 共享全局限速器）。失败静默等下一窗口（无用户可报，先例：自动轮次）。async
/// 形态（ADR-0125 决策 7 / issue #1413）：编排直接在车道 async 任务上 `await`，
/// 不再经全局运行时跨线程驱动。
async fn run_backfill_round_gated<R: Runtime>(app: &AppHandle<R>) {
    let write = app.state::<DbState>().write_handle();
    let slot = app.try_state::<BackfillChannelsSlot>().map(|s| s.0.clone());
    let (result, any_written) = match slot {
        Some(arc) => run_round_with_channels(app, &write, &arc).await,
        None => match SyncFetchChannels::production_backfill() {
            Ok(channels) => {
                let channels = tokio::sync::Mutex::new(channels);
                run_round_with_channels(app, &write, &channels).await
            }
            Err(error) => (Err(error), false),
        },
    };

    // 整体裁决（issue #1277 成败同判同形）：本轮实际写过价格数据 → 提交点
    // 置脏一次 + 发既有价格失效信号，消费方由信号驱动重拉；零写入（队列空/
    // 全部无新点/失败未写过）不置脏不广播。置脏失败记日志不静默吞运行结果。
    // 置脏经门面异步作业投递（车道已是 async 任务，不再用阻塞等待形态）。
    if any_written {
        if let Err(error) = write.run("history_backfill", |_| Ok(())).await {
            tracing::warn!(%error, "历史补全收尾置脏失败（脏标记待下次写入补上）");
        }
        emit_for(
            app,
            WriteOp::SyncInstrumentInfo,
            WriteEvidence::PriceWritten(true),
        );
    }
    match result {
        Ok(stats) if stats.queued == 0 => tracing::debug!("历史补全队列空，本轮零动作"),
        Ok(stats) => tracing::info!(
            queued = stats.queued,
            failed = stats.failed,
            "价格历史后台补全一轮完成"
        ),
        Err(error) => tracing::warn!(%error, "价格历史后台补全一轮失败（静默等下一窗口）"),
    }
}

/// 持束跑一轮：会话（门面写槽裸作业会话）+ 进度发射（后台补全事件）+ 写入见证。
/// 返回 (编排结果, 是否实际写过)——见证跨单只失败存活（成败同判的证据源，
/// 与手动同步同形）。
async fn run_round_with_channels<R: Runtime>(
    app: &AppHandle<R>,
    write: &DbWriteHandle,
    channels: &tokio::sync::Mutex<SyncFetchChannels>,
) -> (Result<HistoryBackfillStats>, bool) {
    let mut witness = WriteWitness::default();
    let session = FacadeWriteSession::new(write.clone(), "history_backfill");
    let mut progress = backfill_progress_to_emitter(app);
    let mut channels = channels.lock().await;
    // 借用拆字段：互斥体守卫经 DerefMut 的整体借用不可拆（借用检查按整守卫
    // 记账），先解引用再按字段拆借，四条通道互不重叠地交给编排。
    let SyncFetchChannels {
        fetch_kline,
        fetch_fx,
        fetch_nav,
        fetch_nav_full,
        ..
    } = &mut *channels;
    let result = run_history_backfill_round(
        &session,
        fetch_kline,
        fetch_fx,
        fetch_nav,
        fetch_nav_full,
        &mut progress,
        &mut witness,
    )
    .await;
    let any_written = witness.any_written();
    (result, any_written)
}
