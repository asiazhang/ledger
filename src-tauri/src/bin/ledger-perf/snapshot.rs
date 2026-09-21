//! 写基准共享快照机制（bench-import / bench-sync 共用）：pristine 快照与迭代恢复。
//!
//! 「迭代前从快照恢复隔离写副作用」是写基准的公共形态（bench-import 先例，
//! issue #532；bench-sync 随 #1628 收编共用）：源库复制出 pristine 快照，每次
//! 迭代从快照恢复工作库——迭代间数据集规模固定，p95 是同一状态的真分位数；
//! 源库全程零改动。
//!
//! 恢复后紧跟一次预写提交冲净拷贝写回（量测有效性）：文件拷贝会在 OS 页缓存
//! 里留下约整个库文件大小的脏页，恢复后全进程第一次 fsync（即被测写入的
//! COMMIT）会把这笔写回一并冲掉——大库上把每次量测虚增到秒级，量到的是写回
//! 不是被测写入；机制见 [`restore_from_snapshot`]。

use std::path::{Path, PathBuf};

use ledger_infra::db::open_connection;

/// 快照与工作库的文件路径组（源库同目录，保证同盘复制与权限一致）。
///
/// `label` 进中间文件名（如 `bench-import` / `bench-sync`）：多个写基准共用
/// 同一源库时中间文件互不踩踏。Drop 时删除快照/工作库及其 -wal/-shm 残留：
/// 成功、失败、panic 路径都不留基准中间文件（源库本身全程零改动）。
pub(crate) struct SnapshotPaths {
    pub(crate) snapshot: PathBuf,
    pub(crate) work: PathBuf,
}

impl SnapshotPaths {
    /// 建路径组并落 pristine 快照（源库文件复制；generate 末尾已回填余额缓存
    /// 并 ANALYZE，快照即健康 V017 形态，无需再补基线）。
    pub(crate) fn create(source_db: &Path, label: &str) -> Result<Self, String> {
        let dir = source_db
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .ok_or_else(|| {
                format!(
                    "无法确定快照目录（源库路径缺少父目录）：{}",
                    source_db.display()
                )
            })?;
        let snapshot = dir.join(format!("ledger-perf-{label}-snapshot.db"));
        let work = dir.join(format!("ledger-perf-{label}-work.db"));
        remove_db_files(&work);
        remove_db_files(&snapshot);
        copy_db_files(source_db, &snapshot)?;
        // 工作库先行从快照恢复：探测与正式迭代打开同一个完整状态的库。
        restore_from_snapshot(&snapshot, &work)?;
        Ok(SnapshotPaths { snapshot, work })
    }
}

impl Drop for SnapshotPaths {
    fn drop(&mut self) {
        remove_db_files(&self.work);
        remove_db_files(&self.snapshot);
    }
}

/// 复制库文件（含 -wal 残留防御：干净关闭的库无 -wal，存在则一并带走，
/// 快照必须自含完整状态）。
fn copy_db_files(src: &Path, dst: &Path) -> Result<(), String> {
    std::fs::copy(src, dst)
        .map_err(|e| format!("快照复制失败（{} → {}）：{e}", src.display(), dst.display()))?;
    let src_wal = sidecar_path(src, "-wal");
    if src_wal.exists() {
        std::fs::copy(&src_wal, sidecar_path(dst, "-wal"))
            .map_err(|e| format!("快照复制失败（-wal 伴生文件）：{e}；请确认源库已干净关闭"))?;
    }
    Ok(())
}

/// 从快照恢复工作库：迭代间数据集规模固定的落点。
///
/// 恢复后紧跟一次预写提交（量测有效性）：恢复后全进程第一次 fsync（即被测
/// 写入的 COMMIT）会把文件拷贝留下的 OS 脏页写回一并冲掉——608MB 库上约
/// 2.5s，与被测写入无关，却恰好落进计时窗口（DELETE 日志 + synchronous=FULL
/// 下 fsync 计入提交语句）。预写必须产生真实页写入：SQLite 对「值未变化的
/// UPDATE」跳过写页、提交零 fsync（`SET version = version` 形态吸收不了写回），
/// 故对单行做 version+1；fsync 按文件生效，一次提交即冲净拷贝写回，此后计时
/// 窗口量到的才是被测写入本身。
pub(crate) fn restore_from_snapshot(snapshot: &Path, work: &Path) -> Result<(), String> {
    remove_db_files(work);
    copy_db_files(snapshot, work)?;
    {
        let conn = open_connection(work).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "BEGIN IMMEDIATE; \
             UPDATE accounts SET version = version + 1 WHERE id = (SELECT min(id) FROM accounts); \
             COMMIT;",
        )
        .map_err(|e| format!("恢复后预写提交失败（拷贝写回吸收）：{e}"))?;
    }
    Ok(())
}

/// 删除库文件及其 -wal/-shm 伴生文件（尽力而为，不存在即忽略）。
fn remove_db_files(db: &Path) {
    for path in [
        db.to_path_buf(),
        sidecar_path(db, "-wal"),
        sidecar_path(db, "-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

/// 库文件的 -wal/-shm 伴生路径。
fn sidecar_path(db: &Path, suffix: &str) -> PathBuf {
    let mut s = db.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}
