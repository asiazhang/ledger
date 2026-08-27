//! 备份/恢复测试（issue #91 外迁）：zip 打包/恢复往返/新旧 schema 策略/受管备份列表与修剪。

use std::fs::File;
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use rusqlite::params;

use super::*;
use crate::db;
use crate::db::{init_db, open_connection, open_in_memory};

fn temp_file(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ledger-backup-test-{tag}-{}-{}.db",
        std::process::id(),
        db::new_uuid()
    ))
}

/// 建内存库并写入一条账户 + 一条交易。
fn seed(conn: &Connection) {
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('acc-1','现金','cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('txn-1','expense',1500,'CNY',1500,'acc-1','2026-02-01','2026-02-01T00:00:00Z','2026-02-01T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap();
}

fn count_transactions(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM transactions", [], |r| r.get(0))
        .unwrap()
}

/// 为每个测试准备独立的安全备份目录（互不干扰）。
fn temp_safety_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ledger-backup-test-safety-{}-{}",
        std::process::id(),
        db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn backup_creates_zip_with_db_and_meta() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    seed(&conn);

    let target = temp_file("zip");
    let result = backup_db_to(&conn, &target, "0.2.0", BackupKind::Manual).unwrap();

    assert!(target.exists());
    assert!(result.size_bytes > 0);
    assert!(result.schema_version >= 4);

    // 校验 zip 内容：两个条目。
    let file = File::open(&target).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), 2);
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
        .collect();
    assert!(names.contains(&"ledger.db".to_string()));
    assert!(names.contains(&"backup.json".to_string()));

    // 解出 db 可打开且数据完整。
    let extracted = temp_file("extracted");
    let mut db_entry = archive.by_name("ledger.db").unwrap();
    let mut out = File::create(&extracted).unwrap();
    std::io::copy(&mut db_entry, &mut out).unwrap();
    drop(out);
    let db_conn = open_connection(&extracted).unwrap();
    assert_eq!(count_transactions(&db_conn), 1);
    super::core::cleanup(&target);
    super::core::cleanup(&extracted);
}

#[test]
fn restore_roundtrip_preserves_data() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    seed(&conn);

    let backup = temp_file("rt-backup");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    // 目标库先建好，含一条多余交易；恢复后应只剩备份里的数据。
    let db_path = temp_file("rt-db");
    {
        let mut c = open_connection(&db_path).unwrap();
        init_db(&mut c).unwrap();
        seed(&c);
        c.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('txn-2','expense',999,'CNY',999,'acc-1','2026-03-01','2026-03-01T00:00:00Z','2026-03-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
        assert_eq!(count_transactions(&c), 2);
    }

    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected).unwrap();
    assert_eq!(result.schema_version, expected);

    // 恢复后数据与备份一致（1 条交易）。
    let c = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&c), 1);
    // 恢复前的库被安全备份。
    let safeties: Vec<_> = std::fs::read_dir(&safety_dir)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("restore-safety-")
        })
        .collect();
    assert_eq!(safeties.len(), 1);

    super::core::cleanup(&backup);
    super::core::cleanup(&db_path);
    std::fs::remove_dir_all(&safety_dir).ok();
}

#[test]
fn restore_rejects_newer_schema() {
    // 构造一个 schema 版本更高的库文件。
    let newer = temp_file("newer");
    {
        let c = open_connection(&newer).unwrap();
        c.execute_batch("PRAGMA user_version = 999").unwrap();
    }
    let db_path = temp_file("db");
    let expected = expected_schema_version().unwrap();
    let tmp_dir = std::env::temp_dir();
    let err = restore_db_from(&newer, &db_path, &tmp_dir, expected)
        .unwrap_err()
        .to_string();
    assert!(err.contains("更高版本"), "错误信息: {err}");
    assert!(!db_path.exists(), "恢复应被拒绝，不产生目标库");
    super::core::cleanup(&newer);
    super::core::cleanup(&db_path);
}

#[test]
fn restore_supports_bare_db() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    seed(&conn);

    // 直接 VACUUM INTO 生成裸 db 文件作为"备份"。
    let bare = temp_file("bare");
    conn.execute("VACUUM INTO ?1", params![bare.to_string_lossy()])
        .unwrap();

    let db_path = temp_file("db2");
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    restore_db_from(&bare, &db_path, &safety_dir, expected).unwrap();
    let c = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&c), 1);
    super::core::cleanup(&bare);
    super::core::cleanup(&db_path);
    std::fs::remove_dir_all(&safety_dir).ok();
}

#[test]
fn backup_meta_records_kind_for_auto_and_manual() {
    let conn = open_in_memory().unwrap();
    // 手动产物：kind 落盘为 manual。
    let manual = temp_file("meta-manual");
    backup_db_to(&conn, &manual, "0.2.0", BackupKind::Manual).unwrap();
    assert_eq!(read_backup_kind(&manual).unwrap(), BackupKind::Manual);
    super::core::cleanup(&manual);

    // 自动产物：kind 落盘为 auto。
    let auto = temp_file("meta-auto");
    backup_db_to(&conn, &auto, "0.2.0", BackupKind::Auto).unwrap();
    assert_eq!(read_backup_kind(&auto).unwrap(), BackupKind::Auto);
    super::core::cleanup(&auto);
}

/// 旧版本备份的 backup.json 缺 kind 字段：读取不报错且视为 manual。
#[test]
fn legacy_meta_without_kind_reads_as_manual() {
    let path = temp_file("legacy-meta");
    {
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("ledger.db", options).unwrap();
        std::io::Write::write_all(&mut zip, b"stub").unwrap();
        zip.start_file("backup.json", options).unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"{"created_at":"2025-01-01T00:00:00Z","app_version":"0.1.0","schema_version":4}"#,
        )
        .unwrap();
        zip.finish().unwrap();
    }
    assert_eq!(read_backup_kind(&path).unwrap(), BackupKind::Manual);
    super::core::cleanup(&path);
}

/// 元数据里出现未知/非法的 kind 值：宽容回落 manual 而非解析失败（兼容优先）。
#[test]
fn meta_with_unknown_kind_reads_as_manual() {
    let path = temp_file("unknown-kind");
    {
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("ledger.db", options).unwrap();
        std::io::Write::write_all(&mut zip, b"stub").unwrap();
        zip.start_file("backup.json", options).unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"{"created_at":"2025-01-01T00:00:00Z","app_version":"0.1.0","schema_version":4,"kind":"AutoMated"}"#,
        )
        .unwrap();
        zip.finish().unwrap();
    }
    assert_eq!(read_backup_kind(&path).unwrap(), BackupKind::Manual);
    super::core::cleanup(&path);
}

/// 旧版本备份（元数据无 kind 字段）：恢复不报错、列表正常出现，视为 manual。
#[test]
fn legacy_backup_restores_and_lists_without_error() {
    let mut conn = open_in_memory().unwrap();
    init_db(&mut conn).unwrap();
    seed(&conn);

    // 用 VACUUM INTO 造一份裸库，再打包成元数据缺 kind 的旧格式 zip。
    let raw = temp_file("legacy-raw");
    conn.execute("VACUUM INTO ?1", params![raw.to_string_lossy()])
        .unwrap();
    let dir = std::env::temp_dir().join(format!(
        "ledger-backup-legacy-dir-{}-{}",
        std::process::id(),
        db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let legacy = dir.join("ledger-backup-20260101-000000.db.zip");
    {
        let file = File::create(&legacy).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("ledger.db", options).unwrap();
        std::io::copy(&mut File::open(&raw).unwrap(), &mut zip).unwrap();
        zip.start_file("backup.json", options).unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"{"created_at":"2025-01-01T00:00:00Z","app_version":"0.1.0","schema_version":4}"#,
        )
        .unwrap();
        zip.finish().unwrap();
    }
    super::core::cleanup(&raw);

    // 列表：旧格式文件按命名规则正常被识别，来源按 manual 处理。
    let list = list_managed_backups(&dir).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        read_backup_kind(Path::new(&list[0].path)).unwrap(),
        BackupKind::Manual
    );

    // 恢复：旧格式包完整还原数据。
    let db_path = temp_file("legacy-restore-db");
    let safety_dir = temp_safety_dir();
    let result = restore_db_from(
        &legacy,
        &db_path,
        &safety_dir,
        expected_schema_version().unwrap(),
    );
    assert!(result.is_ok(), "旧格式备份恢复失败: {:?}", result.err());
    let c = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&c), 1);

    super::core::cleanup(&legacy);
    super::core::cleanup(&db_path);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&safety_dir);
}

#[test]
fn list_and_prune_managed_backups() {
    let dir = std::env::temp_dir().join(format!(
        "ledger-backup-managed-{}-{}",
        std::process::id(),
        db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    // 3 个手动自动命名文件 + 1 个自动备份命名文件 + 1 个不匹配命名 + 1 个名字匹配但为目录。
    for (name, size) in [
        ("ledger-backup-20260101-010101.db.zip", 10u64),
        ("ledger-backup-20260102-010101.db.zip", 20),
        ("ledger-backup-20260103-010101.db.zip", 30),
        ("ledger-auto-20260201-010101.db.zip", 40),
    ] {
        std::fs::write(dir.join(name), vec![0u8; size as usize]).unwrap();
    }
    std::fs::write(dir.join("notes.zip"), b"x").unwrap();
    std::fs::create_dir(dir.join("ledger-backup-20260104-010101.db.zip")).unwrap();

    let list = list_managed_backups(&dir).unwrap();
    assert_eq!(list.len(), 4);
    assert_eq!(
        list[0].file_name, "ledger-auto-20260201-010101.db.zip",
        "auto 前缀同样受管且按时间排序"
    );
    assert_eq!(list[0].created_at, "2026-02-01T01:01:01Z");
    assert_eq!(list[3].file_name, "ledger-backup-20260101-010101.db.zip");

    // 修剪到 2：删除最旧的手动 2 个；不匹配文件与目录不受影响。
    let r = prune_managed_backups(&dir, 2).unwrap();
    assert_eq!(
        r.deleted,
        vec![
            "ledger-backup-20260101-010101.db.zip",
            "ledger-backup-20260102-010101.db.zip"
        ]
    );
    assert!(r.failed.is_empty());
    assert_eq!(r.kept, 2);
    assert!(dir.join("notes.zip").exists());
    assert!(dir.join("ledger-backup-20260104-010101.db.zip").is_dir());
    assert!(dir.join("ledger-auto-20260201-010101.db.zip").exists());

    // 继续修剪到 1。
    let r2 = prune_managed_backups(&dir, 1).unwrap();
    assert_eq!(r2.deleted, vec!["ledger-backup-20260103-010101.db.zip"]);
    assert_eq!(r2.kept, 1);
    assert!(dir.join("ledger-auto-20260201-010101.db.zip").exists());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn prune_keeps_all_when_within_limit_and_missing_dir() {
    let dir = std::env::temp_dir().join(format!(
        "ledger-backup-prune-none-{}-{}",
        std::process::id(),
        db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("ledger-backup-20260101-010101.db.zip"), b"x").unwrap();

    let r = prune_managed_backups(&dir, 30).unwrap();
    assert!(r.deleted.is_empty());
    assert_eq!(r.kept, 1);

    // 目录不存在：空结果而非报错。
    let missing = dir.join("gone");
    assert!(list_managed_backups(&missing).unwrap().is_empty());
    let r2 = prune_managed_backups(&missing, 5).unwrap();
    assert_eq!(r2.kept, 0);
    assert!(r2.deleted.is_empty());

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn backup_fails_when_target_dir_missing() {
    let conn = open_in_memory().unwrap();
    let missing = std::env::temp_dir().join(format!(
        "no-such-dir-{}-{}",
        std::process::id(),
        db::new_uuid()
    ));
    let target = missing.join("x.zip");
    let err = backup_db_to(&conn, &target, "0.2.0", BackupKind::Manual)
        .unwrap_err()
        .to_string();
    assert!(err.contains("备份目标目录不存在"));
}
