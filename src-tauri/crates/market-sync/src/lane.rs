//! 后台车道骨架：单轮半边（issue #1426）与调度半边（issue #1622）。
//!
//! **单轮骨架**（两条行情车道）：价格历史补全（[`super::history`]）与每日现价
//! 刷新（[`super::daily_refresh`]）的单轮形态同构：通道束换装（管理态桩槽优先、
//! 生产后台车道束兜底）、门面写槽裸作业会话、进度发射接线、写入见证、收尾裁决
//!（置脏 + 价格失效信号，成败同判）与失败日志。
//!
//! **调度循环**（三条后台车道共用）：进程级单次拉起守卫 → 两扇门
//!（[`EncryptionGate`] / [`BootFailureGate`]）clone → `tauri::async_runtime::spawn`
//! → 启动延迟 `sleep` → `loop { 门检（锁定/失败不触碰占位连接）→ 自然日窗口判定
//! → 标记已跑 + 跑一轮 → 巡检 sleep }` 收口在 [`start_daily_lane`]；三条车道
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

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

use ledger_infra::db::DbState;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::error::Result;
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};

use super::channels::SyncFetchChannels;
use super::incremental::{beijing_today, daily_window_opens};
use super::model::WriteWitness;
use super::progress::{BackfillProgressEmitter, ProgressEmitter, SyncProgress};
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

/// 车道通道束槽（骨架的换装接缝）：两条车道各自的公开槽类型实现本 trait，骨架按
/// 同一形状（共享通道束 + 异步互斥体，引用在 `.await` 之间存活）解包——「管理态
/// 桩束优先、生产后台车道束兜底」的换装判据收口在骨架一处。
///
/// 生产不管理本状态（每轮建后台车道生产束）；集成测试 manage 本状态并装入门控
/// 桩束，使「车道真实在途」可确定复现（先例：命令壳 `SyncChannelsSlot`）。
///
/// 实现住**各车道自己的模块**（`for BackfillChannelsSlot` 住 `history`、`for
/// DailyPriceRefreshChannelsSlot` 住 `daily_refresh`）：接缝由骨架定义、槽类型由
/// 车道提供，依赖方向单向（车道 → 骨架），骨架不反向认识自己的消费者。
pub(super) trait LaneChannelsSlot: Send + Sync + 'static {
    /// 槽内共享的通道束（克隆共享句柄，不取走管理态）。
    fn shared(&self) -> Arc<tokio::sync::Mutex<SyncFetchChannels>>;
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

/// 后台车道单轮骨架（issue #1426）：换装 → 会话 → 见证 → 编排 → 收尾裁决 → 日志。
///
/// - **换装**：管理态桩槽（[`LaneChannelsSlot`]）优先——集成测试注入门控桩束，
///   使「车道真实在途」可确定复现；未管理时每轮建后台车道生产束
///   （[`SyncFetchChannels::production_backfill`]：同一进程级全局限速器、前台请求
///   在途时让行，ADR-0122 决策 6）。
/// - **会话**：门面写槽裸作业会话（[`FacadeWriteSession`]，issue #1412 async
///   形态），span 归因串由本函数统一接线。
/// - **裁决**（issue #1277 成败同判）：本轮实际写过（见证跨失败存活）→ 提交点
///   置脏一次 + 发既有价格失效信号；零写入不置脏不广播。置脏失败记日志不静默吞
///   运行结果（脏标记待下次写入补上）。
/// - **日志**：本轮失败静默等下一窗口（无用户可报），在此按车道一次收尾；统计
///   日志归各车道（骨架不解释统计字段）。
pub(super) async fn run_background_lane_round<R, S, L>(
    app: &AppHandle<R>,
    lane: LaneId,
    round: L,
) -> Result<L::Stats>
where
    R: Runtime,
    S: LaneChannelsSlot,
    L: LaneRound,
{
    let span = lane.span();
    let write = app.state::<DbState>().write_handle();
    let session = FacadeWriteSession::new(write.clone(), span);
    let mut witness = WriteWitness::default();
    // 进度事件与手动同步同形（同一 payload 形状），事件名按车道选择；前端接缝
    // 只在手动同步在途时消费进度条事件，后台推进不点亮进度条。
    let mut progress = |progress: SyncProgress| match lane {
        LaneId::DailyPriceRefresh => ProgressEmitter::emit_progress(app, progress),
        LaneId::HistoryBackfill => app.emit_backfill_progress(progress),
    };
    let result = match app.try_state::<S>() {
        Some(slot) => {
            let shared = slot.shared();
            let mut channels = shared.lock().await;
            round
                .run(&session, &mut channels, &mut progress, &mut witness)
                .await
        }
        None => match SyncFetchChannels::production_backfill() {
            Ok(channels) => {
                let channels = tokio::sync::Mutex::new(channels);
                let mut guard = channels.lock().await;
                round
                    .run(&session, &mut guard, &mut progress, &mut witness)
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
        emit_for(
            app,
            WriteOp::SyncInstrumentInfo,
            WriteEvidence::PriceWritten(true),
        );
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

/// 后台车道调度循环单点（issue #1622）：进程级单次拉起 → 启动延迟 →
/// 「门检 → 自然日窗口判定 → 标记已跑 + 跑一轮 → 巡检 sleep」的巡检任务。
///
/// - **单次拉起**（原位重引导重复调用幂等，ADR-0080）：`spawned` 是各车道自己
///   的进程级标志——每车道一枚、声明住车道入口函数体（不能收进本函数体：泛型
///   函数体内的 `static` 是全体实例化共享的一枚，三条车道会互相顶掉），本函数
///   只收口 `swap` 判定与「第二次调用直接返回」的语义。
/// - **每轮门检**（先例：自动备份调度，issue #644 / ADR-0080）：锁定/启动失败
///   期间不触碰占位连接；门开后首个到达的自然日窗口即补跑——启动轮即当天的
///   窗口（「启动后延迟一次 + 此后每日各一次」）。
/// - **同日只开一次窗口**（规则单点见 `incremental::daily_window_opens`）：循环
///   只负责把判定接上「标记已跑 + 跑一轮」的副作用；失败不回拨标记——重试等
///   下一自然日窗口（即时补救走手动入口）。
/// - **执行器**（ADR-0125 决策 7 / issue #1413）：挂全局运行时的 async 任务，
///   启动延迟与自然日窗口用异步定时；应用退出即任务随进程硬停，无需优雅关闭
///   （写入全是幂等 upsert，中断无残留）。
///
/// `round` 是本车道一轮的编排（收 handle 克隆，async 体归车道模块：走单轮骨架
/// 还是自带最小轮次、以及统计日志，是车道自己的裁决面）。
pub(super) fn start_daily_lane<R, F, Fut>(
    app: &AppHandle<R>,
    spawned: &'static AtomicBool,
    timings: LaneTimings,
    round: F,
) where
    R: Runtime,
    F: Fn(AppHandle<R>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    if spawned.swap(true, Ordering::SeqCst) {
        return;
    }
    let gate = EncryptionGate::clone(&app.state::<EncryptionGate>());
    let boot_gate = BootFailureGate::clone(&app.state::<BootFailureGate>());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        // 启动延迟：让出启动期再补跑当天首轮。
        tokio::time::sleep(timings.startup_delay).await;
        let mut last_round_date: Option<chrono::NaiveDate> = None;
        loop {
            if !gate.is_locked() && !boot_gate.is_failed() {
                let today = beijing_today();
                if daily_window_opens(last_round_date, today) {
                    last_round_date = Some(today);
                    round(handle.clone()).await;
                }
            }
            tokio::time::sleep(timings.window_poll).await;
        }
    });
}
