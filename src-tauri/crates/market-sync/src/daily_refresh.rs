//! 现价刷新的后台每日形态（ADR-0122 决策 3 / issue #1377）：「同步标的信息」
//! 的两种触发形态之一——应用启动后延迟补跑一次，此后每个自然日窗口各跑一次。
//!
//! 与手动形态**同形、同取数面**：消费同一编排
//!（[`super::incremental::do_incremental_sync_with`]，经 [`super::channels`]
//! 的通道束）、同一进度事件（[`super::progress::INSTRUMENT_SYNC_PROGRESS`]，
//! 前端进度条只在手动同步在途时消费事件，后台推进不点亮 UI）、同一收尾裁决
//!（实际写入 → 置脏 + 价格失效信号，成败同判）。与手动形态的差异只有三处：
//!
//! - **触发**：系统自动（本模块调度任务），不占「用户动作在途唯一」槽位
//!   ——在途唯一是前端对用户可见动作防重复点击的承诺，不管辖系统自己的任务；
//! - **车道**：后台车道（[`super::channels::SyncFetchChannels::production_backfill`]），
//!   同一进程级全局限速器、前台请求在途时让行（ADR-0122 决策 6「共享额度让路」）；
//! - **失败面**：静默等下一窗口（无用户在场可报，先例：后台补全调度）。
//!
//! 单轮骨架（换装 / 会话 / 见证 / 裁决 / 发射 / 失败日志）与价格历史后台补全
//! 共用 [`super::lane`] 单点（issue #1426）：本模块只留编排（[`DailyRefreshRound`]）
//! 与统计日志。
//!
//! 调度与启动接线（[`start_daily_price_refresh`]）与价格历史后台补全调度
//!（[`super::history::start_history_backfill`]）共用域内单点：进程级单次拉起
//! 守卫（原位重引导幂等，ADR-0080）、每轮门检（锁定/启动失败期间不触碰占位
//! 连接）、自然日窗口比对北京日历日，循环收口在
//! [`super::lane::start_daily_lane`]（issue #1622）；启动接线收进壳层后台服务
//! 编排单点（issue #961 名单，`scripts/check-background-services.ts` 守门）。
//! 车道为挂全局运行时的 async 任务（ADR-0125 决策 7 / issue #1413），异步定时
//! 承载启动延迟与自然日窗口。
//!
//! 首刷与历史采集不归本任务：现价刷新不落单点（无历史序列者落单点会冒充
//! 「历史完整」，破坏首刷判据），历史归价格历史后台补全（[`super::history`]）。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tauri::{AppHandle, Runtime};

use super::channels::SyncFetchChannels;
use super::channels::do_incremental_sync_channels;
use super::lane::{
    LaneChannelsSlot, LaneId, LaneRound, LaneRoundFuture, LaneTimings, progress_forwarder,
    run_background_lane_round, start_daily_lane,
};
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use super::progress::SyncProgress;
use super::session::FacadeWriteSession;

/// 后台每日现价刷新的通道注入接缝（先例：命令壳 `SyncChannelsSlot` / 后台补全
/// `BackfillChannelsSlot`）：生产**不管理**本状态（每轮建后台车道生产束），
/// 集成测试 manage 本状态并装入门控桩束，使「每日刷新真实在途」可确定复现。
/// 异步互斥体：通道束引用在 `.await` 之间存活（issue #1412 通道束闭包 async 化）。
pub struct DailyPriceRefreshChannelsSlot(pub Arc<tokio::sync::Mutex<SyncFetchChannels>>);

/// 槽接缝实现（骨架定义接缝、车道就地提供槽类型，issue #1426）：骨架按同一形状
/// 解包本状态，不反向认识本车道。
impl LaneChannelsSlot for DailyPriceRefreshChannelsSlot {
    fn shared(&self) -> Arc<tokio::sync::Mutex<SyncFetchChannels>> {
        self.0.clone()
    }
}

/// 后台每日现价刷新的启动入口（壳层后台服务编排单点调用，issue #961 名单）：
/// 进程级单次拉起与「启动延迟补跑一次 + 每自然日窗口一次」的巡检循环归
/// [`super::lane::start_daily_lane`] 域内单点（issue #1622），本模块只留每车道
/// 一枚守卫标志与一轮编排 + 统计日志（async 任务挂全局运行时，ADR-0125 决策 7 /
/// issue #1413）。
pub fn start_daily_price_refresh<R: Runtime>(app: &AppHandle<R>) {
    start_daily_price_refresh_with(app, LaneTimings::default());
}

/// 同 [`start_daily_price_refresh`]，调度时机可注入（测试短时机先例
/// `start_history_backfill_with`）。
pub fn start_daily_price_refresh_with<R: Runtime>(app: &AppHandle<R>, timings: LaneTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    start_daily_lane(app, &SPAWNED, timings, |handle| {
        run_daily_refresh_round(handle)
    });
}

/// 每日现价刷新的编排（骨架的编排输入，issue #1426）：与手动同步**同一编排入口**
/// 与同一取数面——骨架负责换装、会话、见证、裁决、发射与失败日志，本 impl 只把
/// 持束驱动编排。
struct DailyRefreshRound;

impl LaneRound for DailyRefreshRound {
    type Stats = SyncInstrumentInfoResult;

    fn run<'a>(
        &'a self,
        session: &'a FacadeWriteSession,
        channels: &'a mut SyncFetchChannels,
        progress: &'a mut (dyn FnMut(SyncProgress) + Send),
        witness: &'a mut WriteWitness,
    ) -> LaneRoundFuture<'a, Self::Stats> {
        Box::pin(async move {
            // 骨架交来进度接缝的 trait 对象，编排接缝要泛型 `FnMut`：经骨架的
            // 转接单点（[`super::lane::progress_forwarder`]）交给编排。
            let mut forward = progress_forwarder(progress);
            do_incremental_sync_channels(session, channels, &mut forward, witness).await
        })
    }
}

/// 跑一轮每日现价刷新（后台车道单轮骨架，issue #1426）：换装 / 会话 / 见证 /
/// 裁决 / 发射 / 失败日志归 [`super::lane`] 单点（与价格历史后台补全共用），本
/// 函数只留本车道的统计日志。async 形态（ADR-0125 决策 7 / issue #1413）：编排
/// 直接在车道 async 任务上 `await`，不再经全局运行时跨线程驱动。
async fn run_daily_refresh_round<R: Runtime>(app: AppHandle<R>) {
    let result = run_background_lane_round::<R, DailyPriceRefreshChannelsSlot, _>(
        &app,
        LaneId::DailyPriceRefresh,
        DailyRefreshRound,
    )
    .await;
    match result {
        Ok(stats) if stats.written == 0 && stats.renamed == 0 => {
            tracing::debug!("每日现价刷新完成：全部已是最新，零写入");
        }
        Ok(stats) => tracing::info!(
            synced = stats.synced,
            written = stats.written,
            renamed = stats.renamed,
            skipped = stats.skipped,
            "后台每日现价刷新完成"
        ),
        // 失败已由骨架按车道记日志（静默等下一窗口），此处不重复。
        Err(_) => {}
    }
}
