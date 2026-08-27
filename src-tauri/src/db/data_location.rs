//! DataLocation 引导内核（issue #132 / ADR-0018）。
//!
//! 收口「读指针 → 三分支定位/搬迁」：以"默认应用数据目录 + 可选指针"为输入，
//! 返回最终生效的库文件目录。纯 Rust、不依赖 Tauri runtime，建连前的唯一
//! DataLocation 权威。术语见 CONTEXT.md 的 DataLocation / Relocation 条目。
//!
//! 回退原则：指针损坏、目标不可用等一切引导期失败都回退默认目录并通过
//! [`Boot::fallback_reason`] 告知调用方（供界面显著提示）；绝不删除或修改
//! 任何既有文件，搬迁完成后旧位置的库永久保留。

use std::path::{Path, PathBuf};

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{check_integrity, open_connection};
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
pub struct Boot {
    /// 最终生效的库文件目录。
    pub db_dir: PathBuf,
    /// 引导期发生回退（指针损坏 / 目标不可用）时的人类可读原因，
    /// 供界面显著提示；`None` 表示正常定位，未发生回退。
    pub fallback_reason: Option<String>,
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
        },
        PointerRead::Corrupt(reason) => Boot {
            db_dir: default_dir.to_path_buf(),
            fallback_reason: Some(reason),
        },
        PointerRead::Configured(target) => relocate_or_adopt(default_dir, &target),
    }
}

/// 三分支：目标位置已有库 → 直接使用；目标为空而原位置有库 → `VACUUM INTO`
/// 整库搬迁后再使用；两者皆无 → 使用目标位置（新建空库由随后的建连完成）。
/// 任何失败都回退默认目录并携带回退原因，绝不删除或修改既有文件。
fn relocate_or_adopt(default_dir: &Path, target: &Path) -> Boot {
    match enter_target(default_dir, target) {
        Ok(()) => Boot {
            db_dir: target.to_path_buf(),
            fallback_reason: None,
        },
        Err(reason) => {
            tracing::warn!(target = %target.display(), reason = %reason, "DataLocation 引导回退默认目录");
            Boot {
                db_dir: default_dir.to_path_buf(),
                fallback_reason: Some(reason),
            }
        }
    }
}

fn enter_target(default_dir: &Path, target: &Path) -> std::result::Result<(), String> {
    let target_db = target.join(DB_FILE_NAME);
    let source_db = default_dir.join(DB_FILE_NAME);

    // 分支 1：目标位置已有库 → 直接使用（接管已就位的库 / 二次启动幂等）。
    if target_db.exists() {
        return Ok(());
    }
    // 分支 3：两者皆无 → 使用目标位置，空库由随后的建连迁移创建。
    if !source_db.exists() {
        return ensure_target_dir(target);
    }
    // 分支 2：目标为空而原位置有库 → 整库搬迁。
    ensure_target_dir(target)?;
    relocate(&source_db, &target_db)
}

fn ensure_target_dir(target: &Path) -> std::result::Result<(), String> {
    std::fs::create_dir_all(target)
        .map_err(|e| format!("目标目录不可用（无法创建 {}）：{e}", target.display()))
}

/// 用 `VACUUM INTO` 把源库完整复制到目标：先写唯一临时名，校验完整后再替换启用
/// （复用备份功能的既有机制）。源库只读不写，任何失败都清理临时文件后回退。
fn relocate(source_db: &Path, target_db: &Path) -> std::result::Result<(), String> {
    let source = open_connection(source_db)
        .map_err(|e| format!("原库无法打开（{}）：{e}", source_db.display()))?;
    let tmp_db = temp_sibling(target_db, "relocate");

    let result = (|| -> std::result::Result<(), String> {
        source
            .execute("VACUUM INTO ?1", params![tmp_db.to_string_lossy()])
            .map_err(|e| format!("整库搬迁失败（VACUUM INTO）：{e}"))?;
        // 校验：临时库能打开且完整性检查通过，才允许替换启用。
        let check = open_connection(&tmp_db).map_err(|e| format!("搬迁临时库无法打开：{e}"))?;
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

#[cfg(test)]
mod tests;
