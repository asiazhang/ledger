//! 备份/恢复测试（issue #91 外迁）：zip 打包/恢复往返/新旧 schema 策略/受管备份列表与修剪。

use std::fs::File;
use std::io::Read;
use std::path::Path;

use tauri_app_lib::test_support::{ScratchDir, ScratchFile};

use rusqlite::Connection;
use rusqlite::params;

use super::*;
use ledger_infra::db;
use ledger_infra::db::open_connection;

/// 散文件夹具（issue #1645）：临时文件收进各自的暂存目录，drop（含 panic
/// unwind）连目录一起删除；手写 `fs_util::cleanup` 收尾随迁移退役。
fn temp_file(tag: &str) -> ScratchFile {
    ScratchFile::new(
        &format!("backup-test-{tag}"),
        format!("ledger-backup-test-{tag}-{}.db", db::new_uuid()),
    )
}

/// 建内存库并写入一条账户 + 一条交易（账户经工厂种子，spec #728 / ADR-0084 决策 4）。
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

/// 为每个测试准备独立的安全备份目录（互不干扰）；guard drop 整棵删除。
fn temp_safety_dir() -> ScratchDir {
    ScratchDir::new("backup-test-safety")
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

    // 校验 zip 内容：三个条目（双文件成对，ADR-0139 决策 6；expand 期单库布局
    // 下 sync 件为回退源 main 的同刻拷贝，paired=false 让恢复按旧形态分支）。
    let file = File::open(&target).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    assert_eq!(archive.len(), 3);
    let names: Vec<String> = (0..archive.len())
        .map(|i| archive.by_index(i).unwrap().name().to_string())
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
    assert_eq!(meta["paired"], false, "内存库无挂载，回退形态 paired=false");

    // 解出 db 可打开且数据完整。
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
    // seed 裸 SQL 绕过写接缝，按生产不变量（备份时缓存与实时一致）补齐缓存行。
    ledger_accounts::balance::refresh_all_account_balances(&conn).unwrap();

    let backup = temp_file("rt-backup");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    // 目标库先建好，含一条多余交易；恢复后应只剩备份里的数据。
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
        // 文件库不入测试工厂（ADR-0084 决策 3）：迁移后的库经 VACUUM INTO 落盘
        // （与下方裸库产物同款模式）。
        c.execute("VACUUM INTO ?1", params![db_path.to_string_lossy()])
            .unwrap();
        assert_eq!(count_transactions(&c), 2);
    }

    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    let result = restore_db_from(&backup, &db_path, &safety_dir, expected, None).unwrap();
    assert_eq!(result.schema_version, expected);

    // 恢复后数据与备份一致（1 条交易）。
    let c = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&c), 1);
    // 余额缓存随库文件原样恢复，且与实时计算一致（issue #491：备份恢复场景数字正确）。
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
    // 恢复安全备份的落盘父目录也走暂存目录（原先直接写 /tmp 根）。
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

    // 直接 VACUUM INTO 生成裸 db 文件作为"备份"。
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
    // 仅验产物元数据，无需表结构：工厂全量建库无碍（spec #728 / ADR-0084）。
    let conn = tauri_app_lib::test_support::open();
    // 手动产物：kind 落盘为 manual。
    let manual = temp_file("meta-manual");
    backup_db_to(&conn, &manual, "0.2.0", BackupKind::Manual).unwrap();
    assert_eq!(read_backup_kind(&manual).unwrap(), BackupKind::Manual);

    // 自动产物：kind 落盘为 auto。
    let auto = temp_file("meta-auto");
    backup_db_to(&conn, &auto, "0.2.0", BackupKind::Auto).unwrap();
    assert_eq!(read_backup_kind(&auto).unwrap(), BackupKind::Auto);
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
}

/// 旧版本备份（元数据无 kind 字段）：恢复不报错、列表正常出现，视为 manual。
#[test]
fn legacy_backup_restores_and_lists_without_error() {
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);

    // 用 VACUUM INTO 造一份裸库，再打包成元数据缺 kind 的旧格式 zip。
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
    // 列表：旧格式文件按命名规则正常被识别，来源按 manual 处理。
    let list = list_managed_backups(&dir, None).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        read_backup_kind(Path::new(&list[0].path)).unwrap(),
        BackupKind::Manual
    );
    // 列表项自带来源字段（issue #129）：旧格式缺 kind 按手动展示。
    assert_eq!(list[0].kind, BackupKind::Manual);

    // 恢复：旧格式包完整还原数据。
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

    let list = list_managed_backups(&dir, None).unwrap();
    assert_eq!(list.len(), 4);
    assert_eq!(
        list[0].file_name, "ledger-auto-20260201-010101.db.zip",
        "auto 前缀同样受管且按时间排序"
    );
    assert_eq!(list[0].created_at, "2026-02-01T01:01:01");
    // stub 文件读不出元数据：按文件名前缀回落（issue #129 展示用）。
    assert_eq!(list[0].kind, BackupKind::Auto);
    assert_eq!(list[3].file_name, "ledger-backup-20260101-010101.db.zip");
    assert_eq!(list[3].kind, BackupKind::Manual);

    // 修剪到 2：删除最旧的手动 2 个；不匹配文件与目录不受影响。
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

    // 继续修剪到 1。
    let r2 = prune_managed_backups(&dir, 1, None).unwrap();
    assert_eq!(r2.deleted, vec!["ledger-backup-20260103-010101.db.zip"]);
    assert_eq!(r2.kept, 1);
    assert!(dir.join("ledger-auto-20260201-010101.db.zip").exists());
}

#[test]
fn prune_keeps_all_when_within_limit_and_missing_dir() {
    let dir = ScratchDir::new("backup-test-prune-none");
    std::fs::write(dir.join("ledger-backup-20260101-010101.db.zip"), b"x").unwrap();

    let r = prune_managed_backups(&dir, 30, None).unwrap();
    assert!(r.deleted.is_empty());
    assert_eq!(r.kept, 1);

    // 目录不存在：空结果而非报错。
    let missing = dir.join("gone");
    assert!(list_managed_backups(&missing, None).unwrap().is_empty());
    let r2 = prune_managed_backups(&missing, 5, None).unwrap();
    assert_eq!(r2.kept, 0);
    assert!(r2.deleted.is_empty());
}

#[test]
fn backup_fails_when_target_dir_missing() {
    let conn = tauri_app_lib::test_support::open();
    // 目标父级不存在（暂存目录在场、其下 gone 子目录不建）→ 备份必失败。
    let missing_root = ScratchDir::new("backup-test-missing-target");
    let target = missing_root.join("gone").join("x.zip");
    let err = backup_db_to(&conn, &target, "0.2.0", BackupKind::Manual)
        .unwrap_err()
        .to_string();
    assert!(err.contains("备份目标目录不存在"));
}

/// 断言目录内没有 `.` 前缀的临时残留（`temp_sibling` 产名恒以 `.` 开头，
/// 备份失败的任一退出路径漏清理都会在此显形）。
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

/// 失败路径临时文件卫生（#1454 验收判据负向①，既有缺陷的范围外修复）：
/// 备份在 `VACUUM INTO`、替换启用任一步失败，目标目录都不得留下临时残留——
/// 失败早退绕过 cleanup 曾每次在备份目录留下 0 字节文件（现场累积 162 个）。
/// 恢复「失败早退不清理」本用例即变红。
#[test]
fn backup_failure_leaves_no_temp_residue() {
    let dir = ScratchDir::new("backup-test-residue");

    // 注入点一：源连接处于事务内，`VACUUM INTO` 报错（现场失败形态：
    // 0 字节临时库文件残留）。
    let conn = tauri_app_lib::test_support::open();
    conn.execute_batch("BEGIN").unwrap();
    let target = dir.join("ledger-backup-20260918-120000.db.zip");
    backup_db_to(&conn, &target, "0.6.0", BackupKind::Manual).unwrap_err();
    assert_no_temp_residue(&dir);
    conn.execute_batch("ROLLBACK").unwrap();

    // 注入点二：目标路径被目录占用，替换启用失败（tmp_db 与 tmp_zip
    // 两个临时文件都已生成，双双必须收走）。
    let occupied = dir.join("occupied.zip");
    std::fs::create_dir(&occupied).unwrap();
    backup_db_to(&conn, &occupied, "0.6.0", BackupKind::Manual).unwrap_err();
    assert_no_temp_residue(&dir);
}

// -------------------------------------------------------------------------
// 源库形态门禁（issue #1454）：外来形态明文库（每页保留字节 ≠ 0，外部工具
// 写入，见 #1453）的 `VACUUM INTO` 必然失败（SQLCipher 无 KEY ATTACH 推断
// 路径），备份前先拦下并给可读的码化错误，不再发起注定失败的语句。
// -------------------------------------------------------------------------

/// 外来形态明文库的备份被码化错误拦下（issue #1454 正向 + 负向②）：稳定
/// 错误码 `backup.foreign-form`（用户读不到 SQLite 原文），目标目录无产物、
/// 无临时残留。去掉前置形态检查本用例即变红：错误退回 SQLite 原文
/// `unable to open database`，且留下 0 字节临时库残留。
#[test]
fn backup_rejects_foreign_form_db_with_coded_error() {
    let dir = ScratchDir::new("backup-test-foreign");
    let src = dir.join("ledger.db");
    ledger_infra::test_utils::write_foreign_form_plaintext_db(&src, 3);
    // 裸连接：外来形态在产品可开启集之外（启动在建连前归一化，issue #1453；
    // 建连收尾的同步元数据库挂载会在其上先一步复现同一条无 KEY ATTACH
    // 推断失败）——本用例的主语是 backup_db_to 的形态门禁，不是建连。
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

/// 正常库备份行为与产物形态不变（issue #1454 回归判据）：应用自有形态的
/// 明文库照常备份，产物内数据库偏移 20 = 0（自有形态，不因门禁引入变化）。
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

/// 加密库快照不受形态门禁影响（issue #1454 回归判据）：密文库的保留字节
/// 无意义（`VACUUM INTO` 继承真实密钥，ADR-0075 决策 7），照常备份，产物
/// 仍为密文。
#[test]
fn backup_of_encrypted_db_bypasses_form_guard_and_stays_encrypted() {
    let dir = ScratchDir::new("backup-test-enc");
    let src = dir.join("ledger.db");
    // 文件库不入测试工厂（ADR-0084 决策 3，同本文件上方先例）：迁移后的库经
    // VACUUM INTO 落盘，再整库转密文。
    {
        let conn = tauri_app_lib::test_support::open();
        conn.execute("VACUUM INTO ?1", params![src.to_string_lossy()])
            .unwrap();
    }
    ledger_infra::db::encryption::enable_encryption_for_file(&src, "pass-phrase-123").unwrap();
    let conn = ledger_infra::db::open_connection_with_passphrase(&src, "pass-phrase-123").unwrap();

    let target = dir.join("ledger-backup-20260918-120000.db.zip");
    backup_db_to(&conn, &target, "0.6.0", BackupKind::Manual).unwrap();

    // 产物内数据库仍是密文（继承源库加密与密钥，既有设计依赖）。
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

// -------------------------------------------------------------------------
// 备份按账本分域（issue #836 / ADR-0089 决策 5）：命名携带账本标识、列表与
// 滚动清理按本作用域、无标识历史产物归登记序首本。
// -------------------------------------------------------------------------

/// 命名/解析往返：标识内容含 `-`（UUID 与目录派生哈希）从尾部定宽解析不受干扰；
/// 旧命名缺标识可读；非受管/残缺命名不参与解析。
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
        // 解析出的标识与命名一致；时间戳可解析。
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
    // 非受管 / 残缺命名：不解析。
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

/// 作用域过滤：标识匹配可见；其他账本的产物不可见；无标识历史产物仅
/// `include_legacy`（登记序首本）可见；无作用域全可见（兼容口径）。
#[test]
fn scoped_listing_filters_by_book() {
    let dir = ScratchDir::new("backup-test-scope-list");
    for name in [
        "ledger-backup-20260101-000000.db.zip", // 无标识（历史产物）
        "ledger-auto-20260102-000000-book-a.db.zip", // 账本 A
        "ledger-backup-20260103-000000-book-a.db.zip", // 账本 A
        "ledger-backup-20260104-000000-book-b.db.zip", // 账本 B
    ] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    let names = |list: &[BackupFileInfo]| -> Vec<String> {
        let mut v: Vec<String> = list.iter().map(|f| f.file_name.clone()).collect();
        v.sort();
        v
    };

    // 无作用域：全部可见（兼容口径）。
    assert_eq!(names(&list_managed_backups(&dir, None).unwrap()).len(), 4);
    // 账本 A（登记序首本，include_legacy）：历史产物 + A 的两份。
    let scope_a = BackupScope {
        book_id: "book-a".into(),
        include_legacy: true,
    };
    assert_eq!(
        names(&list_managed_backups(&dir, Some(&scope_a)).unwrap()),
        vec![
            "ledger-auto-20260102-000000-book-a.db.zip",
            "ledger-backup-20260101-000000.db.zip",
            "ledger-backup-20260103-000000-book-a.db.zip",
        ]
    );
    // 账本 B（非首本）：只见 B 自己的产物，历史产物不归属。
    let scope_b = BackupScope {
        book_id: "book-b".into(),
        include_legacy: false,
    };
    assert_eq!(
        names(&list_managed_backups(&dir, Some(&scope_b)).unwrap()),
        vec!["ledger-backup-20260104-000000-book-b.db.zip"]
    );
}

/// 按本滚动清理：共享目录内各账本独立计算保留上限，互不影响。
#[test]
fn scoped_prune_is_per_book() {
    let dir = ScratchDir::new("backup-test-scope-prune");
    for name in [
        "ledger-auto-20260101-000000-book-a.db.zip",
        "ledger-auto-20260102-000000-book-a.db.zip",
        "ledger-auto-20260103-000000-book-a.db.zip",
        "ledger-auto-20260101-000000-book-b.db.zip",
        "ledger-backup-20260101-000000.db.zip",
    ] {
        std::fs::write(dir.join(name), b"x").unwrap();
    }
    // 账本 A 上限 1：只删 A 自己最旧的两份，B 的与历史产物不受影响。
    let scope_a = BackupScope {
        book_id: "book-a".into(),
        include_legacy: false,
    };
    let r = prune_managed_backups(&dir, 1, Some(&scope_a)).unwrap();
    assert_eq!(
        r.deleted,
        vec![
            "ledger-auto-20260101-000000-book-a.db.zip",
            "ledger-auto-20260102-000000-book-a.db.zip",
        ]
    );
    assert!(
        dir.join("ledger-auto-20260103-000000-book-a.db.zip")
            .exists()
    );
    assert!(
        dir.join("ledger-auto-20260101-000000-book-b.db.zip")
            .exists()
    );
    assert!(dir.join("ledger-backup-20260101-000000.db.zip").exists());

    // 首本作用域（include_legacy）：历史产物计入上限，最旧淘汰。
    let scope_first = BackupScope {
        book_id: "book-a".into(),
        include_legacy: true,
    };
    let r2 = prune_managed_backups(&dir, 1, Some(&scope_first)).unwrap();
    assert_eq!(r2.deleted, vec!["ledger-backup-20260101-000000.db.zip"]);
    // 作用域内 = A 的 1 份 + 历史产物 1 份 = 2，上限 1 → 删最旧的历史产物。
    assert_eq!(r2.kept, 1, "作用域内只剩 A 的最新一份");
    assert!(
        dir.join("ledger-auto-20260103-000000-book-a.db.zip")
            .exists()
    );
    assert!(
        dir.join("ledger-auto-20260101-000000-book-b.db.zip")
            .exists()
    );
}

/// 恢复——同步元数据库一并移位（ADR-0139 决策 3 配对纪律，issue #1869）：
/// 原位遗留的 sync.db 移入恢复安全备份目录保留；恢复后的世界重开时挂载按
/// 恢复后主库形态补建 sync.db，不留形态错配堵塞（如密文世界恢复明文备份）。
#[test]
fn restore_moves_stale_sync_db_aside() {
    let dir = ScratchDir::new("backup-test-restore-sync");
    let db_path = dir.join("ledger.db");
    {
        let conn = tauri_app_lib::test_support::open();
        seed(&conn);
        conn.execute("VACUUM INTO ?1", params![db_path.to_string_lossy()])
            .unwrap();
    }
    // 原位世界带一个 sync.db（挂载接线建连即补建的形态），写入探针表供副本
    // 可读性断言。
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

    // 安全目录保留主库与 sync.db 两份副本；移走的 sync.db 副本可凭明文读回
    // 探针表（证明移位的是原位世界的那一份）。恢复路径末端 open_reset 重开
    // 时挂载接线会按恢复后主库形态补建新 sync.db，原位存在性不作断言。
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
    // 恢复后的世界重开：普通挂载成功（新 sync.db 按恢复后主库形态补建），
    // 数据与备份一致。
    let conn = open_connection(&db_path).unwrap();
    assert_eq!(count_transactions(&conn), 1);
    conn.query_row::<i64, _, _>("SELECT count(*) FROM sync.sqlite_master", [], |r| r.get(0))
        .expect("恢复后世界应可正常挂载 sync.db");
}

// ---------------------------------------------------------------------------
// 双库成对备份与按形态恢复（票 04 / ADR-0139 决策 6）：备份产物 zip 成对
// （ledger.db + sync.db + 元数据），恢复按 `paired` 标记分支——成对产物把
// 设备身份、位点、日志随库一致回滚，旧形态单文件行为不变。
// ---------------------------------------------------------------------------

/// 同步元数据四表闭集（ADR-0139 决策 1；测试侧复制以构造与核对双库世界）。
const SYNC_TABLES: [&str; 4] = [
    "sync_device",
    "sync_ops",
    "sync_parked_ops",
    "sync_stream_positions",
];

/// 构造「票 05 后形态」的双库世界文件对：业务库（无同步表）与同步元数据库
/// （仅四表闭集；`sync_rows` 为真时注入身份行——设备 `dev-src`、对 A 流位点 3）。
/// 文件库不入测试工厂（ADR-0084 决策 3）：落盘与改形态经产品建缝
/// `open_connection_unmounted`（不触发挂载接线，避免旁挂出第三份库文件）。
fn make_dual_db_world(dir: &Path, sync_rows: bool) -> (std::path::PathBuf, std::path::PathBuf) {
    let main_path = dir.join("ledger.db");
    let sync_path = dir.join("sync.db");
    // 业务库：工厂库落盘后卸下四张同步表（票 05 后 main 形态）。
    {
        let factory = tauri_app_lib::test_support::open();
        seed(&factory);
        factory
            .execute("VACUUM INTO ?1", params![main_path.to_string_lossy()])
            .unwrap();
    }
    {
        let conn = db::open_connection_unmounted(&main_path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        for table in SYNC_TABLES {
            conn.execute(&format!("DROP TABLE IF EXISTS {table}"), [])
                .unwrap();
        }
    }
    // 同步库：工厂库落盘后卸下全部业务表（仅剩四表闭集）。
    {
        let factory = tauri_app_lib::test_support::open();
        factory
            .execute("VACUUM INTO ?1", params![sync_path.to_string_lossy()])
            .unwrap();
    }
    {
        let conn = db::open_connection_unmounted(&sync_path).unwrap();
        conn.execute_batch("PRAGMA foreign_keys = OFF").unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        drop(stmt);
        for table in tables {
            if !SYNC_TABLES.contains(&table.as_str()) {
                conn.execute(&format!("DROP TABLE IF EXISTS \"{table}\""), [])
                    .unwrap();
            }
        }
        if sync_rows {
            // 簿记戳由工厂固定时刻发放（ADR-0084：时刻值收敛 test_support）。
            conn.execute(
                "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) \
                 VALUES ('dev-src', 7, ?1, ?1)",
                [tauri_app_lib::test_support::FIXED_NOW],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
                 VALUES ('dev-a', 3, ?1)",
                [tauri_app_lib::test_support::FIXED_NOW],
            )
            .unwrap();
        }
    }
    (main_path, sync_path)
}

/// 读取 zip 包内 backup.json 的原始 JSON（形态标记断言用）。
fn read_meta_json(path: &Path) -> serde_json::Value {
    let file = File::open(path).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name("backup.json").unwrap();
    let mut buf = String::new();
    entry.read_to_string(&mut buf).unwrap();
    serde_json::from_str(&buf).unwrap()
}

/// 在目标世界预置一个遗留 sync.db 干扰物（含探针表），返回其路径。
fn plant_stale_sync_db(dst_dir: &Path) -> std::path::PathBuf {
    let dst_sync = dst_dir.join("sync.db");
    let conn = open_connection(&dst_sync).unwrap();
    conn.execute("CREATE TABLE stale_probe(x)", []).unwrap();
    dst_sync
}

/// 成对备份产物把同步身份一并搬上通道：paired=true、sync 件携带身份行；
/// 恢复到新世界后设备身份与位点随库一致回滚，遗留 sync.db 移入安全目录保留，
/// 重开挂载可用（恢复即回到完整世界）。
#[test]
fn paired_backup_restores_sync_db_when_paired() {
    let src_dir = ScratchDir::new("backup-test-paired-src");
    let (main_path, _sync_path) = make_dual_db_world(&src_dir, true);
    let conn = open_connection(&main_path).unwrap(); // 挂载接线挂上四表 sync.db
    assert!(
        db::sync_tables_live_attached(&conn),
        "双库世界判据命中 attached（前置：挂载接线在位）"
    );

    let backup = temp_file("paired-backup");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    // 产物形态：paired=true，sync 件内含身份行。
    let meta = read_meta_json(&backup);
    assert_eq!(meta["paired"], true, "双库世界的产物标记为成对");
    // guard 绑定到用例作用域（块内临时值块结束即 drop，整棵暂存目录被清走）。
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

    // 恢复到新世界（预置遗留 sync.db 干扰物）。
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

    // 主库 = 备份的业务行；sync.db = 备份的同步身份（设备、位点随库回滚）。
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
    // 遗留 sync.db 移入安全目录保留（永不删除）；重开挂载可用。
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

/// 恢复按形态分支（旧形态行为不变）：expand 期单库布局的产物 paired=false
/// （sync 条目为回退源 main 的同刻拷贝），恢复只落主库——sync.db 照旧移入
/// 安全目录、原位不重建（由挂载接线按恢复后主库形态补建）。
#[test]
fn legacy_form_backup_ignores_sync_entry_on_restore() {
    // 内存库（无挂载）→ 回退形态产物。
    let conn = tauri_app_lib::test_support::open();
    seed(&conn);
    let backup = temp_file("legacy-form-backup");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();
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

    // 旧形态：主库恢复照常；sync.db 移入安全目录、原位不重建（restore 不落
    // sync 条目——重建归挂载接线，重开后才出现）。
    // 旧形态行为不变：主库照常恢复；sync 条目不落盘——遗留 sync.db 已移入安全
    // 目录，原位的 sync.db 由恢复尾部的重置建连经挂载接线按空库补建（既有
    // #1869 行为），不是备份 sync 条目的内容（那会是回退源 main 的整库拷贝）。
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
}

/// 成对产物缺 sync 条目（手工拼包才可能命中）：显式报错，不静默丢同步身份。
#[test]
fn paired_backup_missing_sync_entry_fails_loud() {
    let src_dir = ScratchDir::new("backup-test-paired-missing");
    let (main_path, _sync_path) = make_dual_db_world(&src_dir, true);
    let conn = open_connection(&main_path).unwrap();
    let backup = temp_file("paired-missing-entry");
    backup_db_to(&conn, &backup, "0.2.0", BackupKind::Manual).unwrap();

    // 拆包重拼：保留 ledger.db 与 backup.json，丢掉 sync.db 条目。
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
    }
    let safety_dir = temp_safety_dir();
    let expected = expected_schema_version().unwrap();
    let err = restore_db_from(&rewritten, &dst_main, &safety_dir, expected, None).unwrap_err();
    assert_eq!(err.code(), Some("backup.sync-entry-missing"));
    // 拒绝发生在任何替换之前：目标世界原样保留。
    let intact = open_connection(&dst_main).unwrap();
    assert_eq!(count_transactions(&intact), 0, "目标世界未被恢复改动");
}
