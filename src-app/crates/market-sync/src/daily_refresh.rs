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
//! 调度与启动接线与价格历史后台补全调度共用域内单点：进程级单次拉起
//! 守卫（原位重引导幂等，ADR-0080）、每轮门检（锁定/启动失败期间不触碰占位
//! 连接）、自然日窗口比对北京日历日，循环收口在 [`super::lane`] 内核（issue
//! #1622 收敛；宿主启动入口 [`start_daily_price_refresh_for_host`]，
//! issue #2079；旧壳 tauri 门内入口随旧壳退役，#2083）；启动接线收进壳层
//! 启动单点（NSH 住 `run_startup` 成功分支）。车道为挂异步运行时的 async
//! 任务（ADR-0125 决策 7 / issue #1413；执行器经 [`super::lane::LaneHost`]
//! 供给），异步定时承载启动延迟与自然日窗口。
//!
//! 首刷与历史采集不归本任务：现价刷新不落单点（无历史序列者落单点会冒充
//! 「历史完整」，破坏首刷判据），历史归价格历史后台补全（[`super::history`]）。

use std::sync::atomic::AtomicBool;

use super::channels::SyncFetchChannels;
use super::channels::do_incremental_sync_channels;
use super::lane::{
    LaneHost, LaneId, LaneRound, LaneRoundFuture, LaneTimings, progress_forwarder,
    run_lane_round_kernel, start_daily_lane_kernel,
};
use super::model::{SyncInstrumentInfoResult, WriteWitness};
use super::progress::SyncProgress;
use super::session::FacadeWriteSession;

/// 后台每日现价刷新的启动入口（issue #2079 注册点）：NSH 壳经公开宿主面
/// 接线——门检 / 执行器 / 会话解析 / 通道束注入槽 / 进度发射 / 价格失效广播
/// 由宿主束（[`super::lane::LaneHost`]）装配，巡检循环与单轮骨架同走内核；
/// 旧壳 AppHandle 启动入口随旧壳退役（#2083），本入口是唯一入口。
pub fn start_daily_price_refresh_for_host(host: &LaneHost, timings: LaneTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    let host = host.clone();
    start_daily_lane_kernel(
        &SPAWNED,
        timings,
        host.ready.clone(),
        host.spawn.clone(),
        move || {
            let host = host.clone();
            Box::pin(run_daily_refresh_round_for_host(host))
        },
    );
}

/// 跑一轮每日现价刷新（宿主面，issue #2079）：会话解析 / 通道束换装 /
/// 见证 / 收尾裁决 / 发射 / 失败日志归 [`super::lane`] 内核（宿主束供给
/// 宿主件），本函数只留本车道的统计日志。会话缺失轮次空转。
async fn run_daily_refresh_round_for_host(host: LaneHost) {
    let Some(write) = (host.session)() else {
        tracing::debug!("账本会话未就位，本轮空转");
        return;
    };
    let progress = host.refresh_progress.clone();
    let mut sink = move |update: SyncProgress| progress(update);
    let result = run_lane_round_kernel(
        write,
        LaneId::DailyPriceRefresh,
        DailyRefreshRound,
        &mut sink,
        &*host.price_written,
        (host.refresh_channels)(),
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
        // 失败已由内核按车道记日志（静默等下一窗口），此处不重复。
        Err(_) => {}
    }
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
