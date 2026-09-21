//! ECB 汇率同步编排（issue #1544 / ADR-0019 修订记录）：给汇率序列定「要拉多深」
//! 的窗口判据单点，与全量回填 / 90 天增量两条取数腿的域内编排。
//!
//! 用户可观察结果：账本里最早的非本位币痕迹（账户或交易）出现后，一次同步即把
//! 各币种对的历史序列补到**该日期之前**（写路径按交易日折算的取数因此可用），
//! 与标的 K 线的「近两年」窗口和首刷 / 缺周队列彻底无关（两条链路各自独立触发，
//! 本模块不读 instruments、不碰 price_history、不进 [`super::history`] 的队列）。
//!
//! 三块事实收口在本模块：
//! - **窗口判据**（[`plan_fx_sync`]）：深度 = 账本中最早的非本位币相关日期
//!   （[`earliest_non_native_trace_date`]：非本位币账户的 `created_at` 与非本位币
//!   交易的 `date` 取 MIN；软删行与隐藏账户排除——黑洞账户是 V004 种子的系统
//!   缓冲池，其创建日是安装时刻而非用户使用非本位币的起点，其内的交易仍是
//!   真实痕迹）。窗口起点 = 该日期所属 ISO 周的周一再前推一周
//!   （[`WINDOW_LEAD_WEEKS`]）——序列自此**早于**该日期，
//!   交易自身所属周必有点。账本没有任何非本位币痕迹时判据为「零痕迹 → 零请求
//!   零落库」（不做过深的无谓回填，AC 有断言）；痕迹齐全而已落库序列未达窗口
//!   起点时走全量；已达深度（[`fx_depth_reached`]，**逐币种对**判定，一处旧覆盖
//!   不掩盖另一对的缺口）的重复同步只走 90 天增量，不重复拉全量（幂等，AC 有
//!   断言）。已接受代价：某腿在数据源的起点晚于窗口起点时（现字典对 CNY 腿
//!   2005-04 起），该对永远判「未达深」而重复拉全量文件——真实账本痕迹远新于
//!   各腿起点，不为其复杂化判据。
//! - **一轮同步**（[`sync_fx_rates`]）：按判据取数（全量文件或 90 天增量文件，
//!   币种对 = 币种字典全量对本位币，见投资域 FxRateHistory 词条「全字典可折算」）、
//!   交叉推导周采样、全量腿按窗口起点裁剪（深度由判据决定，不把 1999 年起的整根
//!   文件灌进库）后交落库单元 [`persist_ecb_fx_series`]（#1543：单一事务、整周
//!   覆盖幂等、不产同步 op、人工行保护）。取数在会话外、落库短暂取连接
//!   （[`super::session`] 纪律，与 [`super::incremental`] 同形）。
//! - **通道束**（[`FxSyncChannels`]）：两条取数腿的闭包打包与生产换装接缝——
//!   生产接 ECB 官方站两个文件（[`super::ecb`] 取数单元单点），测试注入桩闭包
//!   （判据行为测试）或本地 HTTP 服务（接线证明，[`FxSyncChannels::with_hosts`]）。
//!   触发面（设置页手动入口 / 每日自动调度）已接线：前者住壳层 `sync_exchange_rates`
//!   命令（#1545），后者住 [`super::fx_daily`] 后台车道（#1546），本单元即其共同
//!   消费的唯一编排。
//!
//! 汇率拉取按**可重建缓存**对待（ADR-0019 修订记录）：不进同步日志、不计入任何
//! 写入见证；同步失败上抛且原因三态互不吞并（spec #1540「数据源不可达 vs 该来源
//! 无数据要能分辨」，issue #1545 AC）：取数网络失败 → `fx.source-unreachable`，
//! 取数成功但推导零点 → `fx.source-no-data`，`fx.source-malformed` 原样透传不
//! 折算，已有汇率不受影响（落库是覆盖幂等 upsert）。

use chrono::NaiveDate;
use rusqlite::{Connection, params};
use serde::Serialize;

use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::default_currency_code;

use super::channels::FetchFuture;
use super::ecb::{
    ECB_HOSTS, EcbDayRates, derive_ecb_weekly_series, fetch_ecb_90d_incremental,
    fetch_ecb_full_history,
};
use super::http::{build_client, lock_pacer, shared_pacer};
use super::incremental::week_monday;
use super::persist::{FxPersistReport, persist_ecb_fx_series};
use super::session::ScopedSession;

/// 窗口起点相对最早非本位币日期再前推的周数：序列从该日期**之前**一周起有
/// 采样点（AC「全量回填覆盖到该日期之前」的余量——即使该日期所属周首日无报价，
/// 前一周的点也已先于它落库）。
const WINDOW_LEAD_WEEKS: i64 = 1;

/// 「数据源不可达」码化错误（spec #1540 用户故事 12 / issue #1545：失败提示可
/// 自助补救——先分清是网络问题还是账本问题）。params 无动态值（ADR-0050）。
fn source_unreachable() -> AppError {
    AppError::coded(
        "fx.source-unreachable",
        "汇率数据源暂时不可达，请检查网络后重试",
    )
}

/// 「该来源无数据」码化错误：源可达、报文合法，但没有推导出任何可用汇率点
///（如响应缺本位币腿）。params 无动态值（ADR-0050）。
fn source_no_data() -> AppError {
    AppError::coded(
        "fx.source-no-data",
        "数据源已连通，但当前没有可用的汇率数据，请稍后重试",
    )
}

/// 取数失败的归类（issue #1545 AC：不可达与无数据可分辨）：`fx.source-malformed`
/// 是独立条件（源返回了无法解析的内容），原样透传；其余（连接失败 / 超时 /
/// 限流放弃 / HTTP 状态异常，HTTP 层报 `AppError::Io`）收口为「数据源不可达」。
fn classify_fetch_error(error: AppError) -> AppError {
    if error.is_code("fx.source-malformed") {
        return error;
    }
    tracing::warn!(error = %error, "汇率数据源不可达（取数失败已归类）");
    source_unreachable()
}

/// 取数成功但推导零点的判定（issue #1545 AC）：全部币种对都无点 = 该来源无可用
/// 数据，显式报错——零点不白落一次空事务、也不冒充成功（零痕迹跳过是判据层的
/// 独立路径，不经此处）。
fn ensure_series_has_points(series: &[super::ecb::FxPairWeeklySeries]) -> Result<()> {
    if series.iter().all(|s| s.points.is_empty()) {
        return Err(source_no_data());
    }
    Ok(())
}

/// ECB 文档抓取通道闭包形态：无参数（文件路径由通道内部固定，全量 / 增量各一）
/// → 按日快照序列。编排不拼数据源键，换源只改通道实现。
pub type FetchEcbDocument = Box<dyn FnMut() -> FetchFuture<Vec<EcbDayRates>> + Send>;

/// ECB 汇率同步通道束（两条取数腿的打包形态，先例 [`super::channels::SyncFetchChannels`]）：
/// 生产经 [`FxSyncChannels::production`] 接 ECB 官方站（复用 HTTP 层主机池 / 重试 /
/// 共享限速 pacer），测试把桩闭包装进同形结构注入——编排与触发面对生产 / 测试零分叉。
pub struct FxSyncChannels {
    /// 全量历史文件腿（历史回填，eurofxref-hist.xml）。
    pub fetch_full: FetchEcbDocument,
    /// 90 天增量文件腿（每日自动增量，eurofxref-hist-90d.xml）。
    pub fetch_incremental: FetchEcbDocument,
}

impl FxSyncChannels {
    /// 生产通道束：ECB 官方站两个参考汇率文件（取数单元单点 [`super::ecb`]）。
    pub fn production() -> Result<Self> {
        let hosts = ECB_HOSTS.iter().map(|host| (*host).to_string()).collect();
        Self::with_hosts(hosts)
    }

    /// 生产束构造本体：`ecb_hosts` 携带 ECB 主机池。生产经 [`FxSyncChannels::production`]
    /// 传取数单元单点常量；测试注入本地 HTTP 服务，驱动**生产束**钉住接线：
    /// 「回填打到全量历史文件、深度达成后的增量打到 90 天文件」（删除接线即红）。
    pub(super) fn with_hosts(ecb_hosts: Vec<String>) -> Result<Self> {
        let client = build_client()?;
        let pacer = shared_pacer();
        Ok(Self {
            fetch_full: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = ecb_hosts.clone();
                Box::new(move || {
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        fetch_ecb_full_history(&client, &mut pacer, &hosts).await
                    }) as FetchFuture<Vec<EcbDayRates>>
                })
            },
            fetch_incremental: {
                let client = client.clone();
                let pacer = pacer.clone();
                let hosts = ecb_hosts.clone();
                Box::new(move || {
                    let client = client.clone();
                    let pacer = pacer.clone();
                    let hosts = hosts.clone();
                    Box::pin(async move {
                        let mut pacer = lock_pacer(&pacer).await;
                        let hosts: Vec<&str> = hosts.iter().map(String::as_str).collect();
                        fetch_ecb_90d_incremental(&client, &mut pacer, &hosts).await
                    }) as FetchFuture<Vec<EcbDayRates>>
                })
            },
        })
    }
}

/// 一轮汇率同步的结果（#1544）：触发面（#1545 结果面 / #1546 日志）消费。
/// `Serialize`：#1545 起 IPC 命令直接返回本类型（前端展示覆盖区间 / 条数）。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct FxSyncReport {
    /// 本次是否执行了全量历史回填（false = 深度已达成只走增量，或零痕迹跳过）。
    pub full_backfilled: bool,
    /// 落库统计（对数 / 周点数 / 覆盖区间 / 人工保护计数，#1543 的
    /// [`FxPersistReport`]）。
    pub persist: FxPersistReport,
}

/// 同步判据的计划（读库即可决定，不发请求）：一次连接访问内算清，取数在会话外。
#[derive(Debug, Clone, PartialEq, Eq)]
enum FxSyncPlan {
    /// 无任何非本位币痕迹：零请求零落库（不做过深的无谓回填）。
    Skip,
    /// 全量历史回填：携带窗口起点（最早非本位币日期前 [`WINDOW_LEAD_WEEKS`] 周的周一）。
    FullBackfill(NaiveDate),
    /// 深度已达成：只走 90 天增量。
    Incremental,
}

/// 一轮汇率同步（issue #1544，#1545 手动入口与 #1546 每日增量的共同编排）：
/// 读库定判据 → 会话外取数 → 交叉推导与窗口裁剪 → 落库（单一事务幂等）。
///
/// 判据与取数腿的对应：零痕迹 = [`FxSyncPlan::Skip`]（两个通道都不碰）；深度未达
/// = 全量腿 + 窗口裁剪；深度已达 = 只走增量腿（不重复拉全量）。返回 [`FxSyncReport`]。
pub async fn sync_fx_rates<S: ScopedSession>(
    session: &S,
    channels: &mut FxSyncChannels,
) -> Result<FxSyncReport> {
    let (plan, pairs) = session.with_connection(plan_fx_sync).await?;
    match plan {
        FxSyncPlan::Skip => {
            tracing::debug!("账本无非本位币痕迹，汇率同步跳过（零请求零落库）");
            Ok(FxSyncReport::default())
        }
        FxSyncPlan::FullBackfill(window_start) => {
            let days = (channels.fetch_full)()
                .await
                .map_err(classify_fetch_error)?;
            let series =
                trim_series_to_window(derive_ecb_weekly_series(&days, &pairs), window_start);
            ensure_series_has_points(&series)?;
            let persist = session
                .with_connection(move |conn| persist_ecb_fx_series(conn, &series))
                .await?;
            tracing::info!(
                earliest = persist.earliest.as_deref().unwrap_or("-"),
                points = persist.points,
                "ECB 汇率全量回填完成（窗口起点 {window_start}）"
            );
            Ok(FxSyncReport {
                full_backfilled: true,
                persist,
            })
        }
        FxSyncPlan::Incremental => {
            let days = (channels.fetch_incremental)()
                .await
                .map_err(classify_fetch_error)?;
            let series = derive_ecb_weekly_series(&days, &pairs);
            ensure_series_has_points(&series)?;
            let persist = session
                .with_connection(move |conn| persist_ecb_fx_series(conn, &series))
                .await?;
            tracing::debug!(
                points = persist.points,
                "ECB 汇率增量同步完成（深度已达成）"
            );
            Ok(FxSyncReport {
                full_backfilled: false,
                persist,
            })
        }
    }
}

/// 同步判据（一次连接访问算清）：币种对 = 币种字典全量对本位币（投资域 FxRateHistory
/// 词条「全字典可折算」，不只「有痕迹的那些」）；深度判据见模块文档。
fn plan_fx_sync(conn: &Connection) -> Result<(FxSyncPlan, Vec<(String, String)>)> {
    let native = default_currency_code(conn)?;
    let mut pairs: Vec<(String, String)> = {
        let mut stmt =
            conn.prepare("SELECT code FROM currencies WHERE code <> ?1 ORDER BY code")?;
        let codes = stmt
            .query_map(params![native], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        codes
            .into_iter()
            .map(|code| (code, native.clone()))
            .collect()
    };
    pairs.dedup();

    let Some(earliest) = earliest_non_native_trace_date(conn, &native)? else {
        return Ok((FxSyncPlan::Skip, pairs));
    };
    // 窗口起点 = 最早痕迹日期所属 ISO 周的周一，再前推一周（序列早于该日期）。
    let window_start = week_monday(earliest) - chrono::Duration::weeks(WINDOW_LEAD_WEEKS);
    if fx_depth_reached(conn, &native, &window_start.format("%Y-%m-%d").to_string())? {
        Ok((FxSyncPlan::Incremental, pairs))
    } else {
        Ok((FxSyncPlan::FullBackfill(window_start), pairs))
    }
}

/// 账本中最早的非本位币相关日期（窗口判据的输入，issue #1544）：非本位币**账户**
/// 的创建日与非本位币**交易**的交易日取 MIN；软删行排除（与「删除即从全部视角
/// 消失」的既有口径一致）；隐藏账户也排除——黑洞账户（V004 种子，承接「资金
/// 账户=无」的导入交易，对用户隐藏）在每本账本必然存在，其创建日是安装时刻
/// 而非用户使用非本位币的起点，计入它会让「零痕迹」判据永不成立；其内的导入
/// 交易不受影响（transactions 无隐藏位，仍是真实痕迹）。无痕迹返回 None
/// （判据：零痕迹 → 不回填）。
fn earliest_non_native_trace_date(conn: &Connection, native: &str) -> Result<Option<NaiveDate>> {
    let raw: Option<String> = conn.query_row(
        "SELECT MIN(d) FROM ( \
             SELECT date(created_at) AS d FROM accounts \
              WHERE is_deleted = 0 AND is_hidden = 0 AND currency_code <> ?1 \
             UNION ALL \
             SELECT date(date) AS d FROM transactions \
              WHERE is_deleted = 0 AND currency_code <> ?1 \
         )",
        params![native],
        |row| row.get(0),
    )?;
    Ok(raw.and_then(|day| NaiveDate::parse_from_str(day.trim(), "%Y-%m-%d").ok()))
}

/// 深度判据（幂等的依据，issue #1544 AC3）：**逐币种对**核对——币种字典里每个
/// 非本位币币种对本位币都已有不晚于窗口起点的周采样行，才判「深度已达成」。
/// 一处的旧覆盖（如东财时代的孤立序列）不掩盖另一对的缺口（全局 MIN 会漏判）；
/// 全量回填一次灌齐全部币种对，常态下第二轮即达成。
fn fx_depth_reached(conn: &Connection, native: &str, window_start: &str) -> Result<bool> {
    let missing: i64 = conn.query_row(
        "SELECT count(*) FROM currencies c \
          WHERE c.code <> ?1 \
            AND NOT EXISTS ( \
                SELECT 1 FROM fx_rate_history f \
                 WHERE f.base_code = c.code AND f.quote_code = ?1 \
                   AND f.week_start <= ?2 \
            )",
        params![native, window_start],
        |row| row.get(0),
    )?;
    Ok(missing == 0)
}

/// 全量腿的窗口裁剪：只保留周键不早于窗口起点的采样点——深度由账本判据决定
/// （issue #1544），不把数据源 1999 年起的整根文件灌进库；解析失败的点按缺失跳过
///（取数层周采样契约保证格式，此处防御性兜底）。
fn trim_series_to_window(
    series: Vec<super::ecb::FxPairWeeklySeries>,
    window_start: NaiveDate,
) -> Vec<super::ecb::FxPairWeeklySeries> {
    series
        .into_iter()
        .map(|mut series| {
            series.points.retain(|(trade_date, _)| {
                NaiveDate::parse_from_str(trade_date.trim(), "%Y-%m-%d")
                    .map(|day| week_monday(day) >= window_start)
                    .unwrap_or(false)
            });
            series
        })
        .collect()
}
