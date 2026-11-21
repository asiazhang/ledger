use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use tauri_app_lib::test_support::{ScratchDir, ScratchFile, merge_extra_backup_meta};

use rusqlite::Connection;
use rusqlite::params;

use super::identity::book_fingerprint;
use super::*;
use ledger_infra::db;
use ledger_infra::db::open_connection;

fn temp_file(tag: &str) -> ScratchFile {
    ScratchFile::new(
        &format!("backup-test-{tag}"),
        format!("ledger-backup-test-{tag}-{}.db", db::new_uuid()),
    )
}

fn seed(conn: &Connection) {
    tauri_app_lib::test_support::seed_account(conn, "acc-1", "现金", "cash", "CNY", 0);
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

fn temp_safety_dir() -> ScratchDir {
    ScratchDir::new("backup-test-safety")
}

fn real_managed_backup(dir: &Path, file_name: &str, kind: BackupKind) -> PathBuf {
    real_managed_backup_with_meta(dir, file_name, kind, &serde_json::json!({}))
}

fn real_managed_backup_with_meta(
    dir: &Path,
    file_name: &str,
    kind: BackupKind,
    extra_meta: &serde_json::Value,
) -> PathBuf {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);
    let target = dir.join(file_name);
    backup_db_to(&conn, &target, "0.2.0", kind).unwrap();
    if extra_meta.as_object().is_some_and(|o| !o.is_empty()) {
        merge_extra_backup_meta(&target, extra_meta);
    }
    target
}

#[test]
fn real_managed_backup_fixture_produces_paired_product() {
    let dir = ScratchDir::new("backup-test-fixture");
    let path = real_managed_backup(
        &dir,
        "ledger-auto-20260101-000000-book-a.db.zip",
        BackupKind::Auto,
    );
    assert!(
        super::engine::is_managed_backup_file_name(path.file_name().unwrap().to_str().unwrap()),
        "文件名为受管命名"
    );
    let names: Vec<String> = {
        let mut archive = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(archive.len(), 3);
        (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
            .collect()
    };
    assert!(names.contains(&"ledger.db".to_string()));
    assert!(names.contains(&"sync.db".to_string()));
    assert!(names.contains(&"backup.json".to_string()));
    let read_meta = |path: &Path| -> serde_json::Value {
        let mut archive = zip::ZipArchive::new(File::open(path).unwrap()).unwrap();
        let mut entry = archive.by_name("backup.json").unwrap();
        let mut buf = String::new();
        entry.read_to_string(&mut buf).unwrap();
        serde_json::from_str(&buf).unwrap()
    };
    let meta = read_meta(&path);
    assert_eq!(meta["paired"], true, "真实成对产物");
    assert_eq!(meta["kind"], "auto");

    let injected = real_managed_backup_with_meta(
        &dir,
        "ledger-backup-20260102-000000.db.zip",
        BackupKind::Manual,
        &serde_json::json!({"fingerprint": "fp-1", "device_id": "dev-1"}),
    );
    let meta = read_meta(&injected);
    assert_eq!(meta["fingerprint"], "fp-1");
    assert_eq!(meta["device_id"], "dev-1");
    assert_eq!(meta["paired"], true, "注入不破坏既有字段");
    assert_eq!(meta["kind"], "manual");
}

#[test]
fn backup_creates_zip_with_db_and_meta() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    let target = temp_file("zip");
    let result = backup_db_to(&conn, &target, "0.2.0", BackupKind::Manual).unwrap();

    assert!(target.exists());
    assert!(result.size_bytes > 0);
    assert!(result.schema_version >= 4);

    let file = File::open(&target).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), 3);
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().unwrap().to_string())
        .collect();
    assert!(names.contains(&"ledger.db".to_string()));
    assert!(names.contains(&"sync.db".to_string()));
    assert!(names.contains(&"backup.json".to_string()));
    let meta: serde_json::Value = {
        let mut entry = archive.by_name("backup.json").unwrap();
        let mut buf = String::new();
        entry.read_to_string(&mut buf).unwrap();
        serde_json::from_str(&buf).unwrap()
    };
    assert_eq!(
        meta["paired"], true,
        "工厂世界四表住 attached 侧，paired=true"
    );

    let extracted = temp_file("extracted");
    let mut db_entry = archive.by_name("ledger.db").unwrap();
    let mut out = File::create(&extracted).unwrap();
    std::io::copy(&mut db_entry, &mut out).unwrap();
    drop(out);
    let db_conn = open_connection(&extracted).unwrap();
    assert_eq!(count_transactions(&db_conn), 1);
}

#[test]
fn restore_roundtrip_preserves_data() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();

    let backup = temp_file("rt-backup");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    let db_path = temp_file("rt-db");
    {
        let c = tauri_app_lib::test_support::open();
        seed(&c);
        c.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('txn-2','expense',999,'CNY',999,'acc-1','2026-03-01','2026-03-01T00:00:00Z','2026-03-01T00:00:00Z',1,'test',0)",
            [],
        )
        .unwrap();
        c.execute("VACUUM INTO ?1", params![db_path.to_string_lossy()])
            .unwrap();
        assert_eq!(count_transactions(&c), 2);
    }

    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, None).unwrap();
    assert_eq!(result.schema_version, expected);

    let c = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&c), 1);
    let cached: i64 = c
        .query_row(
            "SELECT balance_cents FROM account_balance_cache WHERE account_id='acc-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        cached,
        ledger_accounts::balance::compute_balance(&c, "acc-1").unwrap(),
        "恢复后缓存行应与实时计算一致"
    );
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
}

#[test]
fn restore_rejects_newer_schema() {
    let newer = temp_file("newer");
    {
        let c = open_connection(&newer).unwrap();
        c.execute_batch("PRAGMA user_version = 999").unwrap();
    }
    let db_path = temp_file("db");
    let expected = expected_schema_version().unwrap();
    let tmp_dir = ScratchDir::new("backup-test-restore-parent");
    let err = restore_db_from(&newer, &db_path, &tmp_dir, expected, None)
        .unwrap_err()
        .to_string();
    assert!(err.contains("更高版本"), "错误信息: {err}");
    assert!(!db_path.exists(), "恢复应被拒绝，不产生目标库");
}

#[test]
fn restore_supports_bare_db() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    let bare = temp_file("bare");
    conn.execute("VACUUM INTO ?1", params![bare.to_string_lossy()])
        .unwrap();

    let db_path = temp_file("db2");
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    restore_db_from(&bare, &db_path, &safety_dir, expected, None).unwrap();
    let c = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&c), 1);
}

#[test]
fn backup_meta_records_kind_for_auto_and_manual() {
    let conn = tauri_app_lib::test_support::open();
    let manual = temp_file("meta-manual");
    backup_db_to(&conn, &manual, "0.2.0", BackupKind::Manual).unwrap();
    assert_eq!(read_backup_kind(&manual).unwrap(), BackupKind::Manual);

    let auto = temp_file("meta-auto");
    backup_db_to(&conn, &auto, "0.2.0", BackupKind::Auto).unwrap();
    assert_eq!(read_backup_kind(&auto).unwrap(), BackupKind::Auto);
}

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
}

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
}

#[test]
fn legacy_backup_restores_and_lists_without_error() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    let raw = temp_file("legacy-raw");
    conn.execute("VACUUM INTO ?1", params![raw.to_string_lossy()])
        .unwrap();
    let dir = ScratchDir::new("backup-test-legacy-dir");
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
    let list = list_managed_backups(&dir, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        read_backup_kind(Path::new(&list[0].path)).unwrap(),
        BackupKind::Manual
    );
    assert_eq!(list[0].kind, BackupKind::Manual);

    let db_path = temp_file("legacy-restore-db");
    let safety_dir = temp_safety_dir();
    let result = restore_db_from(
        &legacy,
        &db_path,
        &safety_dir,
        expected_schema_version().unwrap(),
        None,
    );
    assert!(result.is_ok(), "旧格式备份恢复失败: {:?}", result.err());
    let c = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&c), 1);
}

#[test]
fn list_and_prune_managed_backups() {
    let dir = ScratchDir::new("backup-test-managed");
    real_managed_backup(
        &dir,
        "ledger-backup-20260101-010101.db.zip",
        BackupKind::Manual,
    );
    real_managed_backup(
        &dir,
        "ledger-backup-20260102-010101.db.zip",
        BackupKind::Manual,
    );
    real_managed_backup(
        &dir,
        "ledger-backup-20260103-010101.db.zip",
        BackupKind::Manual,
    );
    real_managed_backup(&dir, "ledger-auto-20260201-010101.db.zip", BackupKind::Auto);
    std::fs::write(dir.join("notes.zip"), b"x").unwrap();
    std::fs::create_dir(dir.join("ledger-backup-20260104-010101.db.zip")).unwrap();

    let list = list_managed_backups(&dir, None).unwrap();
    assert_eq!(list.len(), 4);
    assert_eq!(
        list[0].file_name, "ledger-auto-20260201-010101.db.zip",
        "auto 前缀同样受管且按时间排序"
    );
    assert_eq!(list[0].created_at, "2026-02-01T01:01:01");
    assert_eq!(list[0].kind, BackupKind::Auto, "真实产物元数据读取");
    assert_eq!(list[3].file_name, "ledger-backup-20260101-010101.db.zip");
    assert_eq!(list[3].kind, BackupKind::Manual);

    let r = prune_managed_backups(&dir, 2, None).unwrap();
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

    let r2 = prune_managed_backups(&dir, 1, None).unwrap();
    assert_eq!(r2.deleted, vec!["ledger-backup-20260103-010101.db.zip"]);
    assert_eq!(r2.kept, 1);
    assert!(dir.join("ledger-auto-20260201-010101.db.zip").exists());
}

#[test]
fn unreadable_products_list_with_fallback_and_never_pruned() {
    let dir = ScratchDir::new("backup-test-unreadable");
    std::fs::write(dir.join("ledger-backup-20260101-010101.db.zip"), b"x").unwrap();
    std::fs::write(
        dir.join("ledger-auto-20260201-010101.db.zip"),
        vec![0u8; 40],
    )
    .unwrap();

    let list = list_managed_backups(&dir, None).unwrap();
    assert_eq!(list.len(), 2);
    assert_eq!(list[0].file_name, "ledger-auto-20260201-010101.db.zip");
    assert_eq!(list[0].kind, BackupKind::Auto);
    assert_eq!(list[1].file_name, "ledger-backup-20260101-010101.db.zip");
    assert_eq!(list[1].kind, BackupKind::Manual);

    let r = prune_managed_backups(&dir, 1, None).unwrap();
    assert!(r.deleted.is_empty(), "来源未知的残缺包保守不删");
    assert_eq!(r.kept, 0, "本桶为空：kept 报告本桶保留数而非目录总数");
    assert!(dir.join("ledger-backup-20260101-010101.db.zip").exists());
    assert!(dir.join("ledger-auto-20260201-010101.db.zip").exists());
}

#[test]
fn prune_keeps_all_when_within_limit_and_missing_dir() {
    let dir = ScratchDir::new("backup-test-prune-none");
    std::fs::write(dir.join("ledger-backup-20260101-010101.db.zip"), b"x").unwrap();

    let r = prune_managed_backups(&dir, 30, None).unwrap();
    assert!(r.deleted.is_empty());
    assert_eq!(r.kept, 0, "残缺包不进桶：kept 报告本桶保留数");

    let missing = dir.join("gone");
    assert!(list_managed_backups(&missing, None).unwrap().is_empty());
    let r2 = prune_managed_backups(&missing, 5, None).unwrap();
    assert_eq!(r2.kept, 0);
    assert!(r2.deleted.is_empty());
}

#[test]
fn backup_fails_when_target_dir_missing() {
    let conn = tauri_app_lib::test_support::open();
    let missing_root = ScratchDir::new("backup-test-missing-target");
    let target = missing_root.join("gone").join("x.zip");
    let err = backup_db_to(&conn, &target, "0.2.0", BackupKind::Manual)
        .unwrap_err()
        .to_string();
    assert!(err.contains("备份目标目录不存在"));
}

fn assert_no_temp_residue(dir: &Path) {
    let leftovers: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(
        leftovers.is_empty(),
        "目录内不得留下临时残留: {leftovers:?}"
    );
}

#[test]
fn backup_failure_leaves_no_temp_residue() {
    let dir = ScratchDir::new("backup-test-residue");

    let conn = tauri_app_lib::test_support::open();
    conn.execute_batch("BEGIN").unwrap();
    let target = dir.join("ledger-backup-20260918-120000.db.zip");
    backup_db_to(&conn, &target, "0.6.0", BackupKind::Manual).unwrap_err();
    assert_no_temp_residue(&dir);
    conn.execute_batch("ROLLBACK").unwrap();

    let occupied = dir.join("occupied.zip");
    std::fs::create_dir(&occupied).unwrap();
    backup_db_to(&conn, &occupied, "0.6.0", BackupKind::Manual).unwrap_err();
    assert_no_temp_residue(&dir);
}

#[test]
fn backup_rejects_foreign_form_db_with_coded_error() {
    let dir = ScratchDir::new("backup-test-foreign");
    let src = dir.join("ledger.db");
    ledger_infra::test_utils::write_foreign_form_plaintext_db(&src, 3);
    let conn = Connection::open(&src).unwrap();

    let target = dir.join("ledger-auto-20260918-120000-book-a.db.zip");
    let err = backup_db_to(&conn, &target, "0.6.0", BackupKind::Auto).unwrap_err();

    assert_eq!(err.code(), Some("backup.foreign-form"));
    let shown = err.to_string();
    assert!(
        !shown.contains("unable to open database"),
        "用户可见错误不得是 SQLite 原文: {shown}"
    );
    assert!(shown.contains("重启应用"), "文案应指向修复动作: {shown}");
    assert!(!target.exists(), "不应产生备份产物");
    assert_no_temp_residue(&dir);
}

#[test]
fn backup_of_app_owned_db_succeeds_with_clean_form() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    let target = temp_file("form");
    backup_db_to(&conn, &target, "0.6.0", BackupKind::Manual).unwrap();

    let mut header = [0u8; 21];
    let file = File::open(&target).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name("ledger.db").unwrap();
    std::io::Read::read_exact(&mut entry, &mut header).unwrap();
    assert_eq!(header[20], 0, "产物库偏移 20 应为 0（应用自有形态）");
}

#[test]
fn backup_of_encrypted_db_bypasses_form_guard_and_stays_encrypted() {
    let dir = ScratchDir::new("backup-test-enc");
    let src = dir.join("ledger.db");
    {
        let conn = tauri_app_lib::test_support::open();
        conn.execute("VACUUM INTO ?1", params![src.to_string_lossy()])
            .unwrap();
        conn.execute(
            "VACUUM sync INTO ?1",
            params![dir.join(db::SYNC_DB_FILE_NAME).to_string_lossy()],
        )
        .unwrap();
    }
    ledger_infra::db::encryption::enable_encryption_for_file(&src, "pass-phrase-123").unwrap();
    let conn = ledger_infra::db::open_connection_with_passphrase(&src, "pass-phrase-123").unwrap();

    let target = dir.join("ledger-backup-20260918-120000.db.zip");
    backup_db_to(&conn, &target, "0.6.0", BackupKind::Manual).unwrap();

    let extracted = dir.join("extracted.db");
    let file = File::open(&target).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name("ledger.db").unwrap();
    let mut out = File::create(&extracted).unwrap();
    std::io::copy(&mut entry, &mut out).unwrap();
    drop(out);
    assert_eq!(
        ledger_infra::db::encryption::probe_file_kind(&extracted).unwrap(),
        ledger_infra::db::encryption::DbFileKind::Encrypted,
        "备份产物应为密文库（继承密钥）"
    );
}

#[test]
fn managed_name_roundtrip_with_book_tag() {
    let cases: Vec<(String, &str, Option<&str>)> = vec![
        (
            "ledger-backup-20260909-123456.db.zip".into(),
            "ledger-backup",
            None,
        ),
        (
            "ledger-auto-20260909-123456.db.zip".into(),
            "ledger-auto",
            None,
        ),
        (
            "ledger-backup-20260909-123456-3f2a9c4e-8b1d-4c2a-9f3e-5a7b8c9d0e1f.db.zip".into(),
            "ledger-backup",
            Some("3f2a9c4e-8b1d-4c2a-9f3e-5a7b8c9d0e1f"),
        ),
        (
            "ledger-auto-20260909-123456-ab12cd34ef56ab12.db.zip".into(),
            "ledger-auto",
            Some("ab12cd34ef56ab12"),
        ),
    ];
    for (name, _prefix, book) in cases {
        let path = Path::new(&name);
        assert_eq!(
            super::engine::split_managed_name(&name).map(|(ts, book)| (
                ts.format("%Y%m%d-%H%M%S").to_string(),
                book.map(str::to_string)
            )),
            Some(("20260909-123456".into(), book.map(str::to_string))),
            "{name}"
        );
        assert!(super::engine::is_managed_backup_file_name(&name), "{name}");
        let _ = path;
    }
    for name in [
        "other-20260909-123456.db.zip",
        "ledger-backup-not-a-timestamp.db.zip",
        "ledger-backup-short.db.zip",
        "ledger-backup-20260909-123456.db",
    ] {
        assert_eq!(
            super::engine::split_managed_name(name),
            None,
            "{name} 不应解析"
        );
    }
}

fn scope_of_fixed_fingerprint(conn: &Connection) -> BackupScope {
    ledger_infra::settings::set(
        conn,
        ledger_infra::settings::SettingKey::BookFingerprint,
        &"txn-1".to_string(),
    )
    .unwrap();
    BackupScope::of_conn(conn)
        .unwrap()
        .expect("固化指纹在场应可构造作用域")
}

#[test]
fn listing_attributes_products_by_fingerprint_across_devices() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let own_device = scope.device_id.clone().expect("真实设备标识在场");
    let dir = ScratchDir::new("backup-test-fp-list");
    real_managed_backup_with_meta(
        &dir,
        "ledger-auto-20260103-000000-txn-1.db.zip",
        BackupKind::Auto,
        &serde_json::json!({ "device_id": own_device }),
    );
    real_managed_backup_with_meta(
        &dir,
        "ledger-backup-20260102-000000-ab12cd34ef56ab12.db.zip",
        BackupKind::Manual,
        &serde_json::json!({ "device_id": "dev-other" }),
    );
    real_managed_backup_with_meta(
        &dir,
        "ledger-backup-20260104-000000-cd98ef01ab23cd45.db.zip",
        BackupKind::Manual,
        &serde_json::json!({ "fingerprint": "fp-other-book" }),
    );
    real_managed_backup_with_meta(
        &dir,
        "ledger-backup-20260101-000000.db.zip",
        BackupKind::Manual,
        &serde_json::json!({ "fingerprint": null, "device_id": null }),
    );

    let list = list_managed_backups(&dir, Some(&scope)).unwrap();

    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(
        names,
        vec![
            "ledger-auto-20260103-000000-txn-1.db.zip",
            "ledger-backup-20260102-000000-ab12cd34ef56ab12.db.zip",
            "ledger-backup-20260101-000000.db.zip",
        ],
        "两台设备的同账产物与内容锚归并的旧产物合并可见（新→旧），别本不进列表"
    );
    assert_eq!(list[2].source_device_id, None);
    assert!(list[2].source_unknown);
    assert_eq!(
        list[0].source_device_id.as_deref(),
        Some(own_device.as_str())
    );
    assert!(!list[0].source_unknown);
    assert_eq!(list[1].source_device_id.as_deref(), Some("dev-other"));
    assert!(!list[1].source_unknown);

    assert_eq!(list_managed_backups(&dir, None).unwrap().len(), 4);
}

#[test]
fn attributed_product_without_device_watermark_lists_as_unknown_source() {
    let dir = ScratchDir::new("backup-test-fp-unknown-dev");
    real_managed_backup_with_meta(
        &dir,
        "ledger-backup-20260102-000000-ab12cd34ef56ab12.db.zip",
        BackupKind::Manual,
        &serde_json::json!({ "fingerprint": "txn-1", "device_id": null }),
    );
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    assert_eq!(list.len(), 1, "指纹命中即归属，设备水印缺失不影响可见性");
    assert_eq!(list[0].source_device_id, None, "无设备水印");
    assert!(list[0].source_unknown, "来源标记为未知");
}

use super::attribution::CACHE_FILE_NAME;

fn legacy_product_with_anchor(dir: &Path, file_name: &str) -> PathBuf {
    real_managed_backup_with_meta(
        dir,
        file_name,
        BackupKind::Manual,
        &serde_json::json!({ "fingerprint": null, "device_id": null }),
    )
}

fn encrypted_legacy_product(
    dir: &Path,
    file_name: &str,
    extra_meta: &serde_json::Value,
) -> PathBuf {
    let work = ScratchDir::new("backup-test-enc-legacy-src");
    let src = work.join("ledger.db");
    {
        let conn = tauri_app_lib::test_support::open();
        seed(&conn);
        conn.execute("VACUUM INTO ?1", params![src.to_string_lossy()])
            .unwrap();
        conn.execute(
            "VACUUM sync INTO ?1",
            params![work.join(db::SYNC_DB_FILE_NAME).to_string_lossy()],
        )
        .unwrap();
    }
    ledger_infra::db::encryption::enable_encryption_for_file(&src, "pass-phrase-123").unwrap();
    let conn = ledger_infra::db::open_connection_with_passphrase(&src, "pass-phrase-123").unwrap();
    let target = dir.join(file_name);
    backup_db_to(&conn, &target, "0.2.0", BackupKind::Manual).unwrap();
    if extra_meta.as_object().is_some_and(|o| !o.is_empty()) {
        merge_extra_backup_meta(&target, extra_meta);
    }
    target
}

#[test]
fn field_isomorphic_products_all_visible_and_attributed() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let own_device = scope.device_id.clone().expect("真实设备标识在场");
    let dir = ScratchDir::new("backup-test-field-34");

    let mut expected: Vec<String> = Vec::new();
    let own = real_managed_backup_with_meta(
        &dir,
        "ledger-auto-20260301-000000-txn-1.db.zip",
        BackupKind::Auto,
        &serde_json::json!({ "device_id": own_device }),
    );
    let _ = own;
    expected.push("ledger-auto-20260301-000000-txn-1.db.zip".into());
    for day in (1..=4).rev() {
        let name = format!("ledger-backup-2026020{day}-000000.db.zip");
        legacy_product_with_anchor(&dir, &name);
        expected.push(name);
    }
    for day in (1..=29).rev() {
        let name = format!("ledger-backup-202601{day:02}-000000-handle-mac-{day:02}.db.zip");
        legacy_product_with_anchor(&dir, &name);
        expected.push(name);
    }

    let list = list_managed_backups(&dir, Some(&scope)).unwrap();

    assert_eq!(list.len(), 34, "29 异端 + 4 无标识 + 1 本机全部可见");
    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(names, expected, "列表新→旧完整且归属本账本作用域");
    assert_eq!(
        list[0].source_device_id.as_deref(),
        Some(own_device.as_str())
    );
    assert!(!list[0].source_unknown);
    for f in &list[1..] {
        assert_eq!(f.source_device_id, None, "{}", f.file_name);
        assert!(f.source_unknown, "{}", f.file_name);
    }
}

#[test]
fn encrypted_products_attribute_by_watermark_or_land_unknown() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let dir = ScratchDir::new("backup-test-enc-unknown");

    let watermarked = encrypted_legacy_product(
        &dir,
        "ledger-backup-20260101-000000-handle-mac.db.zip",
        &serde_json::json!({ "device_id": null }),
    );
    assert!(
        read_meta_json(&watermarked)["encrypted"].as_bool().unwrap(),
        "前置：产物为密文形态"
    );
    let anonymous = encrypted_legacy_product(
        &dir,
        "ledger-backup-20260102-000000-handle-mac.db.zip",
        &serde_json::json!({ "fingerprint": null, "device_id": null }),
    );
    let _ = anonymous;

    let list = list_managed_backups(&dir, Some(&scope)).unwrap();

    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(
        names,
        vec![
            "ledger-backup-20260102-000000-handle-mac.db.zip",
            "ledger-backup-20260101-000000-handle-mac.db.zip",
        ],
        "密文带指纹经水印归并，密文无指纹落未知来源桶——两者都可见"
    );
    for f in &list {
        assert!(f.source_unknown, "{}", f.file_name);
        assert!(f.encrypted, "{} 锁形标记保留", f.file_name);
    }
}

#[test]
fn unknown_source_bucket_visible_but_never_pruned() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let own_device = scope.device_id.clone().expect("真实设备标识在场");
    let dir = ScratchDir::new("backup-test-unknown-prune");

    for ts in ["20260101", "20260102"] {
        real_managed_backup_with_meta(
            &dir,
            &format!("ledger-auto-{ts}-000000-txn-1.db.zip"),
            BackupKind::Auto,
            &serde_json::json!({ "device_id": own_device }),
        );
    }
    let encrypted_unknown = encrypted_legacy_product(
        &dir,
        "ledger-backup-20251230-000000-handle-mac.db.zip",
        &serde_json::json!({ "fingerprint": null, "device_id": null }),
    );
    let _ = encrypted_unknown;
    let broken = dir.join("ledger-backup-20251231-000000.db.zip");
    std::fs::write(&broken, b"x").unwrap();

    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    assert_eq!(list.len(), 4, "未知来源桶产物照常出现在列表");
    assert!(
        list.iter().filter(|f| f.source_device_id.is_none()).count() == 2,
        "未知来源桶产物标注来源未知"
    );

    let r = prune_managed_backups(&dir, 1, Some(&scope)).unwrap();
    assert_eq!(
        r.deleted,
        vec!["ledger-auto-20260101-000000-txn-1.db.zip"],
        "只摘本机设备名下的最旧产物"
    );
    assert!(
        dir.join("ledger-backup-20251230-000000-handle-mac.db.zip")
            .exists(),
        "密文无指纹产物（未知来源桶）不被清理"
    );
    assert!(broken.exists(), "残缺包（未知来源桶）不被清理");
    assert_eq!(r.kept, 1, "kept 报告本桶保留数；未知来源产物不在桶内");
}

#[test]
fn same_timestamp_foreign_products_never_cross_attributed() {
    let conn_a = tauri_app_lib::test_support::open();
    seed(&conn_a);
    let scope_a = scope_of_fixed_fingerprint(&conn_a);
    let dir = ScratchDir::new("backup-test-multi-book");

    let product_a =
        legacy_product_with_anchor(&dir, "ledger-backup-20260101-000000-handle-a.db.zip");
    let _ = product_a;
    {
        let conn_b = tauri_app_lib::test_support::open();
        tauri_app_lib::test_support::seed_account(&conn_b, "acc-1", "现金", "cash", "CNY", 0);
        conn_b.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES ('txn-b','expense',9900,'CNY',9900,'acc-1','2026-01-01',?1,?1,1,'test',0)",
            [tauri_app_lib::test_support::FIXED_NOW],
        )
        .unwrap();
        let path_b = dir.join("ledger-backup-20260101-000000-handle-b.db.zip");
        backup_db_to(&conn_b, &path_b, "0.2.0", BackupKind::Manual).unwrap();
        merge_extra_backup_meta(
            &path_b,
            &serde_json::json!({ "fingerprint": null, "device_id": null }),
        );
        assert_eq!(book_fingerprint(&conn_b).unwrap(), "txn-b");
    }

    let list_a = list_managed_backups(&dir, Some(&scope_a)).unwrap();
    let names_a: Vec<String> = list_a.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(
        names_a,
        vec!["ledger-backup-20260101-000000-handle-a.db.zip"],
        "账本 A 只见内容锚命中的产物"
    );

    let conn_b = tauri_app_lib::test_support::open();
    ledger_infra::settings::set(
        &conn_b,
        ledger_infra::settings::SettingKey::BookFingerprint,
        &"txn-b".to_string(),
    )
    .unwrap();
    let scope_b = BackupScope::of_conn(&conn_b)
        .unwrap()
        .expect("固化指纹在场");
    let list_b = list_managed_backups(&dir, Some(&scope_b)).unwrap();
    let names_b: Vec<String> = list_b.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(
        names_b,
        vec!["ledger-backup-20260101-000000-handle-b.db.zip"],
        "异端产物不得归错账本（同名时间戳、不同指纹各归各账）"
    );
}

#[test]
fn attribution_cache_hits_without_repack_and_rebuilds_on_change() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let dir = ScratchDir::new("backup-test-anchor-cache");
    let name = "ledger-backup-20260101-000000-handle-mac.db.zip";
    let product = legacy_product_with_anchor(&dir, name);
    let cache_path = dir.join(CACHE_FILE_NAME);

    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(names, vec![name], "首次判定解包归并");
    assert!(cache_path.exists(), "判定结果缓存到独立本地文件");

    let rewrite_cache = |content_anchor: &str| {
        let mut cache: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cache_path).unwrap()).unwrap();
        cache["entries"][name]["content_anchor"] = serde_json::json!(content_anchor);
        std::fs::write(&cache_path, cache.to_string()).unwrap();
    };
    rewrite_cache("txn-other");
    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    assert!(list.is_empty(), "命中缓存的判定结论被复用（不再解包）");

    rewrite_cache("txn-1");
    let shifted = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_760_000_000);
    std::fs::File::options()
        .write(true)
        .open(&product)
        .unwrap()
        .set_modified(shifted)
        .unwrap();
    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(names, vec![name], "缓存因修改时间变化失效，重算恢复归并");

    std::fs::write(&cache_path, b"not-json{").unwrap();
    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(names, vec![name], "缓存文件损坏不阻断列表");

    std::fs::remove_file(&cache_path).unwrap();
    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(names, vec![name], "缓存可整份丢弃，判定可从产物重建");
    assert!(cache_path.exists(), "列表完成后缓存重新落盘");

    let all = list_managed_backups(&dir, None).unwrap();
    assert_eq!(all.len(), 1, "缓存文件不进受管列表");
}

#[test]
fn content_anchor_merge_scans_device_subdirs() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let dir = ScratchDir::new("backup-test-anchor-dual");
    let device_dir = dir.join("01971234-4567-789a-bcde-f123456789ab");
    std::fs::create_dir_all(&device_dir).unwrap();

    let name = "ledger-backup-20260101-000000-handle-mac.db.zip";
    legacy_product_with_anchor(&dir, name);
    legacy_product_with_anchor(&device_dir, name);

    let list = list_managed_backups(&dir, Some(&scope)).unwrap();
    let names: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
    assert_eq!(
        names,
        vec![name, name],
        "设备子目录内的异端产物同样经内容锚归并，两处同名产物都可见"
    );
    assert!(list.iter().all(|f| f.source_unknown), "均为升级前形态");
}

#[test]
fn content_anchor_earliest_follows_transaction_row_order() {
    let conn = tauri_app_lib::test_support::open();
    tauri_app_lib::test_support::seed_account(&conn, "acc-1", "现金", "cash", "CNY", 0);
    let insert = |id: &str, date: &str, created_at: &str, deleted: i64| {
        conn.execute(
            "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
             VALUES (?1,'expense',1500,'CNY',1500,'acc-1',?2,?3,?3,1,'test',?4)",
            params![id, date, created_at, deleted],
        )
        .unwrap();
    };
    insert("txn-w", "2026-01-01", "2026-06-01T00:00:00Z", 0);
    insert("txn-w2", "2026-01-01", "2026-06-01T00:00:00Z", 0);
    insert(
        "txn-x",
        "2026-05-01",
        tauri_app_lib::test_support::FIXED_NOW,
        0,
    );
    insert(
        "txn-y",
        "2025-01-01",
        tauri_app_lib::test_support::FIXED_NOW,
        1,
    );

    assert_eq!(
        super::identity::content_anchor(&conn).unwrap(),
        Some("txn-w".to_string()),
        "内容锚 = 按交易域行序（date → created_at → id）排除软删后的最早一笔"
    );
}

#[test]
fn scope_of_conn_reads_fixed_fingerprint_or_anchor() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    assert_eq!(scope.fingerprint, "txn-1");
    assert!(scope.device_id.is_some(), "设备标识随作用域一并给出");

    let conn = tauri_app_lib::test_support::open();
    seed(&conn);
    let scope = BackupScope::of_conn(&conn).unwrap().expect("锚在场");
    assert_eq!(scope.fingerprint, "txn-1");

    let empty = tauri_app_lib::test_support::open();
    assert!(BackupScope::of_conn(&empty).unwrap().is_none());
}

#[test]
fn scoped_prune_never_touches_other_device_products() {
    let conn = tauri_app_lib::test_support::open();
    let scope = scope_of_fixed_fingerprint(&conn);
    let own_device = scope.device_id.clone().expect("真实设备标识在场");
    let dir = ScratchDir::new("backup-test-fp-prune");
    for ts in ["20260101", "20260102", "20260103"] {
        real_managed_backup_with_meta(
            &dir,
            &format!("ledger-auto-{ts}-000000-txn-1.db.zip"),
            BackupKind::Auto,
            &serde_json::json!({ "device_id": &own_device }),
        );
    }
    for ts in ["20260101", "20260102", "20260103"] {
        real_managed_backup_with_meta(
            &dir,
            &format!("ledger-auto-{ts}-010000-txn-1.db.zip"),
            BackupKind::Auto,
            &serde_json::json!({ "device_id": "dev-other" }),
        );
    }
    real_managed_backup_with_meta(
        &dir,
        "ledger-auto-20260102-020000-fp-b.db.zip",
        BackupKind::Auto,
        &serde_json::json!({ "fingerprint": "fp-other-book" }),
    );

    let r = prune_managed_backups(&dir, 1, Some(&scope)).unwrap();

    assert_eq!(
        r.deleted,
        vec![
            "ledger-auto-20260101-000000-txn-1.db.zip",
            "ledger-auto-20260102-000000-txn-1.db.zip",
        ],
        "只删本机设备名下的最旧产物"
    );
    assert_eq!(r.kept, 1, "kept 报告本桶清理后保留数（= 上限）");
    assert!(
        r.failed.is_empty(),
        "清理结果仍上报 kept/deleted/failed 三元（failed 空场）"
    );
    for ts in ["20260101", "20260102", "20260103"] {
        assert!(
            dir.join(format!("ledger-auto-{ts}-010000-txn-1.db.zip"))
                .exists(),
            "另一台设备的产物不被本机清理（{ts}）"
        );
    }
    assert!(
        dir.join("ledger-auto-20260102-020000-fp-b.db.zip").exists(),
        "别本账产物不受影响"
    );
    assert!(
        dir.join("ledger-auto-20260103-000000-txn-1.db.zip")
            .exists(),
        "本桶内最新一份留存"
    );
}

fn pre_split_world_file(path: &Path) {
    let mut conn = db::open_connection_unmounted(path).expect("别名裸连接");
    db::migrations()
        .to_version(&mut conn, (db::SYNC_SPLIT_USER_VERSION - 1) as usize)
        .expect("停在拆库前");
    conn.execute(
        "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) \
         VALUES ('dev-1', 5, '2026-05-01T00:00:00Z', '2026-05-01T00:00:00Z')",
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO sync_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at) \
         VALUES ('op-1', 'dev-1', 1, 34, 'transaction', 'txn-1', '{\"k\":1}', '2026-05-01T00:00:00Z')",
        [],
    )
    .unwrap();
}

#[test]
fn restore_moves_stale_sync_db_aside() {
    let dir = ScratchDir::new("backup-test-restore-sync");
    let db_path = dir.join("ledger.db");
    pre_split_world_file(&db_path);
    {
        let conn = db::open_connection_unmounted(&db_path).unwrap();
        seed(&conn);
    }
    let sync_path = dir.join(db::SYNC_DB_FILE_NAME);
    {
        let sync = open_connection(&sync_path).unwrap();
        sync.execute("CREATE TABLE restore_probe(x)", []).unwrap();
    }
    let backup = temp_file("rt-sync-backup");
    {
        let conn = open_connection(&db_path).unwrap();
        backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();
    }

    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    restore_db_from(&backup, &db_path, &safety_dir, expected, None).unwrap();

    let entries: Vec<String> = std::fs::read_dir(&safety_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    let sync_safety_name = entries
        .iter()
        .find(|n| n.starts_with("restore-safety-") && n.contains("sync"))
        .expect("安全目录应保留 sync.db 副本")
        .clone();
    let moved = open_connection(safety_dir.join(&sync_safety_name)).unwrap();
    moved
        .query_row::<i64, _, _>("SELECT count(*) FROM restore_probe", [], |r| r.get(0))
        .expect("移位副本应为原位世界的 sync.db（含探针表）");
    let conn = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&conn), 1);
    conn.query_row::<i64, _, _>("SELECT count(*) FROM sync.sqlite_master", [], |r| r.get(0))
        .expect("恢复后世界应可正常挂载 sync.db");
}

fn make_dual_db_world(dir: &Path, sync_rows: bool) -> (std::path::PathBuf, std::path::PathBuf) {
    let main_path = dir.join("ledger.db");
    let sync_path = dir.join("sync.db");
    let factory = tauri_app_lib::test_support::open();
    seed(&factory);
    if sync_rows {
        factory
            .execute(
                "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) \
                 VALUES ('dev-src', 7, ?1, ?1)",
                [tauri_app_lib::test_support::FIXED_NOW],
            )
            .unwrap();
        factory
            .execute(
                "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
                 VALUES ('dev-a', 3, ?1)",
                [tauri_app_lib::test_support::FIXED_NOW],
            )
            .unwrap();
    }
    factory
        .execute("VACUUM INTO ?1", params![main_path.to_string_lossy()])
        .unwrap();
    factory
        .execute("VACUUM sync INTO ?1", params![sync_path.to_string_lossy()])
        .unwrap();
    (main_path, sync_path)
}

fn read_meta_json(path: &Path) -> serde_json::Value {
    let file = File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name("backup.json").unwrap();
    let mut buf = String::new();
    entry.read_to_string(&mut buf).unwrap();
    serde_json::from_str(&buf).unwrap()
}
fn assert_sync_alias_attached(conn: &Connection, why: &str) {
    let mounted: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_database_list WHERE name = 'sync'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(mounted, 1, "{why}");
}

fn plant_stale_sync_db(dst_dir: &Path) -> std::path::PathBuf {
    let dst_sync = dst_dir.join("sync.db");
    let conn = open_connection(&dst_sync).unwrap();
    conn.execute("CREATE TABLE stale_probe(x)", []).unwrap();
    dst_sync
}

#[test]
fn paired_backup_restores_sync_db_when_paired() {
    let src_dir = ScratchDir::new("backup-test-paired-src");
    let (main_path, _sync_path) = make_dual_db_world(&src_dir, true);
    let conn = open_connection(&main_path).unwrap();
    assert_sync_alias_attached(&conn, "前置：挂载接线在位（attached 侧就绪）");

    let backup = temp_file("paired-backup");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    let meta = read_meta_json(&backup);
    assert_eq!(meta["paired"], true, "双库世界的产物标记为成对");
    let sync_entry_guard = temp_file("paired-sync-entry");
    let sync_entry = sync_entry_guard.path().to_path_buf();
    {
        let file = File::open(&backup).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut entry = archive.by_name("sync.db").unwrap();
        let mut out = File::create(&sync_entry).unwrap();
        std::io::copy(&mut entry, &mut out).unwrap();
    };
    {
        let sync = db::open_connection_unmounted(&sync_entry).unwrap();
        let dev: String = sync
            .query_row("SELECT id FROM sync_device", [], |r| r.get(0))
            .unwrap();
        assert_eq!(dev, "dev-src", "sync 件携带设备身份");
    }

    let dst_dir = ScratchDir::new("backup-test-paired-dst");
    let dst_main = dst_dir.join("ledger.db");
    {
        let factory = tauri_app_lib::test_support::open();
        factory
            .execute("VACUUM INTO ?1", params![dst_main.to_string_lossy()])
            .unwrap();
    }
    plant_stale_sync_db(&dst_dir);
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    restore_db_from(&backup, &dst_main, &safety_dir, expected, None).unwrap();

    let sync = db::open_connection_unmounted(dst_dir.join("sync.db")).unwrap();
    let dev: String = sync
        .query_row("SELECT id FROM sync_device", [], |r| r.get(0))
        .unwrap();
    assert_eq!(dev, "dev-src", "设备身份随库一致回滚");
    let applied: i64 = sync
        .query_row(
            "SELECT applied_through FROM sync_stream_positions",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(applied, 3, "位点随库一致回滚");
    let restored = open_connection(&dst_main).unwrap();
    assert_eq!(count_transactions(&restored), 1, "业务行照常恢复");
    let moved_aside = std::fs::read_dir(&safety_dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("restore-safety-sync-")
        });
    assert!(moved_aside, "遗留 sync.db 应移入恢复安全备份目录");
    restored
        .query_row::<i64, _, _>("SELECT count(*) FROM sync.sqlite_master", [], |r| r.get(0))
        .expect("恢复后世界应可正常挂载 sync.db（成对产物就位）");
}

#[test]
fn legacy_form_backup_ignores_sync_entry_on_restore() {
    let src = temp_file("legacy-form-src");
    pre_split_world_file(&src);
    {
        let conn = db::open_connection_unmounted(&src).unwrap();
        seed(&conn);
    }
    let backup = temp_file("legacy-form-backup");
    {
        let file = File::create(&backup).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for name in ["ledger.db", "sync.db"] {
            zip.start_file(name, options).unwrap();
            std::io::copy(&mut File::open(&src).unwrap(), &mut zip).unwrap();
        }
        zip.start_file("backup.json", options).unwrap();
        zip.write_all(
            br#"{"created_at":"2026-05-01T00:00:00Z","app_version":"0.5.0","schema_version":35,"kind":"manual","paired":false}"#,
        )
        .unwrap();
        zip.finish().unwrap();
    }
    assert_eq!(read_meta_json(&backup)["paired"], false);

    let dst_dir = ScratchDir::new("backup-test-legacy-form");
    let dst_main = dst_dir.join("ledger.db");
    {
        let factory = tauri_app_lib::test_support::open();
        factory
            .execute("VACUUM INTO ?1", params![dst_main.to_string_lossy()])
            .unwrap();
    }
    plant_stale_sync_db(&dst_dir);
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    restore_db_from(&backup, &dst_main, &safety_dir, expected, None).unwrap();

    let restored = open_connection(&dst_main).unwrap();
    assert_eq!(count_transactions(&restored), 1);
    let synced_tables: i64 = restored
        .query_row(
            "SELECT count(*) FROM sync.sqlite_master WHERE name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get(0),
        )
        .expect("重开后挂载接线应补建可挂载的空 sync.db");
    assert_eq!(
        synced_tables, 0,
        "补建的 sync.db 为空库（备份 sync 条目未落盘）"
    );
    let moved_aside = std::fs::read_dir(&safety_dir)
        .unwrap()
        .filter_map(Result::ok)
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("restore-safety-sync-")
        });
    assert!(moved_aside, "遗留 sync.db 应移入恢复安全备份目录");

    drop(restored);
    let converged = db::open_connection_in(&dst_dir).unwrap();
    assert_eq!(count_transactions(&converged), 1, "业务行应原样保留在主库");
    let ops_in_sync: i64 = converged
        .query_row("SELECT COUNT(*) FROM sync.sync_ops", [], |r| r.get(0))
        .unwrap();
    assert_eq!(ops_in_sync, 1, "op 行应随 V036 搬入 sync.db");
    let device: String = converged
        .query_row("SELECT id FROM sync.sync_device", [], |r| r.get(0))
        .unwrap();
    assert_eq!(device, "dev-1", "设备身份应随 V036 搬入 sync.db");
    let in_main: i64 = converged
        .query_row(
            "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = 'sync_ops'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(in_main, 0, "main 不应残留同步表");
}

#[test]
fn paired_backup_missing_sync_entry_fails_loud() {
    let src_dir = ScratchDir::new("backup-test-paired-missing");
    let (main_path, _sync_path) = make_dual_db_world(&src_dir, true);
    let conn = open_connection(&main_path).unwrap();
    let backup = temp_file("paired-missing-entry");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    let rewritten = temp_file("paired-missing-entry-rewritten");
    {
        let file = File::open(&backup).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let out_file = File::create(&rewritten).unwrap();
        let mut writer = zip::ZipWriter::new(out_file);
        let options = zip::write::SimpleFileOptions::default();
        for name in ["ledger.db", "backup.json"] {
            let mut entry = archive.by_name(name).unwrap();
            writer.start_file(name, options).unwrap();
            std::io::copy(&mut entry, &mut writer).unwrap();
        }
        writer.finish().unwrap();
    }

    let dst_dir = ScratchDir::new("backup-test-paired-missing-dst");
    let dst_main = dst_dir.join("ledger.db");
    {
        let factory = tauri_app_lib::test_support::open();
        factory
            .execute("VACUUM INTO ?1", params![dst_main.to_string_lossy()])
            .unwrap();
        factory
            .execute(
                "VACUUM sync INTO ?1",
                params![dst_dir.join(db::SYNC_DB_FILE_NAME).to_string_lossy()],
            )
            .unwrap();
    }
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    let err = restore_db_from(&rewritten, &dst_main, &safety_dir, expected, None).unwrap_err();
    assert_eq!(err.code(), Some("backup.sync-entry-missing"));
    let intact = open_connection(&dst_main).unwrap();
    assert_eq!(count_transactions(&intact), 0, "目标世界未被恢复改动");
}

#[test]
fn paired_backup_encrypted_roundtrip_restores_sync_identity() {
    use ledger_infra::db::encryption::{DbFileKind, enable_encryption_for_file, probe_file_kind};

    let src_dir = ScratchDir::new("backup-test-paired-enc-src");
    let (main_path, _sync_path) = make_dual_db_world(&src_dir, true);
    enable_encryption_for_file(&main_path, "paired-pass").unwrap();
    let conn = db::open_connection_with_passphrase(&main_path, "paired-pass").unwrap();
    assert_sync_alias_attached(&conn, "前置：密文双库世界挂载就绪");

    let backup = temp_file("paired-enc-backup");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    assert_eq!(read_meta_json(&backup)["paired"], true);
    for entry in ["ledger.db", "sync.db"] {
        let extracted = temp_file("paired-enc-entry");
        {
            let file = File::open(&backup).unwrap();
            let mut archive = zip::ZipArchive::new(file).unwrap();
            let mut zip_entry = archive.by_name(entry).unwrap();
            let mut out = File::create(extracted.path()).unwrap();
            std::io::copy(&mut zip_entry, &mut out).unwrap();
        }
        assert_eq!(
            probe_file_kind(extracted.path()).unwrap(),
            DbFileKind::Encrypted,
            "{entry} 继承密文形态"
        );
    }

    let dst_dir = ScratchDir::new("backup-test-paired-enc-dst");
    let dst_main = dst_dir.join("ledger.db");
    {
        let factory = tauri_app_lib::test_support::open();
        factory
            .execute("VACUUM INTO ?1", params![dst_main.to_string_lossy()])
            .unwrap();
    }
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    restore_db_from(
        &backup,
        &dst_main,
        &safety_dir,
        expected,
        Some("paired-pass"),
    )
    .unwrap();

    let dst_sync = dst_dir.join("sync.db");
    assert_eq!(
        probe_file_kind(&dst_sync).unwrap(),
        DbFileKind::Encrypted,
        "sync.db 恢复为密文形态"
    );
    let sync = db::open_connection_with_passphrase(&dst_sync, "paired-pass").unwrap();
    let dev: String = sync
        .query_row("SELECT id FROM sync_device", [], |r| r.get(0))
        .unwrap();
    assert_eq!(dev, "dev-src", "设备身份随库回滚（密文形态）");
    let restored = db::open_connection_with_passphrase(&dst_main, "paired-pass").unwrap();
    assert_eq!(count_transactions(&restored), 1, "业务行照常恢复");
}

#[test]
fn pre_update_backup_creates_fixed_name_zip_with_pre_update_kind() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    let dir = temp_safety_dir();
    create_pre_update_backup(&conn, &dir, "0.2.0").unwrap();

    let target = dir.join(PRE_UPDATE_BACKUP_FILE_NAME);
    assert!(target.exists(), "产物落固定名");
    assert_eq!(
        read_backup_kind(&target).unwrap(),
        BackupKind::PreUpdate,
        "来源标记为更新前备份"
    );
}

#[test]
fn pre_update_backup_overwrites_previous_one() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    let dir = temp_safety_dir();
    create_pre_update_backup(&conn, &dir, "0.2.0").unwrap();
    conn.execute(
        "INSERT INTO transactions (id,kind,amount_cents,currency_code,amount_native_cents,account_id,date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES ('txn-2','expense',2500,'CNY',2500,'acc-1','2026-02-02','2026-02-02T00:00:00Z','2026-02-02T00:00:00Z',1,'test',0)",
        [],
    )
    .unwrap();
    create_pre_update_backup(&conn, &dir, "0.3.0").unwrap();

    let entries: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(entries.len(), 1, "覆盖式保留：目录内只有最近一份");

    let backup = dir.join(PRE_UPDATE_BACKUP_FILE_NAME);
    let dst = temp_file("pre-update-overwrite-dst");
    let safety = temp_safety_dir();
    restore_db_from(
        &backup,
        &dst,
        &safety,
        expected_schema_version().unwrap(),
        None,
    )
    .unwrap();
    let restored = open_connection(&dst).unwrap();
    assert_eq!(count_transactions(&restored), 2, "覆盖产物是最新世界");
}

#[test]
fn pre_update_backup_is_not_managed() {
    assert!(!crate::engine::is_managed_backup_file_name(
        PRE_UPDATE_BACKUP_FILE_NAME
    ));

    let conn = tauri_app_lib::test_support::open();
    let dir = temp_safety_dir();
    create_pre_update_backup(&conn, &dir, "0.2.0").unwrap();
    assert!(
        list_managed_backups(&dir, None).unwrap().is_empty(),
        "更新前备份不进受管清单"
    );
}

#[test]
fn fingerprint_identical_across_dirs_and_handles() {
    let dir_a = ScratchDir::new("backup-test-fp-dir-a");
    let dir_b = ScratchDir::new("backup-test-fp-dir-b");
    let (main_a, _) = make_dual_db_world(&dir_a, false);
    let (main_b, _) = make_dual_db_world(&dir_b, false);
    let conn_a = open_connection(&main_a).unwrap();
    let conn_b = open_connection(&main_b).unwrap();

    let fp_a = book_fingerprint(&conn_a).unwrap();
    let fp_b = book_fingerprint(&conn_b).unwrap();
    assert_eq!(fp_a, fp_b, "同一份数据在两个目录/句柄下指纹相同");
    assert_eq!(fp_a, "txn-1", "内容锚 = 库内最早一笔交易的全局唯一标识");
}

#[test]
fn fingerprint_persists_once_fixed() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);
    let fixed = book_fingerprint(&conn).unwrap();
    assert_eq!(fixed, "txn-1");

    let stored: Option<String> = ledger_infra::settings::get(
        &conn,
        ledger_infra::settings::SettingKey::BookFingerprint,
        None,
    )
    .unwrap();
    assert_eq!(stored.as_deref(), Some("txn-1"), "首用算出即固化存储");

    conn.execute(
        "UPDATE transactions SET is_deleted = 1 WHERE id = 'txn-1'",
        [],
    )
    .unwrap();
    assert_eq!(
        book_fingerprint(&conn).unwrap(),
        fixed,
        "删除最早一笔不改变已固化指纹"
    );
}

#[test]
fn empty_book_backs_up_with_generated_fingerprint() {
    let conn = tauri_app_lib::test_support::open();
    let first = temp_file("fp-empty-1");
    backup_db_to(&conn, &first, "0.2.0", BackupKind::Manual).unwrap();
    let fp = read_meta_json(&first)["fingerprint"]
        .as_str()
        .expect("空库产物也带账本指纹")
        .to_string();
    assert_eq!(fp.len(), 36, "端内生成随机标识（UUID 形态）");

    let second = temp_file("fp-empty-2");
    backup_db_to(&conn, &second, "0.2.0", BackupKind::Manual).unwrap();
    assert_eq!(
        read_meta_json(&second)["fingerprint"].as_str().unwrap(),
        fp,
        "空库退化标识同样首用固化"
    );
}

#[test]
fn backup_meta_carries_fingerprint_and_device_id() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    let target = temp_file("watermark");
    backup_db_to(&conn, &target, "0.2.0", BackupKind::Manual).unwrap();

    let meta = read_meta_json(&target);
    assert_eq!(meta["fingerprint"], "txn-1", "指纹 = 内容锚");
    let device = meta["device_id"].as_str().expect("设备水印落盘");
    assert_eq!(
        device,
        ledger_sync_protocol::device::device_id(&conn).unwrap(),
        "设备标识取多端同步域 DeviceId"
    );
}

#[test]
fn encrypted_backup_carries_plaintext_watermark() {
    let dir = ScratchDir::new("backup-test-enc-watermark");
    let src = dir.join("ledger.db");
    {
        let conn = tauri_app_lib::test_support::open();
        seed(&conn);
        conn.execute("VACUUM INTO ?1", params![src.to_string_lossy()])
            .unwrap();
        conn.execute(
            "VACUUM sync INTO ?1",
            params![dir.join(db::SYNC_DB_FILE_NAME).to_string_lossy()],
        )
        .unwrap();
    }
    ledger_infra::db::encryption::enable_encryption_for_file(&src, "pass-phrase-123").unwrap();
    let conn = ledger_infra::db::open_connection_with_passphrase(&src, "pass-phrase-123").unwrap();

    let target = dir.join("ledger-backup-20260918-120000.db.zip");
    backup_db_to(&conn, &target, "0.6.0", BackupKind::Manual).unwrap();

    let meta = read_meta_json(&target);
    assert_eq!(meta["fingerprint"], "txn-1", "密文产物水印仍为明文");
    assert!(!meta["device_id"].as_str().unwrap().is_empty());
}

#[test]
fn meta_with_watermark_and_unknown_fields_reads_ok() {
    let dir = ScratchDir::new("backup-test-meta-unknown");
    let path = dir.join("ledger-backup-20260101-000000.db.zip");
    {
        let file = File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("ledger.db", options).unwrap();
        std::io::Write::write_all(&mut zip, b"stub").unwrap();
        zip.start_file("backup.json", options).unwrap();
        std::io::Write::write_all(
            &mut zip,
            br#"{"created_at":"2025-01-01T00:00:00Z","app_version":"0.2.0","schema_version":4,
                 "kind":"manual","fingerprint":"fp-x","device_id":"dev-x","future_field":{"a":1}}"#,
        )
        .unwrap();
        zip.finish().unwrap();
    }
    assert_eq!(read_backup_kind(&path).unwrap(), BackupKind::Manual);
    assert_eq!(probe_backup_meta(&path).unwrap().kind, BackupKind::Manual);
}

#[test]
fn prune_skips_unknown_origin_products() {
    let dir = ScratchDir::new("backup-test-prune-unknown");
    for name in [
        "ledger-auto-20260101-000000-book-a.db.zip",
        "ledger-auto-20260102-000000-book-a.db.zip",
        "ledger-auto-20260103-000000-book-a.db.zip",
    ] {
        real_managed_backup(&dir, name, BackupKind::Auto);
    }
    let unknown_name = "ledger-auto-20260101-000000-book-b.db.zip";
    {
        let conn = tauri_app_lib::test_support::open();
        seed(&conn);
        conn.execute("DROP TABLE sync.sync_device", []).unwrap();
        backup_db_to(&conn, &dir.join(unknown_name), "0.2.0", BackupKind::Auto).unwrap();
    }
    assert!(
        read_meta_json(&dir.join(unknown_name))
            .get("device_id")
            .is_none(),
        "设备标识不可得的产物不带设备水印（来源未知）"
    );

    let r = prune_managed_backups(&dir, 3, None).unwrap();
    assert!(r.deleted.is_empty(), "未知来源不占保留配额");
    assert_eq!(r.kept, 3, "kept 报告本桶（带设备水印产物）保留数");

    let r2 = prune_managed_backups(&dir, 2, None).unwrap();
    assert_eq!(
        r2.deleted,
        vec!["ledger-auto-20260101-000000-book-a.db.zip"]
    );
    assert!(dir.join(unknown_name).exists(), "来源未知永不自动清理");
    assert_eq!(r2.kept, 2);
}

#[test]
fn managed_write_target_layers_device_subdir() {
    let dir = ScratchDir::new("backup-test-layout");
    let conn = tauri_app_lib::test_support::open();
    let device = crate::identity::source_device_id(&conn).expect("工厂库可取设备标识");
    let target =
        super::engine::managed_write_target(&dir, "ledger-auto-20260101-000000.db.zip", &conn)
            .unwrap();
    assert_eq!(
        target,
        dir.join(&device).join("ledger-auto-20260101-000000.db.zip"),
        "产物名按设备多分一层"
    );
    assert!(dir.join(&device).is_dir(), "设备子目录按需创建");

    let missing = dir.join("gone");
    let target =
        super::engine::managed_write_target(&missing, "ledger-auto-20260101-000000.db.zip", &conn)
            .unwrap();
    assert_eq!(
        target,
        missing
            .join(&device)
            .join("ledger-auto-20260101-000000.db.zip")
    );
    assert!(!missing.exists(), "备份目录本体缺失不在此补建");
}

#[test]
fn managed_write_target_falls_back_flat_without_device() {
    let dir = ScratchDir::new("backup-test-layout-flat");
    let conn = tauri_app_lib::test_support::open();
    conn.execute("DROP TABLE sync.sync_device", []).unwrap();
    let target =
        super::engine::managed_write_target(&dir, "ledger-auto-20260101-000000.db.zip", &conn)
            .unwrap();
    assert_eq!(target, dir.join("ledger-auto-20260101-000000.db.zip"));
}

#[test]
fn list_and_prune_scan_flat_and_device_subdirs() {
    let dir = ScratchDir::new("backup-test-dual-path");
    let device_dir = dir.join("01971234-4567-789a-bcde-f123456789ab");
    std::fs::create_dir_all(&device_dir).unwrap();
    let flat = real_managed_backup(
        &dir,
        "ledger-backup-20260101-010101.db.zip",
        BackupKind::Manual,
    );
    real_managed_backup(
        &device_dir,
        "ledger-auto-20260201-010101.db.zip",
        BackupKind::Auto,
    );
    let newest = real_managed_backup(
        &device_dir,
        "ledger-auto-20260202-010101.db.zip",
        BackupKind::Auto,
    );

    let list = list_managed_backups(&dir, None).unwrap();
    assert_eq!(list.len(), 3, "扁平 + 设备子目录合并列出");
    assert_eq!(
        list[0].file_name, "ledger-auto-20260202-010101.db.zip",
        "跨布局统一按新→旧排序"
    );
    assert_eq!(
        Path::new(&list[0].path),
        newest,
        "设备子目录产物给出完整路径"
    );
    assert_eq!(list[2].file_name, "ledger-backup-20260101-010101.db.zip");
    assert_eq!(Path::new(&list[2].path), flat, "扁平产物路径不变");

    let r = prune_managed_backups(&dir, 2, None).unwrap();
    assert_eq!(r.deleted, vec!["ledger-backup-20260101-010101.db.zip"]);
    assert!(!flat.exists(), "扁平路径内的超额产物照常清理");
    assert!(newest.exists(), "设备子目录产物留存");
}

#[test]
fn restore_works_from_both_layouts() {
    let dir = ScratchDir::new("backup-test-restore-dual");
    let device_dir = dir.join("01971234-4567-789a-bcde-f123456789ab");
    std::fs::create_dir_all(&device_dir).unwrap();
    let sources = [
        real_managed_backup(
            &device_dir,
            "ledger-auto-20260201-010101.db.zip",
            BackupKind::Auto,
        ),
        real_managed_backup(
            &dir,
            "ledger-backup-20260101-010101.db.zip",
            BackupKind::Manual,
        ),
    ];
    let safety = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    for (i, source) in sources.iter().enumerate() {
        let db_path = dir.join(format!("restored-{i}.db"));
        let result = restore_db_from(source, &db_path, &safety, expected, None).unwrap();
        assert_eq!(result.schema_version, expected, "两种路径恢复都成功");
        let conn = open_connection(&db_path).unwrap();
        assert_eq!(count_transactions(&conn), 1, "恢复内容完整");
    }
}

#[test]
fn command_target_retargets_only_inside_backup_dir() {
    static PREFS_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = PREFS_LOCK.lock().unwrap();
    let prefs = super::auto::shared_prefs();
    let dir = ScratchDir::new("backup-test-retarget");
    let conn = tauri_app_lib::test_support::open();
    let device = crate::identity::source_device_id(&conn).expect("工厂库可取设备标识");
    let managed = dir.join("ledger-backup-20260101-010101.db.zip");
    let expected = dir
        .join(&device)
        .join("ledger-backup-20260101-010101.db.zip");

    for mirror in [
        dir.to_string_lossy().into_owned(),
        format!("{}/", dir.to_string_lossy()),
    ] {
        prefs.set_dir(Some(mirror));
        assert_eq!(
            managed_command_target(&managed, &conn).unwrap(),
            expected,
            "镜像目录内受管命名改道设备子目录"
        );
    }
    let outside = ScratchDir::new("backup-test-retarget-out");
    prefs.set_dir(Some(dir.to_string_lossy().into_owned()));
    assert_eq!(
        managed_command_target(&outside.join("ledger-backup-20260101-010101.db.zip"), &conn)
            .unwrap(),
        outside.join("ledger-backup-20260101-010101.db.zip")
    );
    assert_eq!(
        managed_command_target(&dir.join("my-notes.zip"), &conn).unwrap(),
        dir.join("my-notes.zip")
    );
    prefs.set_dir(None);
    assert_eq!(managed_command_target(&managed, &conn).unwrap(), managed);
}
