//! 文件级原子操作工具：临时名生成、原子替换、临时文件清理。
//!
//! 备份（`backup` 域）与 DataLocation 搬迁（`db::data_location`）共用的
//! 既有机制（ADR-0018：「先写唯一临时名，校验后再替换启用」），独立成模块
//! 避免基础设施层反向依赖命令层。

use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

/// 生成与 `path` 同目录的临时文件路径（名称带唯一后缀）。
pub fn temp_sibling(path: &Path, tag: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(
        ".{file_name}.{tag}-{}-{}",
        std::process::id(),
        crate::db::new_uuid()
    ))
}

/// 原子替换：优先 rename（Unix 上覆盖已存在文件），失败时先删除再 rename（Windows 兼容）。
pub fn replace_file(src: &Path, dst: &Path) -> Result<()> {
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(first) => match std::fs::remove_file(dst).and_then(|_| std::fs::rename(src, dst)) {
            Ok(()) => Ok(()),
            Err(second) => {
                tracing::error!(first = %first, second = %second, "替换文件失败");
                Err(AppError::Io(format!("替换文件失败: {first}（{second}）")))
            }
        },
    }
}

/// 若文件存在则删除（临时文件收尾用，容忍不存在）。
pub fn cleanup(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path).ok();
    }
}
