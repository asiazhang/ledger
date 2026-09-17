//! 现价刷新的后台每日形态（ADR-0122 决策 3 / issue #1377）：「同步标的信息」
//! 的两种触发形态之一——应用启动后延迟补跑一次，此后每个自然日窗口各跑一次。
//!
//! 与手动形态**同形、同取数面**：消费同一编排
//!（[`super::incremental::do_incremental_sync_with`]，经 [`super::channels`]
//! 的通道束）、同一进度事件（[`super::progress::INSTRUMENT_SYNC_PROGRESS`]，
//! 前端进度条只在手动同步在途时消费事件，后台推进不点亮 UI）、同一收尾裁决
//!（实际写入 → 置脏 + 价格失效信号，成败同判）。与手动形态的差异只有三处：
//!
//! - **触发**：系统自动（本模块调度线程），不占「用户动作在途唯一」槽位
//!   ——在途唯一是前端对用户可见动作防重复点击的承诺，不管辖系统自己的任务；
//! - **车道**：后台车道（[`super::channels::SyncFetchChannels::production_backfill`]），
//!   同一进程级全局限速器、前台请求在途时让行（ADR-0122 决策 6「共享额度让路」）；
//! - **失败面**：静默等下一窗口（无用户在场可报，先例：后台补全调度）。
//!
//! 调度与启动接线（[`start_daily_price_refresh`]）与价格历史后台补全调度
//!（[`super::history::start_history_backfill`]）同构：进程级单次拉起守卫
//!（原位重引导幂等，ADR-0080）、每轮门检（锁定/启动失败期间不触碰占位连接）、
//! 自然日窗口比对北京日历日；启动接线收进壳层后台服务编排单点（issue #961
//! 名单，`scripts/check-background-services.ts` 守门）。
//!
//! 首刷与历史采集不归本任务：现价刷新不落单点（无历史序列者落单点会冒充
//! 「历史完整」，破坏首刷判据），历史归价格历史后台补全（[`super::history`]）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime};

use ledger_infra::db::DbState;
use ledger_infra::db::DbWriteHandle;
use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::error::Result;
use ledger_infra::signals::{WriteEvidence, WriteOp, emit_for};

use super::channels::SyncFetchChannels;
use super::channels::do_incremental_sync_channels;
use super::history::{STARTUP_DELAY, WINDOW_POLL_INTERVAL};
use super::incremental::beijing_today;
use super::model::WriteWitness;
use super::progress::ProgressEmitter;
use super::session::FacadeWriteSession;

/// 后台每日现价刷新的通道注入接缝（先例：命令壳 `SyncChannelsSlot` / 后台补全
/// `BackfillChannelsSlot`）：生产**不管理**本状态（每轮建后台车道生产束），
/// 集成测试 manage 本状态并装入门控桩束，使「每日刷新真实在途」可确定复现。
/// 异步互斥体：通道束引用在 `.await` 之间存活（issue #1412 通道束闭包 async 化）。
pub struct DailyPriceRefreshChannelsSlot(pub Arc<tokio::sync::Mutex<SyncFetchChannels>>);

/// 调度时机的显式参数（启动延迟与自然日窗口巡检周期）：生产走默认值（与价格
/// 历史后台补全同一节奏），接线型集成测试注入短时机——断言只等窗口到达，不被
/// 生产常数拖慢（先例：[`super::history::BackfillTimings`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyPriceRefreshTimings {
    /// 应用启动后到首轮刷新的延迟。
    pub startup_delay: Duration,
    /// 自然日窗口的巡检周期（跨北京日历日即跑当天窗口）。
    pub window_poll: Duration,
}

impl Default for DailyPriceRefreshTimings {
    fn default() -> Self {
        Self {
            startup_delay: STARTUP_DELAY,
            window_poll: WINDOW_POLL_INTERVAL,
        }
    }
}

/// 后台每日现价刷新的启动入口（壳层后台服务编排单点调用，issue #961 名单）：
/// 进程级单次拉起（原位重引导重复调用幂等，ADR-0080），spawned 线程自持
/// 「启动延迟补跑一次 + 每自然日窗口一次」的巡检循环。
pub fn start_daily_price_refresh<R: Runtime>(app: &AppHandle<R>) {
    start_daily_price_refresh_with(app, DailyPriceRefreshTimings::default());
}

/// 同 [`start_daily_price_refresh`]，调度时机可注入（测试短时机先例
/// `start_history_backfill_with`）。
pub fn start_daily_price_refresh_with<R: Runtime>(
    app: &AppHandle<R>,
    timings: DailyPriceRefreshTimings,
) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }
    let gate = EncryptionGate::clone(&app.state::<EncryptionGate>());
    let boot_gate = BootFailureGate::clone(&app.state::<BootFailureGate>());
    let handle = app.clone();
    std::thread::spawn(move || {
        // 启动延迟：让出启动期再补跑当天首轮；应用退出即线程随进程硬停，
        // 无需优雅关闭（写入全是幂等 upsert，中断无残留）。
        std::thread::sleep(timings.startup_delay);
        let mut last_round_date: Option<chrono::NaiveDate> = None;
        loop {
            // 每轮门检（先例：自动备份调度，issue #644 / ADR-0080）：锁定/
            // 启动失败期间不触碰占位连接；门开后首个到达的自然日窗口即补跑
            // ——启动轮即当天的窗口。
            if !gate.is_locked() && !boot_gate.is_failed() {
                let today = beijing_today();
                if last_round_date != Some(today) {
                    last_round_date = Some(today);
                    run_daily_refresh_round(&handle);
                }
            }
            std::thread::sleep(timings.window_poll);
        }
    });
}

/// 跑一轮每日现价刷新（与手动同步同形的单轮编排）：通道束换装（测试桩束优先
///（[`DailyPriceRefreshChannelsSlot`] 管理态），生产建后台车道束——同一全局限
/// 速器、前台在途时让行）+ 门面写槽裸作业会话 + 手动形态同款进度事件 + 写入
/// 见证。失败静默等下一窗口（无用户可报）；实际写入的收尾裁决与手动同步同判
/// ——置脏一次 + 发既有价格失效信号，零写入不置脏不广播。async 编排经全局
/// 运行时在调度线程上驱动到完成（issue #1412）。
fn run_daily_refresh_round<R: Runtime>(app: &AppHandle<R>) {
    let write = app.state::<DbState>().write_handle();
    let slot = app
        .try_state::<DailyPriceRefreshChannelsSlot>()
        .map(|s| s.0.clone());
    let (result, any_written) = match slot {
        Some(arc) => tauri::async_runtime::block_on(run_round_with_channels(app, &write, &arc)),
        None => match SyncFetchChannels::production_backfill() {
            Ok(channels) => {
                let channels = tokio::sync::Mutex::new(channels);
                tauri::async_runtime::block_on(run_round_with_channels(app, &write, &channels))
            }
            Err(error) => (Err(error), false),
        },
    };

    // 收尾裁决（issue #1277 成败同判，与手动同步同形）：本轮实际写过价格或
    // 名称 → 提交点置脏一次 + 发既有价格失效信号；零写入不置脏不广播。
    // 置脏失败记日志不静默吞运行结果（脏标记待下次写入补上）。
    if any_written {
        if let Err(error) = write.run_blocking("daily_price_refresh", |_| Ok(())) {
            tracing::warn!(%error, "每日现价刷新收尾置脏失败（脏标记待下次写入补上）");
        }
        emit_for(
            app,
            WriteOp::SyncInstrumentInfo,
            WriteEvidence::PriceWritten(true),
        );
    }
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
        Err(error) => tracing::warn!(%error, "后台每日现价刷新失败（静默等下一窗口）"),
    }
}

/// 持束跑一轮：会话（门面写槽裸作业会话）+ 手动形态同款进度事件 + 写入见证。
/// 返回 (编排结果, 是否实际写过)——见证跨失败存活（成败同判的证据源）。
async fn run_round_with_channels<R: Runtime>(
    app: &AppHandle<R>,
    write: &DbWriteHandle,
    channels: &tokio::sync::Mutex<SyncFetchChannels>,
) -> (Result<super::model::SyncInstrumentInfoResult>, bool) {
    let mut witness = WriteWitness::default();
    let session = FacadeWriteSession::new(write.clone(), "daily_price_refresh");
    // 进度事件与手动同步同形（同一事件名）：前端接缝只在手动同步在途时消费
    // 事件，后台推进不点亮进度条（见模块头注释）。
    let emitter = app.clone();
    let mut progress = move |progress| ProgressEmitter::emit_progress(&emitter, progress);
    let mut channels = channels.lock().await;
    let result =
        do_incremental_sync_channels(&session, &mut channels, &mut progress, &mut witness).await;
    let any_written = witness.any_written();
    (result, any_written)
}
