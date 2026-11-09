//! 汇率每日自动增量同步的后台车道（ADR-0019 修订记录 / issue #1546）：汇率
//! 采集的第二种触发形态——应用启动后延迟补跑一次，此后每个自然日窗口各跑
//! 一次同步。
//!
//! 与手动形态（[`super::fx`] 经壳层 `sync_exchange_rates` 命令，issue #1545）
//! **同一编排、同一取数面**：消费同一深度判据与同步编排
//!（[`super::fx::sync_fx_rates`]——深度未达走全量回填、已达走 90 天增量，
//! 分派由账本判据决定、与触发面无关）、同一落库单元（单一事务、整周覆盖
//! 幂等、人工行保护）。与手动形态的差异只有两处：
//!
//! - **触发**：系统自动（本模块调度任务）。多台设备各自运行，无单机开关、
//!   无领导者选举（与既有行情后台车道同形，spec #1540）；「每自然日至多一次」
//!   由自然日窗口判定保证（[`super::incremental::daily_window_opens`]），
//!   重复调用幂等由落库单元的整周覆盖 upsert 保证；
//! - **失败面**：取数失败记 warn 日志可见（无用户在场可报，先例：两条行情
//!   后台车道），静默等下一自然日窗口；取数发生在落库之前，失败即零写入
//!   ——已有汇率不受影响，记账照常可继续，即时补救走设置页手动入口。
//!
//! 本车道不经后台车道单轮骨架（[`super::lane`]）：骨架承载写入见证与收尾
//! 置脏 / 价格失效信号（价格数据的裁决面），而 ECB 汇率拉取是**可重建缓存**
//! ——不产同步 op、不置脏、不发失效信号（ADR-0019 修订记录，与手动入口
//! 同一裁决），本模块自带「换装 → 会话 → 编排 → 日志」的最小轮次。
//!
//! 调度与启动接线与两条行情后台车道共用域内单点：进程级单次拉起守卫（原位
//! 重引导重复调用幂等，ADR-0080）、每轮门检（锁定 / 启动失败期间不触碰占位
//! 连接）、自然日窗口比对北京日历日，循环收口在 [`super::lane`] 内核（issue
//! #1622 收敛；宿主启动入口 [`start_daily_fx_sync_for_host`]，issue #2079，
//! 旧壳 tauri 门内入口随旧壳退役，#2083）；启动接线收进壳层启动单点
//!（NSH 住 `run_startup` 成功分支）。挂异步运行时的 async 任务（ADR-0125
//! 决策 7 / issue #1413；执行器经 [`super::lane::LaneHost`] 供给）。

use std::sync::atomic::AtomicBool;

use ledger_infra::db::facade_handles::DbWriteHandle;
use ledger_infra::error::Result;

use super::fx::{FX_SYNC_GATE, FxSyncChannels, FxSyncReport, sync_fx_rates_guarded};
use super::lane::{LaneHost, LaneTimings, start_daily_lane_kernel};
use super::session::FacadeWriteSession;

/// 后台每日汇率同步的启动入口（issue #2079 注册点）：NSH 壳经公开宿主面
/// 接线——门检 / 执行器 / 会话解析 / 通道束注入槽由宿主束
///（[`super::lane::LaneHost`]）装配，巡检循环同走内核；旧壳 AppHandle 启动
/// 入口随旧壳退役（#2083），本入口是唯一入口。本车道可重建缓存
///（无见证 / 无置脏 / 无失效信号），宿主束的进度 / 广播字段本车道不消费。
pub fn start_daily_fx_sync_for_host(host: &LaneHost, timings: LaneTimings) {
    static SPAWNED: AtomicBool = AtomicBool::new(false);
    let host = host.clone();
    start_daily_lane_kernel(
        &SPAWNED,
        timings,
        host.ready.clone(),
        host.spawn.clone(),
        move || {
            let host = host.clone();
            Box::pin(run_daily_fx_sync_round_for_host(host))
        },
    );
}

/// 跑一轮每日汇率同步（非门宿主面，issue #2079）：换装（宿主注入槽优先、
/// 生产通道束兑底）→ 门面写槽裸作业会话 → [`sync_fx_rates`] 编排 → 统计 /
/// 失败日志——与门内 [`run_daily_fx_sync_round`] 同形。会话缺失轮次空转。
async fn run_daily_fx_sync_round_for_host(host: LaneHost) {
    let Some(write) = (host.session)() else {
        tracing::debug!("账本会话未就位，本轮空转");
        return;
    };
    let result = match (host.fx_channels)() {
        Some(shared) => {
            let mut channels = shared.lock().await;
            drive_round(&write, &mut channels).await
        }
        None => match FxSyncChannels::production() {
            Ok(mut channels) => drive_round(&write, &mut channels).await,
            // 建生产束失败按本轮失败处置（取数未发生即零写入，与编排失败同路）。
            Err(error) => Err(error),
        },
    };
    match result {
        // 零痕迹跳过与「已是最新」都落在这里：无用户在场，低声记一笔即可。
        Ok(report) if report.persist.points == 0 => {
            tracing::debug!("每日汇率同步完成：零落库点（零痕迹跳过或数据已是最新）");
        }
        Ok(report) => tracing::info!(
            full_backfilled = report.full_backfilled,
            pairs = report.persist.pairs,
            points = report.persist.points,
            earliest = report.persist.earliest.as_deref().unwrap_or("-"),
            manual_protected = report.persist.manual_protected,
            "后台每日汇率同步完成"
        ),
        // 失败可见（issue #1546 AC3）：记 warn 供检索，不静默冒充成功；已有
        // 汇率不受影响（取数在落库之前，失败即零写入），重试等下一自然日窗口。
        Err(error) => {
            tracing::warn!(%error, "后台每日汇率同步失败（已有汇率不受影响，静默等下一自然日窗口）")
        }
    }
}

/// 一轮的会话接线：门面写槽裸作业会话交给汇率同步编排（与手动入口同款会话
/// 形态；网络等待在会话之外以 await 表达，慢闭包纪律）。写槽句柄由调用方
/// 解析（宿主面自宿主束，issue #2079），两条触发面共用本单点。
async fn drive_round(write: &DbWriteHandle, channels: &mut FxSyncChannels) -> Result<FxSyncReport> {
    let session = FacadeWriteSession::new(write.clone(), "daily_fx_sync");
    // 在途互斥（issue #1762）：与手动入口共用进程级单例门；撞车时本轮即以
    // 码化错误收尾（记 warn 日志可见，静默等下一自然日窗口）。每日路径不发阶段
    // 事件（无 UI 消费）。
    sync_fx_rates_guarded(&FX_SYNC_GATE, &session, channels, &mut |_| {}).await
}
