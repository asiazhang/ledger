use std::path::{Path, PathBuf};

use ledger_infra::db::new_uuid;

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
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

pub struct ScratchFile {
    path: PathBuf,
    _dir: ScratchDir,
}

impl ScratchFile {
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

    #[test]
    fn dir_name_carries_normalized_prefix() {
        let dir = ScratchDir::new("selftest-prefix");
        let name = dir.path().file_name().unwrap().to_string_lossy();
        assert!(
            name.starts_with("ledger-test-selftest-prefix-"),
            "暂存目录应带归一前缀，实际 {name}"
        );
    }

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
