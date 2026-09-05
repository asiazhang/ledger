//! 启动失败恢复命令壳层（issue #601 / ADR-0075 决策 5 修订）。
//!
//! 启动期数据库打不开（明文库损坏等）不再弹原生「重置/退出」对话框、不再退出：
//! 启动状态经 [`get_boot_status`] 暴露给前端（前端启动首屏选择的唯一依据），
//! 失败时由启动失败恢复屏承担恢复通道——首版通道为 [`reset_after_startup_failure`]
//! 「重置为空库」（旧库按既有重置命名语义保留 `.bak` 副本，见
//! [`crate::db::reset_db_file`]），成功后原位换连、拉起自动备份调度，应用随即
//! 进入全新空账本，无需重启。明文模式日常启动零改动。
//!
//! 只做参数解包与状态编排：库文件处置判定与启动失败门在 db 基础设施
//! （[`crate::db::boot`]），重置的文件级语义在 [`crate::db`]，本文件不含领域规则。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::commands::data_location::effective_db_dir_of;
use crate::commands::encryption::resume_business_surface;
use crate::db::boot::{BOOT_DB_UNREADABLE, BootFailureGate};
use crate::db::encryption::EncryptionGate;
use crate::db::{reset_db_file, run_db};
use crate::error::{AppError, Result};

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
#[tauri::command]
pub fn get_boot_status(app: AppHandle) -> Result<BootStatus> {
    let failed = app.state::<BootFailureGate>().is_failed();
    let locked = app.state::<EncryptionGate>().is_locked();
    let status = if failed {
        BootStatus {
            phase: "failed",
            error_code: Some(BOOT_DB_UNREADABLE.to_string()),
        }
    } else if locked {
        BootStatus {
            phase: "locked",
            error_code: None,
        }
    } else {
        BootStatus {
            phase: "ready",
            error_code: None,
        }
    };
    Ok(status)
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
