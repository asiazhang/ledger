//! 备份最小入口读写接缝（#2038）：手动备份与账本指纹直调 `ledger-backup`
//! 域入口——交易列表工具区一键备份（backup_entry）与设置卡备份卡消费。

use std::path::Path;

use ledger_infra::db::DbState;
use ledger_infra::error::{AppError, Result};

use super::conn_lock;

// ---------------------------------------------------------------------------

/// 创建手动备份（域 backup 直调，旧壳 `create_backup` 命令同一执行协议）：
/// 目标先经域受管改道判定（[`ledger_backup::managed_command_target`]——落在
/// 配置的备份目录内且为受管命名时改道设备子目录；NSH 本票无备份目录设置
/// 面、进程内偏好镜像未配置时原样落盘，与旧壳未配置备份目录的另存为同
/// 口径），再 `VACUUM INTO` 一致性快照打包 zip（ADR-0007 文件级语义，双库
/// 成对 ADR-0139）。写连接锁内裸作业：备份是只读快照，不经统一写入口的置
/// 脏/到期检查（与旧壳命令同口径，连接锁预算见备份域 [`ledger_backup::LOCK_HOLD_BUDGET`]）。
pub fn create_backup(
    db: &DbState,
    target: &Path,
    app_version: &str,
) -> Result<ledger_backup::BackupResult> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let target = ledger_backup::managed_command_target(target, &conn)?;
    ledger_backup::backup_db_to(
        &conn,
        &target,
        app_version,
        ledger_backup::BackupKind::Manual,
    )
}

/// 当前账本指纹只读判定（备份默认文件名的标识段，旧壳 `get_book_fingerprint`
/// 命令消费的同一域函数）：已固化恒读固化值，未固化回内容锚，空库且未固化
/// = `None`（退化为无标识旧命名，产物归属由内层元数据水印承担）。读失败归
/// `None`——旧壳 `refreshBookFingerprint` 容错同口径：指纹缺失只退化命名，
/// 不阻断备份。读连接是成对建连的只读侧（ADR-0117）。
pub fn book_fingerprint(db: &DbState) -> Option<String> {
    let conn = conn_lock(&db.read_conn).ok()?;
    ledger_backup::book_fingerprint_readonly(&conn)
        .ok()
        .flatten()
}
