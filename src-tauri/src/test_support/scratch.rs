//! 测试暂存目录 guard（issue #1645）：Rust 测试真临时目录现场的 RAII 清理。
//!
//! `ScratchDir` 承接「建目录 + 全部生命周期收尾」：构造即建
//! `temp_dir()/ledger-test-{tag}-{uuid}/`，drop（含 panic unwind）整棵删除；
//! `ScratchFile` 是散文件夹具的目录化形态（临时 .db/.zip 等文件住进自己的
//! 暂存目录）。手写 `remove_dir_all` 收尾随迁移退役，调用方只学构造函数并把
//! 返回值活到用例结束。
//!
//! **前缀闭集**：全部暂存目录落 `ledger-test-` 前缀下——跑完测试后 `/tmp`
//! 残留可一条 `rm -rf /tmp/ledger-test-*` 识别清理。两类既有 `temp_dir()`
//! 调用点不属本器具：进程级 `$HOME` 夹具（目录进程存活期不能删）与产品代码
//! 路径（自有收尾机制）。

use std::path::{Path, PathBuf};

use ledger_infra::db::new_uuid;

/// 测试暂存目录：`temp_dir()/ledger-test-{tag}-{uuid}/`，drop（含 panic
/// unwind）整棵删除。`tag` 保留调用方语义（如 `e2e-dl-default`）作残留识别
/// 的第二段；`Deref<Target = Path>` + `AsRef<Path>` 让既有以 `PathBuf` 形态
/// 消费目录的调用点机械迁移（`join` / `display` / `&dir` 照旧）。
pub struct ScratchDir {
    root: PathBuf,
}

impl ScratchDir {
    pub fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("ledger-test-{tag}-{}", new_uuid()));
        std::fs::create_dir_all(&root).expect("创建测试暂存目录");
        Self { root }
    }

    pub fn path(&self) -> &Path {
        &self.root
    }
}

impl std::ops::Deref for ScratchDir {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.root
    }
}

impl AsRef<Path> for ScratchDir {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        // 清理失败静默吸收（`let _ =`）：清理失败不得变成测试失败的噪声源；
        // panic unwind 时照常 drop，红测不留残留。
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// 暂存文件：散文件夹具收进各自的暂存目录，drop（含 panic unwind）连目录
/// 一起删除。`Deref` / `AsRef` 指向**文件**——既有以 `PathBuf` 持有散文件的
/// 调用点（`&file`、`file.parent()`）机械迁移。guard 由调用方持有到用例
/// 结束；生命周期归测试世界/场景持有的场景改用 [`ScratchDir`] + `join`。
pub struct ScratchFile {
    path: PathBuf,
    _dir: ScratchDir,
}

impl ScratchFile {
    /// `file_name` 是暂存目录内的文件名；历史散文件名（含语义前缀）原样内迁，
    /// 对文件名的既有断言不受影响。
    pub fn new(tag: &str, file_name: impl Into<String>) -> Self {
        let dir = ScratchDir::new(tag);
        let path = dir.path().join(file_name.into());
        Self { path, _dir: dir }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl std::ops::Deref for ScratchFile {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.path
    }
}

impl AsRef<Path> for ScratchFile {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 前缀契约：目录名以 `ledger-test-{tag}-` 起头——残留可按一条 rm 识别
    /// 清理；前缀形态被改掉本断言红。
    #[test]
    fn dir_name_carries_normalized_prefix() {
        let dir = ScratchDir::new("selftest-prefix");
        let name = dir.path().file_name().unwrap().to_string_lossy();
        assert!(
            name.starts_with("ledger-test-selftest-prefix-"),
            "暂存目录应带归一前缀，实际 {name}"
        );
    }

    /// 清理契约：drop 整棵删除——建目录、写嵌套文件，drop 后根目录消失；
    /// 删掉 Drop 实现（或改成 no-op）本断言红。
    #[test]
    fn drop_removes_the_whole_tree() {
        let dir = ScratchDir::new("selftest-drop");
        let root = dir.path().to_path_buf();
        let nested = root.join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(nested.join("a.txt"), b"x").unwrap();
        assert!(root.exists());
        drop(dir);
        assert!(!root.exists(), "drop 后整棵暂存目录应消失");
    }

    /// panic unwind 照常清理：持 guard 的线程 panic（unwind 经过 drop）后，
    /// 暂存目录不残留——红测不留残留的承诺；去掉 Drop 实现本断言红。
    #[test]
    fn panic_unwind_still_cleans_up() {
        let dir = ScratchDir::new("selftest-unwind");
        let root = dir.path().to_path_buf();
        assert!(root.exists());
        let joined = std::thread::spawn(move || {
            let _guard = dir;
            panic!("unwind 经过的现场");
        })
        .join();
        assert!(joined.is_err(), "现场线程应 panic");
        assert!(!root.exists(), "panic unwind 后暂存目录不应残留");
    }

    /// 暂存文件：文件落在自己的暂存目录内，drop 连目录一起删除；删掉 Drop
    /// 实现本断言红。
    #[test]
    fn scratch_file_lives_in_its_own_dir_and_cleans_up() {
        let file = ScratchFile::new("selftest-file", "fixture.db");
        std::fs::write(&file, b"x").unwrap();
        assert!(file.exists());
        let parent = file.path().parent().unwrap().to_path_buf();
        assert!(
            parent
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("ledger-test-")
        );
        drop(file);
        assert!(!parent.exists(), "drop 后暂存文件连目录应一起消失");
    }
}
