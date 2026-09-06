//! 启动与重启命令壳层（issue #601 / #602 / #644 / ADR-0075 决策 5 修订 / ADR-0080）。
//!
//! 启动期数据库打不开（明文库损坏等）不再弹原生「重置/退出」对话框、不再退出：
//! 启动状态经 [`get_boot_status`] 暴露给前端（前端启动首屏选择的唯一依据），
//! 失败时由启动失败恢复屏承担恢复通道——「重置为空库」（[`reset_after_startup_failure`]，
//! 旧库按既有重置命名语义保留 `.bak` 副本，见 [`crate::db::reset_db_file`]，成功后
//! 原位换连、拉起自动备份调度，应用随即进入全新空账本，无需重启）与「从备份文件
//! 恢复…」（issue #602：复用既有 [`crate::commands::backup::restore_backup`] 全语义，
//! 恢复成功后前端经 `restart_app` 自动重启；本文件只扩失败门白名单，恢复命令不变）。
//! 明文模式日常启动零改动。
//!
//! 进程启动与重启共用同一段引导序列（[`boot_sequence`]，内核是 db 层 [`crate::db::boot::plan_boot`]）：
//! 重启命令（[`restart_app`]，原位重引导）不再重建进程，而是原地重跑启动引导，
//! 保证「重启后状态 = 新进程启动状态」恒成立（ADR-0080）。
//!
//! 只做参数解包与状态编排：库文件处置判定与启动失败门在 db 基础设施
//! （[`crate::db::boot`]），重置的文件级语义在 [`crate::db`]，本文件不含领域规则。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use std::sync::RwLock;

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::backup;
use crate::commands::data_location::{default_data_dir, effective_db_dir_of};
use crate::commands::encryption::resume_business_surface;
use crate::db::boot::{BOOT_DB_UNREADABLE, BootFailureGate, BootPlan};
use crate::db::data_location::Boot;
use crate::db::encryption::EncryptionGate;
use crate::db::{DbState, open_connection_in, reset_db_file, run_db};
use crate::error::{AppError, Result};

/// 引导结果的托管形态（issue #644 / ADR-0080）：`RwLock` 包裹——原位重引导
/// 需要整体换入新引导结果，消费方只读克隆。登记/读取收口本模块的
/// [`register_boot`] / [`current_boot`] 两个接缝。
pub type BootCell = RwLock<Boot>;

/// 登记引导结果：首次管理或原位整体换入（重引导路径）。
fn register_boot(app: &AppHandle, boot: Boot) {
    match app.try_state::<BootCell>() {
        Some(cell) => {
            let mut guard = cell.write().unwrap_or_else(|e| e.into_inner());
            *guard = boot;
        }
        None => {
            app.manage(BootCell::new(boot));
        }
    }
}

/// 读取当前引导结果（克隆快照）；未登记（极端时序）返回 `None`，消费方按
/// 各自的兜底语义处理（与旧 `try_state::<Boot>()` 同形）。
pub(crate) fn current_boot(app: &AppHandle) -> Option<Boot> {
    app.try_state::<BootCell>()
        .map(|cell| cell.read().unwrap_or_else(|e| e.into_inner()).clone())
}

/// 启动相位（引导序列的去向，启动与原位重引导共用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootPhase {
    /// 明文库/空文件已建连，业务可用（主界面）。
    Ready,
    /// 密文库等待解锁（占位连接维持形状，解锁屏）。
    AwaitUnlock,
    /// 引导失败（占位连接维持形状，失败恢复屏）。
    Failed,
}

impl BootPhase {
    /// 相位词表的单一来源（wire 形态与日志同名）：[`BootStatus::phase`]、
    /// 日志相位字段都经此处产出，不得平行手拼同义字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            BootPhase::Ready => "ready",
            BootPhase::AwaitUnlock => "locked",
            BootPhase::Failed => "failed",
        }
    }
}

/// 把裸连接换入既有 `DbState`（原位重引导路径；Arc 共享，HTTP 壳/调度线程
/// 持有的克隆同步可见）或首次登记为应用状态（进程启动路径，[`DbState`]
/// 形状自此恒在）。
fn swap_or_manage_db_state(app: &AppHandle, conn: Connection) -> Result<()> {
    match app.try_state::<DbState>() {
        Some(existing) => {
            let mut guard = existing
                .conn
                .lock()
                .map_err(|e| AppError::Db(e.to_string()))?;
            *guard = conn;
        }
        None => {
            app.manage(DbState {
                conn: std::sync::Arc::new(std::sync::Mutex::new(conn)),
            });
        }
    }
    Ok(())
}

/// 占位内存连接（锁定/启动失败期间维持 [`DbState`] 形状；门禁拦截业务 IPC，
/// 占位连接不被触达；恢复/解锁路径成功后原位换入真实连接）。
fn placeholder_db(app: &AppHandle) -> Result<()> {
    let conn = crate::db::open_in_memory()?;
    swap_or_manage_db_state(app, conn)
}

/// 启动引导序列（issue #570 / #601 / #644 / ADR-0080）：DataLocation 引导 →
/// 生效库文件处置判定 → 按处置分派（占位/真实连接 + 两扇门翻转）。进程启动
/// （`lib.rs` setup）与重启命令（[`restart_app`] 原位重引导）消费同一函数，
/// 唯一差别是应用状态首次登记还是原位换入，序列本体零分叉。
///
/// 门翻转次序 fail-closed：进入锁定/失败先把门立起再换连接（换连窗口内业务
/// IPC 已被拦截）；回到就绪先换入真实连接再开门（开门前窗口内业务 IPC 同样
/// 被拦截）——两向都不会把业务读写暴露给占位/旧连接。
pub(crate) fn boot_sequence(app: &AppHandle) -> Result<BootPhase> {
    let default_dir = default_data_dir(app)?;
    std::fs::create_dir_all(&default_dir).map_err(|e| AppError::Io(e.to_string()))?;
    let BootPlan { boot, disposition } = crate::db::boot::plan_boot(&default_dir);
    if let Some(reason) = &boot.fallback_reason {
        tracing::warn!(reason = %reason, "DataLocation 引导发生回退，已改用默认数据目录");
    }
    let db_dir = boot.db_dir.clone();
    register_boot(app, boot);
    let gate = app.state::<EncryptionGate>();
    let boot_gate = app.state::<BootFailureGate>();
    match disposition? {
        crate::db::boot::BootDisposition::AwaitUnlock => {
            // 占位连接只维持 DbState 形状（IPC/HTTP 壳在锁定期间被门禁拦截，
            // 不会触达）；解锁成功后原位换成凭主口令打开的真实连接。
            gate.set_locked(true);
            placeholder_db(app)?;
            tracing::info!(db_dir = %db_dir.display(), "检测到密文库，等待解锁");
            Ok(BootPhase::AwaitUnlock)
        }
        crate::db::boot::BootDisposition::OpenPlaintext => {
            let conn = open_connection_in(&db_dir)?;
            swap_or_manage_db_state(app, conn)?;
            gate.set_locked(false);
            boot_gate.clear();
            // 日志等级接管（spec #608 / #611）：数据库就绪后按持久化档位 reload 一次
            //（此前短暂按「RUST_LOG 环境变量或默认 info」运行，属 ADR-0006 接受的启动窗口）；
            // 显式 RUST_LOG 在本次启动内优先级最高，此时不覆盖。密文库为占位连接，
            // 此步随解锁换连后（`resume_business_surface`）再做。
            {
                let state = app.state::<DbState>();
                let conn = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
                crate::logger::apply_persisted_level(&conn);
            }
            tracing::info!(db_dir = %db_dir.display(), "数据库初始化完成");
            Ok(BootPhase::Ready)
        }
        crate::db::boot::BootDisposition::Unreadable => {
            // 明文损坏主场景（issue #601）：库文件不可读，按启动失败处理——
            // 由调用方登记失败门、交由前端失败恢复屏接管。
            Err(AppError::coded(
                BOOT_DB_UNREADABLE,
                "数据库文件无法打开（文件可能已损坏）",
            ))
        }
    }
}

/// 引导失败登记（启动与重引导共用的失败路径，issue #601）：登记失败门、
/// 占位连接维持形状，应用存活，由前端失败恢复屏接管。`Err` 仅在占位内存
/// 库也建不起（登记本身失败）时上抛：启动路径 fail loud 退出（run() 二次
/// 失败兑底），重引导路径留痕后保持失败态。
pub(crate) fn recover_boot_failure(app: &AppHandle, error: &AppError) -> Result<()> {
    tracing::error!(error = %error, "数据库初始化失败，登记启动失败状态，交由前端失败恢复屏接管");
    app.state::<BootFailureGate>().set_failed();
    placeholder_db(app)
}

/// 引导序列的失败容忍形态（issue #644）：失败不上抛，登记失败态后返回
/// `Failed` 相位——重引导失败不能把用户留在旧界面（旧连接已换出），必须
/// 重载进失败恢复屏；启动路径需要区分「序列内失败」与「登记也失败」，
/// 仍直接消费 [`boot_sequence`] 的 `Result`。
pub(crate) fn try_boot_sequence(app: &AppHandle) -> BootPhase {
    match boot_sequence(app) {
        Ok(phase) => phase,
        Err(e) => {
            if let Err(recover_err) = recover_boot_failure(app, &e) {
                tracing::error!(error = %recover_err, "占位内存库登记失败，保持启动失败态");
            }
            BootPhase::Failed
        }
    }
}

/// 启动状态（前端启动首屏选择的唯一依据）。
#[derive(Debug, Serialize)]
pub struct BootStatus {
    /// 启动相位（闭集）：`ready`（明文库/已解锁，挂主界面）、`locked`
    /// （密文库等待解锁，挂解锁屏）、`failed`（启动失败，挂失败恢复屏）。
    pub phase: &'static str,
    /// 失败时的稳定错误码（前端按码本地化失败恢复屏文案）；非 failed 为 `None`。
    pub error_code: Option<String>,
}

/// 查询启动状态（issue #601）：前端启动探测的唯一入口，一次拿到
/// 「主界面 / 解锁屏 / 失败恢复屏」三态选择。纯进程状态读取、无副作用，
/// 不经数据库，无需阻塞线程池（先例：`get_remember_passphrase_support`）。
/// 每次探测即「WebView 已加载」的日志信号（issue #644）：原位重引导后
/// 本日志出现 = 前端重载成功；缺失 = WebView 未加载（白屏根因二的可观测面）。
#[tauri::command]
pub fn get_boot_status(app: AppHandle) -> Result<BootStatus> {
    let failed = app.state::<BootFailureGate>().is_failed();
    let locked = app.state::<EncryptionGate>().is_locked();
    // 相位词表单一来源（审查：与 [`BootPhase::as_str`] 共用闭集，不再平行手拼）。
    let phase = if failed {
        BootPhase::Failed
    } else if locked {
        BootPhase::AwaitUnlock
    } else {
        BootPhase::Ready
    };
    let status = BootStatus {
        phase: phase.as_str(),
        error_code: (phase == BootPhase::Failed).then(|| BOOT_DB_UNREADABLE.to_string()),
    };
    tracing::info!(phase = status.phase, "前端启动探测完成（WebView 已加载）");
    Ok(status)
}

/// 重启应用（issue #644 / ADR-0080）：**原位重引导**——进程不退出，重跑
/// 启动引导序列（DataLocation 解析 → 生效库文件判定 → 连接换入 → 两扇门
/// 翻转），返回后由前端重载 WebView、重新探测启动相位，落到解锁屏/失败
/// 恢复屏/主界面。
///
/// 取代 `app.restart()` 进程重启（Restore/转换/搬迁共用入口，语义不变、
/// 机制升级）：
/// - 进程重启在 `tauri dev` 下与 CLI 的 dev server 生命周期相克：老进程
///   退出即被 CLI 回收 dev server，新进程拉起时 devUrl 已不可达 → 空白
///   窗口（issue #644 白屏根因二）；
/// - macOS 上 `restart()` 重拉二进制越过 `RunEvent::Exit` 的兑现有竞态
///   （tauri#12310），窗口状态与激活行为也不可控。
/// 原位重引导无进程边界：开发与签名构建行为一致；引导序列与进程启动是
/// 同一段代码，「重启后状态 = 新进程启动状态」恒成立。退出兜底备份
/// （`RunEvent::Exit` 上的 `exit_fallback`）不参与本路径：转换/恢复各自
/// 有副本语义（旧库 .bak 副本 / RestoreSafetyBackup），不靠退出兜底。
///
/// 阻塞 IO（目录解析、文件头探测、建连）经 [`run_db`] 进阻塞线程池
/// （形状乙，spec #498/#503 先例）；白名单保持既有放行（锁定/失败期间
/// 恢复通道可达）。
#[tauri::command]
pub async fn restart_app(app: AppHandle) -> Result<()> {
    tracing::info!("应用重启开始：原位重引导（进程不退出），完成后由前端重载 WebView");
    let handle = app.clone();
    let phase = run_db("restart_app", move || Ok(try_boot_sequence(&handle))).await?;
    if phase == BootPhase::Ready {
        // 重引导落到就绪即拉起调度（幂等，单次拉起守卫）：锁定/失败态启动的
        // 会话里 setup 未拉起调度线程，恢复通道重启不经 setup——此处是该场景
        // 下调度线程唯一的生产点（ADR-0080 决策 4：解锁/恢复后自动继续）。
        // 未就绪（解锁屏/失败恢复屏）不拉：解锁/重置路径的
        // `resume_business_surface` 会在业务可用起点拉起。
        backup::start_scheduler(&app);
    }
    tracing::info!(
        phase = phase.as_str(),
        "原位重引导完成，等待前端重载 WebView"
    );
    Ok(())
}

/// 启动失败恢复通道①：重置为空库（issue #601 / ADR-0075 决策 5 修订）。
///
/// 只在启动失败状态可达（失败恢复屏专用面）。旧库按既有重置命名语义保留
/// `.bak` 副本（[`crate::db::reset_db_file`]），原位新建明文空库；成功后
/// 业务可用起点编排（与解锁恢复同型）：原位换连 → 清失败门 → 日志档位
/// 接管 → 拉起自动备份调度，应用随即进入全新空账本，无需重启。
#[tauri::command]
pub async fn reset_after_startup_failure(app: AppHandle) -> Result<()> {
    let gate = app.state::<BootFailureGate>();
    if !gate.is_failed() {
        return Err(AppError::coded(
            "boot.not-failed",
            "应用未处于启动失败状态，无需重置",
        ));
    }
    let db_dir = effective_db_dir_of(&app)?;
    let conn = run_db("reset_after_startup_failure", move || {
        reset_db_file(&db_dir)
    })
    .await?;
    // 业务可用起点编排与解锁恢复同型（原位换连 → 日志档位接管 → 拉起调度），
    // 锁定门翻转为无操作；此处再清启动失败门，业务 IPC 随即放行。
    resume_business_surface(&app, conn)?;
    app.state::<BootFailureGate>().clear();
    tracing::info!("启动失败重置完成：旧库保留 .bak 副本，应用以全新明文空库进入");
    Ok(())
}
