//! DataLocation 基础设施（issue #132 / #133 / ADR-0018）。
//!
//! 收口「读指针 → 三分支定位/搬迁」引导：以"默认应用数据目录 + 可选指针"为输入，
//! 返回最终生效的库文件目录。#408 起同时承载更改意图三步校验
//!（[`validate_and_commit`]）与信息聚合（[`gather_info`]），自壳层下沉至此。
//! 纯 Rust、不依赖 Tauri runtime，建连前的唯一 DataLocation 权威。
//! 术语见 CONTEXT.md 的 DataLocation / Relocation 条目。
//!
//! 回退原则：指针损坏、目标不可用等一切引导期失败都回退默认目录并通过
//! [`Boot::fallback_reason`] 告知调用方（供界面显著提示）；绝不删除或修改
//! 任何既有文件，搬迁完成后旧位置的库永久保留。

use std::path::{Path, PathBuf};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::encryption::{DbFileKind, probe_file_kind};
use super::{check_integrity, open_connection, open_connection_with_passphrase};
use crate::error::{AppError, Result};
use crate::fs_util::{cleanup, replace_file, temp_sibling};

/// 库文件名（固定，不可配置；spec：只选目录、文件名由应用固定）。
pub const DB_FILE_NAME: &str = "ledger.db";

/// 引导指针文件名：位于默认应用数据目录下，是 DataLocation 的唯一权威记录。
/// 删除该文件即回到出厂行为（未配置 → 默认目录）。
pub const POINTER_FILE_NAME: &str = "data_location.json";

/// 指针文件内容：仅「库所在目录」一个意图字段。
/// 除本字段外不得再向指针文件添加任何内容（ADR-0018 后果条款）。
#[derive(Serialize, Deserialize)]
struct PointerFile {
    data_dir: String,
}

/// 启动期 DataLocation 引导结果。
#[derive(Clone)]
pub struct Boot {
    /// 最终生效的库文件目录。
    pub db_dir: PathBuf,
    /// 引导期发生回退（指针损坏 / 目标不可用）时的人类可读原因，
    /// 供界面显著提示；`None` 表示正常定位，未发生回退。
    pub fallback_reason: Option<String>,
    /// 搬迁待解锁后补做（issue #570 / ADR-0075 决策 7）：源库是密文库
    /// 而启动期无主口令，无法执行 `VACUUM INTO`——引导改用源库位置生效，
    /// 待解锁成功后由解锁路径以主口令补做搬迁（成功后重启接管目标位置）。
    pub deferred_relocation: Option<PathBuf>,
}

/// 指针文件读取结果。
#[derive(Debug)]
enum PointerRead {
    /// 文件缺失 → 未配置（出厂行为，无回退信号）。
    Unconfigured,
    /// 文件存在但无法读取/解析 → 视同未配置使用默认目录，但需回退信号。
    Corrupt(String),
    /// 已配置：库所在目录意图。
    Configured(PathBuf),
}

/// 读取指针文件。缺失视同未配置；损坏是常态输入而非异常（一律不 panic、不报错上抛）。
fn read_pointer(default_dir: &Path) -> PointerRead {
    let path = default_dir.join(POINTER_FILE_NAME);
    let raw = match std::fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return PointerRead::Unconfigured,
        Err(e) => {
            tracing::warn!(pointer = %path.display(), error = %e, "无法读取数据位置指针文件");
            return PointerRead::Corrupt(format!("无法读取数据位置指针文件：{e}"));
        }
    };
    match serde_json::from_str::<PointerFile>(&raw) {
        Ok(file) if !file.data_dir.trim().is_empty() => {
            PointerRead::Configured(PathBuf::from(file.data_dir))
        }
        _ => {
            tracing::warn!(pointer = %path.display(), "数据位置指针文件无法解析，视同未配置");
            PointerRead::Corrupt(format!(
                "数据位置指针文件无法解析（{}），已回退默认位置",
                path.display()
            ))
        }
    }
}

/// 执行 DataLocation 引导：读指针 → 三分支定位/搬迁，返回最终生效的库文件目录。
/// 本函数不打开库文件；建连由调用方在 [`Boot::db_dir`] 上继续（`db::open_db_in`）。
pub fn boot(default_dir: &Path) -> Boot {
    match read_pointer(default_dir) {
        PointerRead::Unconfigured => Boot {
            db_dir: default_dir.to_path_buf(),
            fallback_reason: None,
            deferred_relocation: None,
        },
        PointerRead::Corrupt(reason) => Boot {
            db_dir: default_dir.to_path_buf(),
            fallback_reason: Some(reason),
            deferred_relocation: None,
        },
        PointerRead::Configured(target) => relocate_or_adopt(default_dir, &target),
    }
}

/// 三分支：目标位置已有库 → 直接使用；目标为空而原位置有库 → `VACUUM INTO`
/// 整库搬迁后再使用；两者皆无 → 使用目标位置（新建空库由随后的建连完成）。
/// 搬迁失败回退默认目录；源库为密文库时搬迁需要主口令（启动期不可得），
/// 改为推迟到解锁后补做，同样以源库位置生效、不携带回退警示。
fn relocate_or_adopt(default_dir: &Path, target: &Path) -> Boot {
    match enter_target(default_dir, target) {
        Ok(()) => Boot {
            db_dir: target.to_path_buf(),
            fallback_reason: None,
            deferred_relocation: None,
        },
        Err(EnterTargetError::DeferredEncryptedRelocation) => {
            tracing::info!(
                target = %target.display(),
                "源库为密文库，搬迁待解锁后补做（本次仍以源库位置生效）"
            );
            Boot {
                db_dir: default_dir.to_path_buf(),
                fallback_reason: None,
                deferred_relocation: Some(target.to_path_buf()),
            }
        }
        Err(EnterTargetError::Failed(reason)) => {
            tracing::warn!(target = %target.display(), reason = %reason, "DataLocation 引导回退默认目录");
            Boot {
                db_dir: default_dir.to_path_buf(),
                fallback_reason: Some(reason),
                deferred_relocation: None,
            }
        }
    }
}

/// `enter_target` 的失败形态：可报告的失败（回退默认目录）或密文库搬迁
/// 需要主口令而启动期不可得（推迟到解锁后补做，非回退）。
enum EnterTargetError {
    Failed(String),
    DeferredEncryptedRelocation,
}

fn enter_target(default_dir: &Path, target: &Path) -> std::result::Result<(), EnterTargetError> {
    let target_db = target.join(DB_FILE_NAME);
    let source_db = default_dir.join(DB_FILE_NAME);

    // 分支 1：目标位置已有库 → 直接使用（接管已就位的库 / 二次启动幂等）。
    if target_db.exists() {
        return Ok(());
    }
    // 分支 3：两者皆无 → 使用目标位置，空库由随后的建连迁移创建。
    if !source_db.exists() {
        return ensure_target_dir(target).map_err(EnterTargetError::Failed);
    }
    // 分支 2：目标为空而原位置有库 → 整库搬迁。源库是密文库时搬迁必须
    // 凭主口令（无口令的连接首条语句即报 not-a-database），启动期不可得 →
    // 推迟到解锁后补做；非页对齐的非明文文件是损坏残留而非密文库，保持
    // 既有回退行为。
    match probe_file_kind(&source_db) {
        Ok(DbFileKind::Encrypted) if super::encryption::has_encrypted_file_layout(&source_db) => {
            return Err(EnterTargetError::DeferredEncryptedRelocation);
        }
        Ok(_) => {}
        Err(e) => {
            return Err(EnterTargetError::Failed(format!(
                "原库无法探测（{}）：{e}",
                source_db.display()
            )));
        }
    }
    ensure_target_dir(target).map_err(EnterTargetError::Failed)?;
    relocate(&source_db, &target_db, None).map_err(EnterTargetError::Failed)
}

fn ensure_target_dir(target: &Path) -> std::result::Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|e| format!("目标目录不可用（无法创建 {}）：{e}", target.display()))
}

/// 用 `VACUUM INTO` 把源库完整复制到目标：先写唯一临时名，校验完整后再替换启用
/// （复用备份功能的既有机制）。源库只读不写，任何失败都清理临时文件。
/// `passphrase`：源库为密文库时必须携带主口令（带口令打开的连接执行
/// `VACUUM INTO`，产物继承源库加密与密钥，ADR-0075 决策 7）；明文库传 `None`。
fn relocate(
    source_db: &Path,
    target_db: &Path,
    passphrase: Option<&str>,
) -> std::result::Result<(), String> {
    // 按口令有无选建连缝：密文库凭主口令打开（产物继承加密与密钥）。
    let open_by_key = |path: &Path| match passphrase {
        Some(pass) => open_connection_with_passphrase(path, pass),
        None => open_connection(path),
    };
    let source = open_by_key(source_db)
        .map_err(|e| format!("原库无法打开（{}）：{e}", source_db.display()))?;
    let tmp_db = temp_sibling(target_db, "relocate");

    let result = (|| -> std::result::Result<(), String> {
        source
            .execute("VACUUM INTO ?1", params![tmp_db.to_string_lossy()])
            .map_err(|e| format!("整库搬迁失败（VACUUM INTO）：{e}"))?;
        // 校验：临时库能打开且完整性检查通过，才允许替换启用；密文产物
        // （带口令搬迁）凭同一口令验证。
        let check = open_by_key(&tmp_db).map_err(|e| format!("搬迁临时库无法打开：{e}"))?;
        check_integrity(&check).map_err(|e| format!("搬迁临时库完整性检查失败：{e}"))?;
        replace_file(&tmp_db, target_db).map_err(|e| format!("搬迁临时库替换启用失败：{e}"))?;
        Ok(())
    })();

    if let Err(reason) = result {
        // 临时文件用后即清（成功时已被 rename 走，cleanup 容忍不存在）。
        cleanup(&tmp_db);
        return Err(reason);
    }
    Ok(())
}

/// 解锁后补做等待中的搬迁（issue #570）：以主口令打开源密文库执行
/// `VACUUM INTO`，产物继承加密与密钥——目标库仍是密文库（ADR-0075 决策 7）。
/// 成功后需重启应用，由启动引导接管目标位置（与「更改位置重启后生效」
/// 语义一致）。失败时应用继续以当前位置运行，意图保持待重启状态。
pub fn relocate_with_key(source_db: &Path, target_db: &Path, passphrase: &str) -> Result<()> {
    relocate(source_db, target_db, Some(passphrase)).map_err(AppError::Io)
}

/// 读取当前已配置的意图目录（指针存在且可解析时返回 `Some`）。
/// 缺失、损坏一律视同未配置（回退警示由 [`boot`] 结果另行承载）。
/// 供命令层聚合 DataLocation 信息使用（issue #133）。
pub fn configured_intent(default_dir: &Path) -> Option<PathBuf> {
    match read_pointer(default_dir) {
        PointerRead::Configured(dir) => Some(dir),
        PointerRead::Unconfigured | PointerRead::Corrupt(_) => None,
    }
}

/// 把「库所在目录」意图写入指针文件（原子：先写唯一临时名再替换）。
/// 供命令层「更改位置 / 恢复默认」提交意图使用。
pub fn write_pointer(default_dir: &Path, target: &Path) -> crate::error::Result<()> {
    std::fs::create_dir_all(default_dir)?;
    let pointer = default_dir.join(POINTER_FILE_NAME);
    let content = serde_json::to_string_pretty(&PointerFile {
        data_dir: target.to_string_lossy().into_owned(),
    })?;
    let tmp = temp_sibling(&pointer, "pointer");
    let result = (|| -> crate::error::Result<()> {
        std::fs::write(&tmp, content)?;
        replace_file(&tmp, &pointer)
    })();
    cleanup(&tmp);
    result
}

// ---------------------------------------------------------------------------
// 更改意图校验与信息聚合（issue #133 逻辑，#408 自壳层下沉）
// ---------------------------------------------------------------------------

/// 可写性探针文件名（②试写的固定路径，BDD 用同名目录预占可稳定触发拒绝分支）。
pub const WRITE_PROBE_FILE_NAME: &str = ".ledger_write_probe";

/// 更改意图提交结果（issue #133）：更改位置与恢复默认共用。
#[derive(Debug, Serialize)]
pub struct DataLocationChangeOutcome {
    /// 目标已存在同名 `ledger.db`，需用户二选一（接管该库 / 取消换位）。
    /// 前端呈现确认后，以 `adopt_existing = true` 二次提交即接管落盘；
    /// 取消换位则不再提交，状态保持不变。
    pub requires_choice: bool,
    /// 意图是否已落盘（校验通过并写入指针文件，下次启动生效）。
    pub committed: bool,
    /// 已落盘意图的目标目录（`committed` 时有值）。
    pub target_dir: Option<String>,
}

/// 对目标目录执行三步校验，通过后把更改意图写入指针文件。
/// `adopt_existing`：目标已有同名 `ledger.db` 时是否接管（用户二选一后二次提交）。
/// 本函数不搬迁任何文件、不解析既有库内容；真实搬迁只发生在下次启动。
pub fn validate_and_commit(
    default_dir: &Path,
    target: &Path,
    adopt_existing: bool,
) -> Result<DataLocationChangeOutcome> {
    // ① 目录不存在则自动创建。
    std::fs::create_dir_all(target).map_err(|e| {
        AppError::codedp(
            "data-location.mkdir-failed",
            format!("无法创建目标目录（{}）：{e}", target.display()),
            &[&target.display().to_string(), &e.to_string()],
        )
    })?;

    // ② 试写小临时文件验证可写，用后即清。
    let probe = target.join(WRITE_PROBE_FILE_NAME);
    let probe_result = (|| -> std::io::Result<()> {
        std::fs::write(&probe, b"ok")?;
        std::fs::remove_file(&probe)
    })();
    probe_result.map_err(|e| {
        AppError::codedp(
            "data-location.dir-not-writable",
            format!("目标目录不可写（{}）：{e}", target.display()),
            &[&target.display().to_string(), &e.to_string()],
        )
    })?;

    // ③ 目标已有同名库 → 返回二选一信号，不静默覆盖、不解析库内容。
    let target_db = target.join(DB_FILE_NAME);
    if target_db.exists() && !adopt_existing {
        tracing::info!(target = %target.display(), "目标位置已有同名库，返回二选一信号");
        return Ok(DataLocationChangeOutcome {
            requires_choice: true,
            committed: false,
            target_dir: None,
        });
    }

    // 校验通过 → 意图落盘（指针文件原子写入）。
    write_pointer(default_dir, target)?;
    tracing::info!(target = %target.display(), "DataLocation 更改意图已落盘，下次启动生效");
    Ok(DataLocationChangeOutcome {
        requires_choice: false,
        committed: true,
        target_dir: Some(target.to_string_lossy().into_owned()),
    })
}

/// DataLocation 当前信息（issue #133）：设置页展示用。
#[derive(Debug, Serialize)]
pub struct DataLocationInfo {
    /// 当前生效的库文件目录（完整路径）。
    pub active_dir: String,
    /// 指针文件记录的意图目录；`None` = 未配置（缺失或损坏均视同未配置，
    /// 损坏时的警示由 `fallback_reason` 另行承载）。
    pub configured_dir: Option<String>,
    /// 已更改待重启生效：意图目录 ≠ 当前生效目录（意图已落盘、搬迁尚未发生）。
    pub pending_restart: bool,
    /// 上次启动引导发生回退的原因（供界面显著提示）；`None` = 未回退。
    pub fallback_reason: Option<String>,
}

/// 聚合 DataLocation 信息（引导结果可选）：壳层与 BDD 共用同一降级逻辑——
/// 引导结果未登记（异常时序）时按出厂行为降级：生效目录即默认目录。
pub fn gather_info_from_boot(default_dir: &Path, boot: Option<&Boot>) -> DataLocationInfo {
    let (active_dir, fallback) = match boot {
        Some(boot) => (boot.db_dir.clone(), boot.fallback_reason.clone()),
        None => (default_dir.to_path_buf(), None),
    };
    gather_info(default_dir, &active_dir, fallback.as_deref())
}

/// 聚合 DataLocation 信息：生效目录 / 意图目录 / 待重启生效 / 回退警示。
/// 壳层与 BDD 共用的实现；`active_dir` 与 `fallback` 来自启动期
/// 已登记的引导结果（[`Boot`]）。
pub fn gather_info(
    default_dir: &Path,
    active_dir: &Path,
    fallback_reason: Option<&str>,
) -> DataLocationInfo {
    let configured_dir = configured_intent(default_dir);
    let pending_restart = match &configured_dir {
        Some(intent) => intent != active_dir,
        None => false,
    };
    DataLocationInfo {
        active_dir: active_dir.to_string_lossy().into_owned(),
        configured_dir: configured_dir.map(|dir| dir.to_string_lossy().into_owned()),
        pending_restart,
        fallback_reason: fallback_reason.map(str::to_string),
    }
}

#[cfg(test)]
mod tests;
