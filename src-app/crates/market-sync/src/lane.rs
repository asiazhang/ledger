//! 后台车道骨架：单轮半边（issue #1426）与调度半边（issue #1622）。
//!
//! **单轮骨架**（两条行情车道）：价格历史补全（[`super::history`]）与每日现价
//! 刷新（[`super::daily_refresh`]）的单轮形态同构：通道束换装（宿主注入槽优先、
//! 生产后台车道束兜底）、门面写槽裸作业会话、进度发射接线、写入见证、收尾裁决
//!（置脏 + 价格失效信号，成败同判）与失败日志。
//!
//! **调度循环**（三条后台车道共用）：进程级单次拉起守卫 → 启动延迟 `sleep` →
//! `loop { 门检（锁定/失败不触碰占位连接）→ 自然日窗口判定 → 标记已跑 + 跑一轮
//! → 巡检 sleep }` 收口在 [`start_daily_lane_kernel`]；三条车道
//!（含不经单轮骨架的每日汇率增量同步 [`super::fx_daily`]）各自只留一枚守卫标志
//! 与本车道一轮的编排 + 统计日志。收敛对象是循环与守卫，不是轮次裁决面——
//! fx_daily 的可重建缓存轮次（无见证 / 无置脏 / 无失效信号）保持域内自带。
//!
//! 单轮骨架的输入 = **编排**（[`LaneRound`] 实现，只回答「这一轮怎么跑」）+ **槽**
//!（[`LaneChannelsSlot`] 实现由各车道模块就地提供，换装接缝的类型身份）+ **车道
//! 标识**（[`LaneId`]：span 归因串与进度事件面的单点配对）；两车道各自只留编排与
//! 统计日志。
//!
//! ADR-0122 决策 3「后台每日现价刷新与历史补全同一调度节奏（同形调度）」的骨架
//! 自此是代码单点：车道形态调整（换装、会话、见证、裁决、发射）只改本模块一处
//! ——issue #1412 的接缝 async 化与槽换异步互斥体曾须两处同改（Shotgun Surgery）；
//! 调度规则调整（门检条件、日历口径、守卫语义）同样只改本模块一处（issue #1622：
//! 三份同构调度循环收敛，第三份随 #1546 每日汇率车道引入使「改一处漏两处」的
//! 风险显著上升）。
//!
//! 用户可见语义零变化：三条车道各自的槽类型（`history::BackfillChannelsSlot` /
//! `daily_refresh::DailyPriceRefreshChannelsSlot` /
//! `fx_daily::DailyFxSyncChannelsSlot`）、进度事件名、置脏 / 失效信号口径与调度
//! 时机（[`STARTUP_DELAY`] / [`WINDOW_POLL_INTERVAL`]）逐字保持。
//!
//! **宿主面**（issue #2079，NSH 切轨④）：巡检循环与单轮骨架的宿主无关内核
//!（[`start_daily_lane_kernel`] / [`run_lane_round_kernel`]）——宿主差异（门检、
//! 执行器、会话解析、通道束注入槽、进度发射、价格失效广播）收进公开宿主接缝
//! [`LaneHost`]，宿主（NSH 原生壳）启动接线时装配（下层定义注册点、上层注册
//! 实现、壳层启动时接线，ADR-0112 决策 5）。旧壳 AppHandle 包装
//!（`run_background_lane_round` / `start_daily_lane` / `LaneChannelsSlot` 槽接缝）
//! 随旧壳退役删除（#2083）。

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ledger_infra::db::facade_handles::DbWriteHandle;
use ledger_infra::error::Result;

use super::channels::SyncFetchChannels;
use super::fx::FxSyncChannels;
use super::incremental::{beijing_today, daily_window_opens};
use super::model::WriteWitness;
use super::progress::SyncProgress;
use super::session::FacadeWriteSession;

/// 后台车道一轮的 future 装箱形态（编排借用的现场：会话 / 通道束 / 进度回调 /
/// 见证），限 `Send` 以便整轮跑在车道的 async 任务上。
pub(super) type LaneRoundFuture<'a, Stats> =
    Pin<Box<dyn Future<Output = Result<Stats>> + Send + 'a>>;

/// 车道标识（骨架的车道差异单点）：span 归因串与进度事件面同址配对——两处各自
/// 传一条串会允许错配（历史车道配现价事件）且无编译期阻隔，这里一次说清。
///
/// 进度事件面：现价刷新与手动同步同事件名（前端只在手动同步在途时消费该事件，
/// 后台推进不点亮进度条），价格历史补全走静默标的级计数事件（ADR-0122 决策 4）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LaneId {
    /// 现价刷新的后台每日形态：`ledger:instrument-sync-progress`（与手动同步同形）。
    DailyPriceRefresh,
    /// 价格历史后台补全：`ledger:history-backfill-progress`（静默计数）。
    HistoryBackfill,
}

impl LaneId {
    /// SQL 归因串（语义同门面作业的 `command` 参数）：同时充当日志的车道判别字段。
    fn span(self) -> &'static str {
        match self {
            Self::DailyPriceRefresh => "daily_price_refresh",
            Self::HistoryBackfill => "history_backfill",
        }
    }

    /// 日志用车道名（失败面文本按车道可读可检索；纯结构化消费方读 `lane` 字段）。
    fn name(self) -> &'static str {
        match self {
            Self::DailyPriceRefresh => "后台每日现价刷新",
            Self::HistoryBackfill => "价格历史后台补全",
        }
    }
}

/// 后台车道的单轮编排（骨架的编排输入）：只回答「这一轮怎么跑」——持已换装的
/// 通道束驱动本车道编排，并在每个实际写入点标记写入见证（成败同判的证据源）。
pub(super) trait LaneRound: Send {
    /// 本轮统计（只被本车道的统计日志消费，骨架不解释其字段）。
    type Stats;

    /// 跑一轮：抓取网络等待以 `await` 表达；落库经会话短暂取连接（网络等待不进
    /// 连接闭包，ADR-0069 决策 4）；单只失败是否中断本轮由编排自行裁决。
    fn run<'a>(
        &'a self,
        session: &'a FacadeWriteSession,
        channels: &'a mut SyncFetchChannels,
        progress: &'a mut (dyn FnMut(SyncProgress) + Send),
        witness: &'a mut WriteWitness,
    ) -> LaneRoundFuture<'a, Self::Stats>;
}

/// 把骨架交出的进度接缝（trait 对象）转接为编排接缝要的泛型 `FnMut`：两者都是
/// 「把一次进度推进投递出去」，非阻塞交接语义不变（编排接缝的泛型形状保留，
/// 转接单点住此，车道侧只写一行）。
pub(super) fn progress_forwarder(
    sink: &mut (dyn FnMut(SyncProgress) + Send),
) -> impl FnMut(SyncProgress) + Send + '_ {
    |update| sink(update)
}

// ---------------------------------------------------------------------------
// 宿主接缝（issue #2079）：宿主件闭集与两条内核
// ---------------------------------------------------------------------------

/// 车道 async 任务的装箱形态（宿主执行器接缝的载荷：宿主自择执行现场）。
pub type LaneTask = Pin<Box<dyn Future<Output = ()> + Send + 'static>>;

/// 共享同步通道束句柄（现价刷新与价格历史补全的注入槽形态，与门内
/// `try_state` 槽位内部同形）。
pub type SharedSyncChannels = Arc<tokio::sync::Mutex<SyncFetchChannels>>;

/// 共享汇率通道束句柄（每日汇率同步的注入槽形态，与门内
/// `DailyFxSyncChannelsSlot` 内部同形）。
pub type SharedFxChannels = Arc<tokio::sync::Mutex<FxSyncChannels>>;

/// 车道宿主接缝（issue #2079 注册点）：宿主启动接线时装配的车道运行环境。
/// 下层定义注册点、上层注册实现、壳层启动时接线（ADR-0112 决策 5）——宿主
///（NSH 原生壳）经三个 `_for_host` 入口消费（历史补全 / 每日现价刷新 /
/// 每日汇率同步各自取用本束的相关字段）。全部回调约定**非阻塞**：车道在后台运行时上跑，宿主
/// 回调阻塞即拖慢巡检节奏（进度发射器契约同源）。
#[derive(Clone)]
pub struct LaneHost {
    /// 每轮门检（锁定 / 启动失败期间不触碰会话的宿主等价判定；NSH = 账本
    /// 会话在场——无占位连接形态，引导失败无会话即空转）。
    pub ready: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
    /// 车道 async 任务的执行器（ADR-0125 决策 7 的宿主等价形：NSH = 自有
    /// 异步运行时现场；任务退出随进程硬停，写入全是幂等 upsert，中断无残留）。
    pub spawn: Arc<dyn Fn(LaneTask) + Send + Sync + 'static>,
    /// 每轮解析门面写槽句柄（`None` = 无会话，轮次空转——会话可在启动后
    /// 消失，每轮重解析不缓存）。
    pub session: Arc<dyn Fn() -> Option<DbWriteHandle> + Send + Sync + 'static>,
    /// 每日现价刷新的通道束注入槽（测试接缝，对准门内 `try_state` 槽位；
    /// `None` = 域内每轮建生产束兜底，生产不管理）。
    pub refresh_channels: Arc<dyn Fn() -> Option<SharedSyncChannels> + Send + Sync + 'static>,
    /// 价格历史补全的通道束注入槽（同上）。
    pub backfill_channels: Arc<dyn Fn() -> Option<SharedSyncChannels> + Send + Sync + 'static>,
    /// 每日汇率同步的通道束注入槽（同上；汇率束与同步束类型不同，分槽注入）。
    pub fx_channels: Arc<dyn Fn() -> Option<SharedFxChannels> + Send + Sync + 'static>,
    /// 现价刷新进度发射（`ledger:instrument-sync-progress` 事件面的宿主实现；
    /// 非阻塞交接即返回，投递失败静默）。
    pub refresh_progress: Arc<dyn Fn(SyncProgress) + Send + Sync + 'static>,
    /// 价格历史补全进度发射（`ledger:history-backfill-progress` 静默计数事件
    /// 面；非阻塞交接即返回）。
    pub backfill_progress: Arc<dyn Fn(SyncProgress) + Send + Sync + 'static>,
    /// 价格失效广播（收尾裁决「实际写过」半边的宿主实现；宿主自行决定
    /// 信号通道投影——NSH 广播进程内失效信号）。
    pub price_written: Arc<dyn Fn() + Send + Sync + 'static>,
}

/// 后台车道单轮内核（issue #2079）：换装（宿主注入槽优先、生产束兜底）→
/// 会话 → 见证 → 编排 → 收尾裁决（置脏 + 宿主价格失效广播，成败同判）→
/// 日志。宿主入口解析宿主件（写槽 / 进度 / 失效广播 / 注入槽）后全部交给本
/// 内核，用户可见语义零变化（旧壳包装随旧壳退役，#2083）。
pub(super) async fn run_lane_round_kernel<L>(
    write: DbWriteHandle,
    lane: LaneId,
    round: L,
    progress: &mut (dyn FnMut(SyncProgress) + Send),
    price_written: &(dyn Fn() + Send + Sync),
    injected_channels: Option<SharedSyncChannels>,
) -> Result<L::Stats>
where
    L: LaneRound,
{
    let span = lane.span();
    let session = FacadeWriteSession::new(write.clone(), span);
    let mut witness = WriteWitness::default();
    let result = match injected_channels {
        Some(shared) => {
            let mut channels = shared.lock().await;
            round
                .run(&session, &mut channels, progress, &mut witness)
                .await
        }
        None => match SyncFetchChannels::production_backfill() {
            Ok(channels) => {
                let channels = tokio::sync::Mutex::new(channels);
                let mut guard = channels.lock().await;
                round
                    .run(&session, &mut guard, progress, &mut witness)
                    .await
            }
            // 建生产束失败按本轮失败处置（与编排失败同路：不置脏、不发射、一次 warn）。
            Err(error) => Err(error),
        },
    };

    if witness.any_written() {
        if let Err(error) = write.run(span, |_| Ok(())).await {
            tracing::warn!(
                %error,
                lane = %span,
                "{}收尾置脏失败（脏标记待下次写入补上）",
                lane.name()
            );
        }
        price_written();
    }
    if let Err(error) = &result {
        tracing::warn!(%error, lane = %span, "{}失败（静默等下一窗口）", lane.name());
    }
    result
}

// ---------------------------------------------------------------------------
// 调度循环单点（issue #1622）：三条后台车道共用的巡检循环
// ---------------------------------------------------------------------------

/// 应用启动后的首轮延迟：让出启动期（引导、参考数据装载、首屏渲染），再开始
/// 第一轮。延迟是可逆工程决策，测试注入 [`LaneTimings`] 覆写。三条后台车道
///（价格历史补全、每日现价刷新、每日汇率增量同步）同一节奏（同形调度）。
const STARTUP_DELAY: Duration = Duration::from_secs(30);

/// 自然日窗口的巡检周期：任务低频醒来比对北京日历日，跨日即跑当天的窗口。
/// 与自动备份调度、多端同步轮询同一「低频」品味（分钟级间隔，代价为零——
/// 每次巡检只做一次日期比对）。
const WINDOW_POLL_INTERVAL: Duration = Duration::from_secs(10 * 60);

/// 三条后台车道共用的调度时机（issue #1622 收敛：三份同构 timing 类型并成一份
/// ——`startup_delay` + `window_poll` + 生产默认即全部形状）：生产走默认值
///（三条车道同一节奏），接线型集成测试注入短时机——断言只等窗口到达，不被
/// 生产常数拖慢（先例：多端同步 `TriggerTimings`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaneTimings {
    /// 应用启动后到首轮的延迟。
    pub startup_delay: Duration,
    /// 自然日窗口的巡检周期（跨北京日历日即跑当天窗口）。
    pub window_poll: Duration,
}

impl Default for LaneTimings {
    fn default() -> Self {
        Self {
            startup_delay: STARTUP_DELAY,
            window_poll: WINDOW_POLL_INTERVAL,
        }
    }
}

/// 后台车道调度循环内核（issue #1622 单点，issue #2079 宿主化）：进程级单次
/// 拉起 → 启动延迟 →「门检 → 自然日窗口判定 → 标记已跑 + 跑一轮 → 巡检
/// sleep」的巡检任务。门检与执行器由宿主接缝注入，其余语义逐字不变：
///
/// - **单次拉起**（原位重引导重复调用幂等，ADR-0080）：`spawned` 是各车道自己
///   的进程级标志——每车道一枚、声明住车道入口函数体（不能收进本函数体：泛型
///   函数体内的 `static` 是全体实例化共享的一枚，三条车道会互相顶掉），本函数
///   只收口 `swap` 判定与「第二次调用直接返回」的语义。
/// - **每轮门检**（先例：自动备份调度，issue #644 / ADR-0080）：宿主 ready 判定
///   为假期间不触碰会话；门开后首个到达的自然日窗口即补跑——启动轮即当天的
///   窗口（「启动后延迟一次 + 此后每日各一次」）。
/// - **同日只开一次窗口**（规则单点见 `incremental::daily_window_opens`）：循环
///   只负责把判定接上「标记已跑 + 跑一轮」的副作用；失败不回拨标记——重试等
///   下一自然日窗口（即时补救走手动入口）。
/// - **执行器与定时**（ADR-0125 决策 7 / issue #1413）：宿主执行器拉起的 async
///   任务 + 异步定时，启动延迟与自然日窗口用 `tokio::time::sleep`；应用退出即
///   任务随进程硬停，无需优雅关闭（写入全是幂等 upsert，中断无残留）。
pub(super) fn start_daily_lane_kernel(
    spawned: &'static AtomicBool,
    timings: LaneTimings,
    ready: Arc<dyn Fn() -> bool + Send + Sync + 'static>,
    executor: Arc<dyn Fn(LaneTask) + Send + Sync + 'static>,
    round: impl Fn() -> LaneTask + Send + 'static,
) {
    if spawned.swap(true, Ordering::SeqCst) {
        return;
    }
    executor(Box::pin(async move {
        // 启动延迟：让出启动期再补跑当天首轮。
        tokio::time::sleep(timings.startup_delay).await;
        let mut last_round_date: Option<chrono::NaiveDate> = None;
        loop {
            if ready() {
                let today = beijing_today();
                if daily_window_opens(last_round_date, today) {
                    last_round_date = Some(today);
                    round().await;
                }
            }
            tokio::time::sleep(timings.window_poll).await;
        }
    }));
}
