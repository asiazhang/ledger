//! 加密模式命令壳层（issue #570 / #571 / #574 / ADR-0075）：状态查询、解锁、
//! 开启加密、关闭加密、修改主口令、忘记口令重置（issue #573）、本机记住主口令
//! （钥匙串缓存 + macOS 生物认证门，issue #574）。
//!
//! 只做参数解包与 [`crate::db::encryption`] / [`crate::db::data_location`] /
//! [`crate::db::passphrase_cache`] 调用与状态编排：文件级转换（三形态同机制）、
//! 解锁建连、搬迁补做、重置副本语义、钥匙串口令缓存都在 db 基础设施，
//! 本文件不含领域规则。「本机记住」的偏好开关是前端 localStorage 轻量设置项
//! （不落库、不随备份迁移），钥匙串缓存内容为主口令本身（密钥仍由口令派生）。
//!
//! 全部命令 async 化（形状乙，spec #498 / #503 先例）：转换导出、解锁建连、
//! 搬迁补做与钥匙串读写（含生物认证阻塞）是阻塞 IO，经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use rusqlite::Connection;
use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::backup;
use crate::db::DbState;
use crate::db::data_location::{self, Boot, DB_FILE_NAME};
use crate::db::encryption::{self, DbFileKind, EncryptionGate};
use crate::db::passphrase_cache::{self, CacheLoad, RememberMode};
use crate::db::run_db;
use crate::error::{AppError, Result};

/// 加密状态（设置页加密卡片与启动解锁屏的消费形状）。
#[derive(Debug, Serialize)]
pub struct EncryptionStatus {
    /// 进程是否处于锁定（等待解锁）状态：密文库已探测、业务读写不可用。
    pub locked: bool,
    /// 库文件当前是否为密文库（文件即真相，ADR-0075 决策 4）。
    pub file_encrypted: bool,
}

/// 解锁结果：`relocated` 表示解锁后补做了等待中的搬迁（目标库已就绪），
/// 前端据此触发应用重启，由下次启动引导接管目标位置。
#[derive(Debug, Serialize)]
pub struct UnlockOutcome {
    pub relocated: bool,
}

/// 本机记住主口令的平台能力与运行形态（issue #574 / #662 / ADR-0075 决策 3）：
/// 前端据 `supported` 决定是否展示「记住」选项（v1 仅 macOS 支持；否则隐藏选项、
/// 回退手输），据 `mode` 区分「平台不支持」与「开发构建回退」并给出可预期提示。
#[derive(Debug, Clone, Copy, Serialize)]
pub struct RememberPassphraseSupport {
    /// 平台是否支持本机缓存主口令（v1 仅 macOS；不支持时前端隐藏选项、回退手输）。
    pub supported: bool,
    /// 运行形态（issue #662）：`biometry` = 发布构建，条目带 Touch ID 生物认证门；
    /// `dev-fallback` = 开发/未签名构建的无门缓存回退（读取不弹生物认证）。
    pub mode: RememberMode,
}

/// 生效目录中的库文件路径（引导结果登记的单一来源）。
fn active_db_path(app: &AppHandle) -> std::path::PathBuf {
    app.state::<Boot>().db_dir.join(DB_FILE_NAME)
}

/// 转换类命令（开启/关闭/修改主口令）的共同门禁：应用处于锁定状态时
/// 拒绝——转换只能在解锁后的运行中应用发起。
fn ensure_unlocked(app: &AppHandle) -> Result<()> {
    if app.state::<EncryptionGate>().is_locked() {
        return Err(AppError::coded(
            "encryption.locked",
            "应用已锁定，请先解锁后再操作",
        ));
    }
    Ok(())
}

/// 查询加密状态：锁定门状态 + 库文件头探测。
#[tauri::command]
pub async fn get_encryption_status(app: AppHandle) -> Result<EncryptionStatus> {
    let locked = app.state::<EncryptionGate>().is_locked();
    let db_path = active_db_path(&app);
    run_db("get_encryption_status", move || {
        let file_encrypted = encryption::probe_file_kind(&db_path)? == DbFileKind::Encrypted;
        Ok(EncryptionStatus {
            locked,
            file_encrypted,
        })
    })
    .await
}

/// 解锁核心（手输 [`unlock_encryption`] 与凭缓存 [`unlock_with_remembered_passphrase`]
/// 共用，issue #574）：凭主口令打开密文库并原位换连（HTTP 壳与调度线程已持有的
/// 连接 Arc 克隆全部可见），翻转锁定门、拉起自动备份调度；启动期等待中的搬迁
/// （源库为密文库）在解锁后以主口令补做，成功即触发重启语义。
async fn do_unlock(app: &AppHandle, passphrase: &str) -> Result<UnlockOutcome> {
    let db_path = active_db_path(app);
    let pass = passphrase.to_string();
    let conn = run_db("unlock_encryption_core", move || {
        encryption::unlock_db_file(&db_path, &pass)
    })
    .await?;
    resume_business_surface(app, conn)?;

    // 等待中的搬迁（issue #570）：源库为密文库时启动期无法搬迁，解锁后
    // 以主口令补做。失败不阻断解锁：应用继续以当前位置运行，意图保持
    // 待重启状态（下次解锁重试）。
    let mut relocated = false;
    let pending = app.state::<Boot>().deferred_relocation.clone();
    if let Some(target_dir) = pending {
        let source = active_db_path(app);
        let target = target_dir.join(DB_FILE_NAME);
        let pass = passphrase.to_string();
        let outcome = run_db("unlock_deferred_relocation", move || {
            data_location::relocate_with_key(&source, &target, &pass)
        })
        .await;
        match outcome {
            Ok(()) => {
                relocated = true;
                tracing::info!(target = %target_dir.display(), "解锁后搬迁完成，待重启接管目标位置");
            }
            Err(e) => {
                tracing::warn!(error = %e, "解锁后搬迁失败，保持当前位置运行");
            }
        }
    }
    Ok(UnlockOutcome { relocated })
}

/// 解锁：凭主口令打开密文库并进入应用（见 [`do_unlock`]）。
#[tauri::command]
pub async fn unlock_encryption(app: AppHandle, passphrase: String) -> Result<UnlockOutcome> {
    let gate = app.state::<EncryptionGate>();
    if !gate.is_locked() {
        return Err(AppError::coded(
            "encryption.not-locked",
            "应用未处于锁定状态，无需解锁",
        ));
    }
    do_unlock(&app, &passphrase).await
}

/// 凭本机缓存的主口令解锁（issue #574 / ADR-0075 决策 3/5）：从系统钥匙串读取
/// 缓存的主口令（macOS 在此弹 Touch ID 生物认证门）→ 以之为凭据走 [`do_unlock`]。
/// 只在锁定状态可达；**口令不回流前端**（前端只调本命令、不拿口令）。
///
/// 失败回退（只损失便利，不损失数据）：
/// - 无缓存：`encryption.remember-no-cache`（前端静默回退手输）；
/// - 生物认证取消：`encryption.remember-biometric-cancelled`（前端回退手输）；
/// - 缓存口令已过期（错误口令，如恢复其它主口令的备份）：清掉缓存避免每次启动
///   都弹生物认证，并把错误上报（前端回退手输）。
#[tauri::command]
pub async fn unlock_with_remembered_passphrase(app: AppHandle) -> Result<UnlockOutcome> {
    let gate = app.state::<EncryptionGate>();
    if !gate.is_locked() {
        return Err(AppError::coded(
            "encryption.not-locked",
            "应用未处于锁定状态，无需解锁",
        ));
    }
    let cached = run_db("read_remembered_passphrase", passphrase_cache::load).await?;
    match cached {
        CacheLoad::Found(passphrase) => match do_unlock(&app, &passphrase).await {
            Ok(outcome) => Ok(outcome),
            Err(e) if e.is_code("encryption.passphrase-incorrect") => {
                // 缓存口令已过期：清缓存（下次仍回退手输），并把错误上报前端回退。
                tracing::warn!("本机缓存的主口令已过期，清理缓存以回退手输");
                let _ = run_db(
                    "clear_stale_remembered_passphrase",
                    passphrase_cache::delete,
                )
                .await;
                Err(e)
            }
            Err(e) => Err(e),
        },
        CacheLoad::NotFound => Err(AppError::coded(
            "encryption.remember-no-cache",
            "本机没有缓存的主口令，请手动输入",
        )),
        CacheLoad::Cancelled => Err(AppError::coded(
            "encryption.remember-biometric-cancelled",
            "生物认证已取消，请手动输入主口令",
        )),
    }
}

/// 业务可用起点编排（解锁与忘记口令重置共用，ADR-0075 决策 5）：新连接
/// 原位换入 DbState（Arc 形状不变，业务路径下次锁连接即取到真实库）→
/// 翻转锁定门 → 拉起自动备份调度（轮询同轮承载定时追补）。
fn resume_business_surface(app: &AppHandle, conn: Connection) -> Result<()> {
    {
        let state = app.state::<DbState>();
        let mut guard = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        *guard = conn;
    }
    app.state::<EncryptionGate>().set_locked(false);
    // 日志等级接管（spec #608 / #611）：解锁后真实库就绪，按持久化档位接管滤镜；
    // 显式 RUST_LOG 在本次启动内优先，此时不覆盖。锁定期未读到持久化档位，
    // 解锁是密文库的正式接管点（与启动期明文库的 `init_database` 对齐）。
    {
        let state = app.state::<DbState>();
        let conn = state.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        crate::logger::apply_persisted_level(&conn);
    }
    tracing::info!("业务读写恢复（解锁/重置后），自动备份调度拉起");
    backup::start_scheduler(app);
    Ok(())
}

/// 开启加密：把当前明文库整库一次性转换为密文库（转换与原子性语义见
/// [`encryption::enable_encryption_for_file`]）。完成后应用需重启以凭
/// 新口令重新打开；重启由前端经既有 `restart_app` 触发（与 Restore
/// 「恢复成功后自动重启」同型）。
#[tauri::command]
pub async fn enable_encryption(app: AppHandle, passphrase: String) -> Result<()> {
    ensure_unlocked(&app)?;
    let db_path = active_db_path(&app);
    run_db("enable_encryption", move || {
        encryption::enable_encryption_for_file(&db_path, &passphrase)
    })
    .await?;
    tracing::info!("整库加密转换完成，待重启以新口令重新打开");
    Ok(())
}

/// 关闭加密：把当前密文库整库一次性转换回明文库（需当前主口令，转换与
/// 原子性语义见 [`encryption::disable_encryption_for_file`]）。完成后
/// 重启由启动探测接管：明文库不再出现解锁屏；重启由前端经既有
/// `restart_app` 触发（Restore 同型）。
#[tauri::command]
pub async fn disable_encryption(app: AppHandle, passphrase: String) -> Result<()> {
    ensure_unlocked(&app)?;
    let db_path = active_db_path(&app);
    run_db("disable_encryption", move || {
        encryption::disable_encryption_for_file(&db_path, &passphrase)
    })
    .await?;
    // 关闭加密后主口令不再适用：清钥匙串缓存（幂等，失败不阻断转换），
    // 避免残留可自动解锁的旧口令（ADR-0075 决策 5）。
    let _ = run_db("clear_remember_after_disable", passphrase_cache::delete).await;
    tracing::info!("整库转换完成（关闭加密），待重启以明文重新打开");
    Ok(())
}

/// 修改主口令：旧口令验证通过后把密文库整库转入新口令的新库（转换与
/// 原子性语义见 [`encryption::change_passphrase_for_file`]）。完成后
/// 重启以新口令解锁；重启由前端经既有 `restart_app` 触发（Restore 同型）。
#[tauri::command]
pub async fn change_encryption_passphrase(
    app: AppHandle,
    passphrase: String,
    new_passphrase: String,
) -> Result<()> {
    ensure_unlocked(&app)?;
    let db_path = active_db_path(&app);
    run_db("change_encryption_passphrase", move || {
        encryption::change_passphrase_for_file(&db_path, &passphrase, &new_passphrase)
    })
    .await?;
    tracing::info!("整库转换完成（修改主口令），待重启以新口令重新打开");
    Ok(())
}

/// 忘记口令逃生门（issue #573 / ADR-0075 决策 2/5）：解锁屏可达的重置。
/// 旧密文库按既有重置命名语义保留为密文副本，原位新建明文空库（副本与
/// 新库语义见 [`encryption::reset_encrypted_db_file`]）。
///
/// 只在锁定状态可达（解锁屏专用面）；成功后经 [`resume_business_surface`]
/// 原位换连、翻 unlock、拉起自动备份调度——应用随即回到明文模式的业务
/// 可用状态，无需重启，可在设置页再次走开启加密流程。
#[tauri::command]
pub async fn reset_after_forgotten_passphrase(app: AppHandle) -> Result<()> {
    let gate = app.state::<EncryptionGate>();
    if !gate.is_locked() {
        return Err(AppError::coded(
            "encryption.not-locked",
            "应用未处于锁定状态，无需重置",
        ));
    }
    let db_path = active_db_path(&app);
    let conn = run_db("reset_after_forgotten_passphrase", move || {
        encryption::reset_encrypted_db_file(&db_path)
    })
    .await?;
    // 忘记口令重置：旧主口令不再适用，清钥匙串缓存（幂等，失败不阻断重置），
    // 不残留可自动解锁的旧口令（ADR-0075 决策 5）。
    let _ = run_db("clear_remember_after_reset", passphrase_cache::delete).await;
    resume_business_surface(&app, conn)?;
    tracing::info!("忘记口令重置完成，应用以全新明文空库回到明文模式");
    Ok(())
}

/// 查询「本机记住主口令」的平台能力与运行形态（issue #574 / #662）：locked
/// 状态下（解锁屏）也要可调——解锁屏据 `supported` 决定是否尝试自动解锁。
/// 纯能力查询、无副作用、不经数据库，无需阻塞线程池。
#[tauri::command]
pub async fn get_remember_passphrase_support() -> Result<RememberPassphraseSupport> {
    Ok(RememberPassphraseSupport {
        supported: passphrase_cache::supported(),
        mode: passphrase_cache::current_mode(),
    })
}

/// 把主口令本身缓存进系统钥匙串（issue #574 / ADR-0075 决策 3；macOS 以
/// Touch ID 生物认证门保护）。只在解锁后可达；「记住」偏好开关由前端
/// localStorage 轻量设置项持有，本命令只落钥匙串缓存。
#[tauri::command]
pub async fn set_remember_passphrase(app: AppHandle, passphrase: String) -> Result<()> {
    ensure_unlocked(&app)?;
    run_db("set_remember_passphrase", move || {
        passphrase_cache::store(&passphrase)
    })
    .await
}

/// 清除「本机记住」的主口令缓存（issue #574）：关闭「记住」、关闭加密或忘记口令
/// 重置后调用，使钥匙串不再持有可自动解锁的口令。只在解锁后可达；幂等（无缓存
/// 也成功）。
#[tauri::command]
pub async fn clear_remember_passphrase(app: AppHandle) -> Result<()> {
    ensure_unlocked(&app)?;
    run_db("clear_remember_passphrase", passphrase_cache::delete).await
}
