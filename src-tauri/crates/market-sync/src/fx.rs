//! ECB 汇率同步编排（issue #1544 / ADR-0019 修订记录）：给汇率同步定「要不要拉、
//! 拉哪条腿」的深度判据单点，与全量回填 / 90 天增量两条取数腿的域内编排。
//!
//! 用户可观察结果：账本里出现非本位币痕迹（账户或交易）后，一次同步即把各币种对
//! 的历史序列**整份补齐**（写路径按交易日折算的取数因此可用），
//! 本模块不读 instruments、不碰 price_history、不进 [`super::history`] 的队列）。
//!
//! 三块事实收口在本模块：
//! - **深度判据**（[`plan_fx_sync`]）：零痕迹 → Skip（[`has_non_native_trace`]：非本位币
//!   账户的 `created_at` 或非本位币交易的 `date` 存在即有痕迹；软删行与隐藏账户
//!   排除——黑洞账户是 V004 种子的系统缓冲池，其创建日是安装时刻而非用户使用
//!   非本位币的起点，其内的交易仍是真实痕迹）→ 零请求零落库（不做过深的无谓
//!   回填，AC 有断言）。有痕迹时深度 = 落库序列对**数据源起点**的覆盖
//!   （[`fx_depth_reached`]，**逐币种对**核对，一处旧覆盖不掩盖另一对的缺口；
//!   覆盖证据只认 [`ECB_FX_SOURCE`]，issue #1551 AC）：未达起点 → 全量腿**整份灌库**
//!   （不裁剪），已达起点 → 只走 90 天增量，不重复拉全量（幂等，AC 有断言）。
//!   #1759 收敛：窗口起点不再由「最早非本位币痕迹」派生——痕迹派生窗口死锁历史
//!   导入（历史交易写入需要该周汇率 → 该周汇率需要全量腿 → 全量腿需要更早的
//!   痕迹 → 痕迹只能由历史写入产生）；取数腿本来就整份下载（全量文件无日期参数），
//!   裁剪只省本地行（~3 MB），不为省行保留死锁判据。
//! - **一轮同步**（[`sync_fx_rates`]）：按判据取数（全量文件或 90 天增量文件，
//!   币种对 = 币种字典全量对本位币，见投资域 FxRateHistory 词条「全字典可折算」）、
//!   交叉推导周采样、整份文件灌库后交落库单元 [`persist_ecb_fx_series`]（#1543：单一事务、整周
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

use std::sync::atomic::{AtomicBool, Ordering};

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
use super::persist::{ECB_FX_SOURCE, FxPersistReport, persist_ecb_fx_series};
use super::progress::FxSyncProgress;
use super::session::ScopedSession;

/// 深度判据的「起点」锚：ECB 全量文件自 1999-01-04 起发布，但币种字典各对全以
/// CNY 为报价腿，CNY 腿 2005-04-01 起才有报价（实测 eurofxref-hist 最早 CNY 报价日；
/// [`super::ecb`] 模块文档同记「CNY 腿 2005-04 才存在」）——整份灌库后各对最早周键
/// = 2005-04-01 所属 ISO 周的周一 2005-03-28。每对存在不晚于该周的 ECB 周采样行即判
/// 「起点已覆盖」（#1759：深度看落库序列对数据源起点的覆盖，不看账本痕迹日期）。
/// 锚失配的最坏面 = 该对判「未达深」而每轮重拉全量（值仍正确、整周覆盖幂等），
/// 不静默错值；锚与周键口径的绑定由测试 `fx_source_origin_anchor_is_the_cny_leg_origin_week` 钉住。
pub(crate) const FX_SOURCE_ORIGIN_WEEK: &str = "2005-03-28";

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

/// 「已有汇率同步在进行」码化错误（issue #1762）：手动与每日自动两路触发
/// 共用编排层在途互斥，撞车时第二个触发立即返回、不排队等待。params 无动态值
///（ADR-0050）。
fn sync_in_progress() -> AppError {
    AppError::coded("fx.sync-in-progress", "已有汇率同步在进行，请稍后再试")
}

/// 汇率同步在途互斥门（issue #1762）：进程级在途标志的持有者——手动入口与
/// 每日自动入口经进程级单例 [`FX_SYNC_GATE`] 共用同一扇门，第二路触发立即返回
/// [`sync_in_progress`] 码化错误，不走队列、不等待。互斥语义从「可并发」改为
/// 「在途互斥」是对既有两路写入语义的收窄（落库仍是单一事务整体回滚，语义不变）。
///
/// 测试隔离：门是显式传入的持有对象——域单测各持自有门，互不干扰；两生产入口
/// 共用进程级单例。
pub struct FxSyncGate {
    in_flight: AtomicBool,
}

impl FxSyncGate {
    /// 新建一扇关闭的门（测试各持自有门；生产共用 [`FX_SYNC_GATE`]）。
    pub const fn new() -> Self {
        Self {
            in_flight: AtomicBool::new(false),
        }
    }

    /// 尝试进门：在途时返回 `None`（调用方报在途互斥码化错误），空闲时返回
    /// 持门守卫——守卫释放（一切路径含提前返回）即清空在途登记。
    fn try_begin(&self) -> Option<FxSyncGuard<'_>> {
        self.in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| FxSyncGuard { gate: self })
    }
}

impl Default for FxSyncGate {
    fn default() -> Self {
        Self::new()
    }
}

/// 持门守卫：释放即清空在途登记（新触发可立即进门）。
struct FxSyncGuard<'a> {
    gate: &'a FxSyncGate,
}

impl Drop for FxSyncGuard<'_> {
    fn drop(&mut self) {
        self.gate.in_flight.store(false, Ordering::Release);
    }
}

/// 进程级汇率同步在途互斥单例（issue #1762）：手动 IPC 命令与每日自动车道
/// 共用同一扇门——任一在途时另一路立即报在途互斥，不并发、不排队。
pub static FX_SYNC_GATE: FxSyncGate = FxSyncGate::new();

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
    /// 全量历史回填：整份文件灌库（不裁剪，#1759）。
    FullBackfill,
    /// 深度已达成：只走 90 天增量。
    Incremental,
}

/// 一轮汇率同步（issue #1544，#1545 手动入口与 #1546 每日增量的共同编排）：
/// 读库定判据 → 会话外取数 → 交叉推导周采样 → 落库（单一事务幂等）。
///
/// 判据与取数腿的对应：零痕迹 = [`FxSyncPlan::Skip`]（两个通道都不碰）；深度未达
/// = 全量腿整份灌库；深度已达 = 只走增量腿（不重复拉全量）。返回 [`FxSyncReport`]。
///
/// 进度与互斥（issue #1762）：阶段推进与在途互斥见 [`sync_fx_rates_with_progress`]
/// 与 [`sync_fx_rates_guarded`]；本函数是无进度直调（域单测判据 / 落库行为的目标形态）。
pub async fn sync_fx_rates<S: ScopedSession>(
    session: &S,
    channels: &mut FxSyncChannels,
) -> Result<FxSyncReport> {
    sync_fx_rates_with_progress(session, channels, &mut |_| {}).await
}

/// 带阶段进度的汇率同步（issue #1762）：与 [`sync_fx_rates`] 同编排，另经 `progress`
/// 发出阶段推进——取数腿调用前报 `fetching`，解析完成落库前报携带已解析天数的
/// `persisting`；零痕迹跳过不取数不落库、无任何阶段事件。`progress` 不得阻塞
///（触发面接到非阻塞事件发射器，发射失败不影响同步结果）。
pub async fn sync_fx_rates_with_progress<S, P>(
    session: &S,
    channels: &mut FxSyncChannels,
    progress: &mut P,
) -> Result<FxSyncReport>
where
    S: ScopedSession,
    P: FnMut(FxSyncProgress) + Send,
{
    let (plan, pairs) = session.with_connection(plan_fx_sync).await?;
    match plan {
        FxSyncPlan::Skip => {
            tracing::debug!("账本无非本位币痕迹，汇率同步跳过（零请求零落库）");
            Ok(FxSyncReport::default())
        }
        FxSyncPlan::FullBackfill => {
            progress(FxSyncProgress::fetching());
            let days = (channels.fetch_full)()
                .await
                .map_err(classify_fetch_error)?;
            let series = derive_ecb_weekly_series(&days, &pairs);
            ensure_series_has_points(&series)?;
            progress(FxSyncProgress::persisting(days.len()));
            let persist = session
                .with_connection(move |conn| persist_ecb_fx_series(conn, &series))
                .await?;
            tracing::info!(
                earliest = persist.earliest.as_deref().unwrap_or("-"),
                points = persist.points,
                "ECB 汇率全量回填完成（整份文件灌库）"
            );
            Ok(FxSyncReport {
                full_backfilled: true,
                persist,
            })
        }
        FxSyncPlan::Incremental => {
            progress(FxSyncProgress::fetching());
            let days = (channels.fetch_incremental)()
                .await
                .map_err(classify_fetch_error)?;
            let series = derive_ecb_weekly_series(&days, &pairs);
            ensure_series_has_points(&series)?;
            progress(FxSyncProgress::persisting(days.len()));
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

/// 持门汇率同步（issue #1762）：两路触发（手动 IPC、每日自动）在编排层的统一
/// 在途互斥入口——持 `gate` 进门后跑 [`sync_fx_rates_with_progress`]；撞车时立即
/// 返回 [`sync_in_progress`] 码化错误，不走队列、不等待。两生产入口共用
/// [`FX_SYNC_GATE`]，门对象显式传入以便域单测各持自有门。
pub async fn sync_fx_rates_guarded<S, P>(
    gate: &FxSyncGate,
    session: &S,
    channels: &mut FxSyncChannels,
    progress: &mut P,
) -> Result<FxSyncReport>
where
    S: ScopedSession,
    P: FnMut(FxSyncProgress) + Send,
{
    let _guard = gate.try_begin().ok_or_else(sync_in_progress)?;
    sync_fx_rates_with_progress(session, channels, progress).await
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

    if !has_non_native_trace(conn, &native)? {
        return Ok((FxSyncPlan::Skip, pairs));
    }
    if fx_depth_reached(conn, &native)? {
        Ok((FxSyncPlan::Incremental, pairs))
    } else {
        Ok((FxSyncPlan::FullBackfill, pairs))
    }
}

/// 账本是否存在非本位币痕迹（零痕迹判据的输入）：非本位币**账户**（软删行排除，
/// 与「删除即从全部视角消失」的既有口径一致；隐藏账户也排除——黑洞账户（V004
/// 种子，承接「资金账户=无」的导入交易，对用户隐藏）在每本账本必然存在，其创建
/// 日是安装时刻而非用户使用非本位币的起点，计入它会让「零痕迹」判据永不成立；
/// 其内的导入交易不受影响（transactions 无隐藏位，仍是真实痕迹）或非本位币
/// **交易**的交易日（软删排除）。只定「要不要同步」，不定「同步多深」——深度
/// 判据（[`fx_depth_reached`]）只看落库序列对数据源起点的覆盖（#1759：痕迹
/// 日期派生窗口会把历史导入判成永久不可达）。
fn has_non_native_trace(conn: &Connection, native: &str) -> Result<bool> {
    let found: i64 = conn.query_row(
        "SELECT EXISTS( \
             SELECT 1 FROM accounts \
              WHERE is_deleted = 0 AND is_hidden = 0 AND currency_code <> ?1 \
             UNION ALL \
             SELECT 1 FROM transactions \
              WHERE is_deleted = 0 AND currency_code <> ?1 \
         )",
        params![native],
        |row| row.get(0),
    )?;
    Ok(found == 1)
}

/// 深度判据（幂等的依据，issue #1544 AC3）：**逐币种对**核对——币种字典里每个
/// 非本位币币种对本位币都已有不晚于起点锚（[`FX_SOURCE_ORIGIN_WEEK`]）的周采样行，
/// 一处的旧覆盖（如东财时代的孤立序列）不掩盖另一对的缺口（全局 MIN 会漏判）；
/// 全量回填一次灌齐全部币种对，常态下第二轮即达成。
/// 覆盖证据只认新来源（[`ECB_FX_SOURCE`]，issue #1551 AC）：存量旧来源行（东财
/// 时代的 `fx_rate_history` 行）不是 ECB 序列的覆盖证据，不计入深度——否则
/// 升级账本的首次同步会被误判「已达深」而跳过全量回填，旧值永远不被纠正。
fn fx_depth_reached(conn: &Connection, native: &str) -> Result<bool> {
    let missing: i64 = conn.query_row(
        "SELECT count(*) FROM currencies c \
          WHERE c.code <> ?1 \
            AND NOT EXISTS ( \
                SELECT 1 FROM fx_rate_history f \
                 WHERE f.base_code = c.code AND f.quote_code = ?1 \
                   AND f.week_start <= ?2 AND f.source = ?3 \
            )",
        params![native, FX_SOURCE_ORIGIN_WEEK, ECB_FX_SOURCE],
        |row| row.get(0),
    )?;
    Ok(missing == 0)
}
