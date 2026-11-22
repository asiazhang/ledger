use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

pub fn temp_sibling(path: &Path, tag: &str) -> PathBuf {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    parent.join(format!(
        ".{file_name}.{tag}-{}-{}",
        std::process::id(),
        crate::ids::new_uuid()
    ))
}

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

pub fn cleanup(path: &Path) {
    if path.exists() {
        std::fs::remove_file(path).ok();
    }
}

pub fn atomic_write(path: &Path, contents: &[u8]) -> Result<()> {
    let tmp = temp_sibling(path, "atomic");
    let result = (|| -> Result<()> {
        std::fs::write(&tmp, contents)?;
        replace_file(&tmp, path)
    })();
    cleanup(&tmp);
    result
}
