use std::path::{Path, PathBuf};

use ledger_infra::db::{open_connection, sync_db_path};

pub(crate) struct SnapshotPaths {
    pub(crate) snapshot: PathBuf,
    pub(crate) work: PathBuf,
}

fn paired_sync_path(db: &Path) -> PathBuf {
    let mut name = db.file_stem().unwrap_or(db.as_os_str()).to_os_string();
    name.push("-sync.db");
    db.with_file_name(name)
}

pub(crate) fn open_paired(db: &Path) -> Result<rusqlite::Connection, String> {
    let conn = open_connection(db).map_err(|e| e.to_string())?;
    let sync = paired_sync_path(db);
    if sync.exists() {
        conn.execute_batch(&format!("ATTACH DATABASE '{}' AS sync", sync.display()))
            .map_err(|e| format!("伴生同步件挂载失败（{}）：{e}", sync.display()))?;
    }
    Ok(conn)
}

impl SnapshotPaths {
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
        remove_db_files(&paired_sync_path(&work));
        remove_db_files(&paired_sync_path(&snapshot));
        copy_db_files(source_db, &snapshot)?;
        let source_sync = sync_db_path(source_db);
        if source_sync.exists() {
            copy_db_files(&source_sync, &paired_sync_path(&snapshot))?;
        }
        restore_from_snapshot(&snapshot, &work)?;
        Ok(SnapshotPaths { snapshot, work })
    }
}

impl Drop for SnapshotPaths {
    fn drop(&mut self) {
        remove_db_files(&self.work);
        remove_db_files(&self.snapshot);
        remove_db_files(&paired_sync_path(&self.work));
        remove_db_files(&paired_sync_path(&self.snapshot));
    }
}

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

pub(crate) fn restore_from_snapshot(snapshot: &Path, work: &Path) -> Result<(), String> {
    remove_db_files(work);
    remove_db_files(&paired_sync_path(work));
    copy_db_files(snapshot, work)?;
    let snapshot_sync = paired_sync_path(snapshot);
    if snapshot_sync.exists() {
        copy_db_files(&snapshot_sync, &paired_sync_path(work))?;
    }
    {
        let conn = open_paired(work)?;
        conn.execute_batch(
            "BEGIN IMMEDIATE; \
             UPDATE accounts SET version = version + 1 WHERE id = (SELECT min(id) FROM accounts); \
             COMMIT;",
        )
        .map_err(|e| format!("恢复后预写提交失败（拷贝写回吸收）：{e}"))?;
    }
    Ok(())
}

fn remove_db_files(db: &Path) {
    for path in [
        db.to_path_buf(),
        sidecar_path(db, "-wal"),
        sidecar_path(db, "-shm"),
    ] {
        let _ = std::fs::remove_file(path);
    }
}

fn sidecar_path(db: &Path, suffix: &str) -> PathBuf {
    let mut s = db.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}
