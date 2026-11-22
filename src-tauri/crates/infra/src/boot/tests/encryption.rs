use std::path::Path;

use tauri_app_lib::test_support::ScratchDir;

use rusqlite::{Connection, params};

use crate::db::encryption::{DbFileKind, SQLITE_HEADER_MAGIC, probe_file_kind};
use crate::db::{migrations, open_connection, open_connection_with_passphrase};

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("db-enc-{tag}"))
}

fn read_header(db: &Path) -> [u8; 16] {
    use std::io::Read;
    let mut file = std::fs::File::open(db).unwrap();
    let mut header = [0u8; 16];
    file.read_exact(&mut header).unwrap();
    header
}

#[test]
fn connection_without_key_stays_plaintext() {
    let dir = temp_dir("plain");
    let db = dir.join("ledger.db");
    let mut conn = open_connection(&db).unwrap();
    migrations().to_latest(&mut conn).unwrap();
    drop(conn);

    assert_eq!(
        read_header(&db),
        SQLITE_HEADER_MAGIC,
        "未设密钥的库文件头应为明文魔数"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);

    let mut reopened = open_connection(&db).unwrap();
    migrations().to_latest(&mut reopened).unwrap();
    let currencies: i64 = reopened
        .query_row("SELECT COUNT(*) FROM currencies", [], |r| r.get(0))
        .unwrap();
    assert_eq!(currencies, 11, "种子币种应原样可读");
}

#[test]
fn passphrase_connection_writes_ciphertext_and_reopens_with_same_passphrase() {
    let dir = temp_dir("keyed");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection_with_passphrase(&db, "主口令-正确").unwrap();
        migrations().to_latest(&mut conn).unwrap();
        conn.execute("CREATE TABLE reopen_probe(name TEXT)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO reopen_probe(name) VALUES (?1)",
            params!["数据①"],
        )
        .unwrap();
    }

    assert_ne!(
        read_header(&db),
        SQLITE_HEADER_MAGIC,
        "带主口令打开的库文件落盘不应是明文魔数"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    let mut conn = open_connection_with_passphrase(&db, "主口令-正确").unwrap();
    migrations().to_latest(&mut conn).unwrap();
    let name: String = conn
        .query_row("SELECT name FROM reopen_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "数据①");
}

#[test]
fn passphrase_connection_rejects_wrong_passphrase() {
    let dir = temp_dir("wrong-key");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection_with_passphrase(&db, "正确口令").unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    let err = open_connection_with_passphrase(&db, "错误口令").unwrap_err();
    assert_eq!(
        code_of(&err),
        Some("encryption.passphrase-incorrect"),
        "错误主口令应报合并口径码化错误，实际: {err}"
    );
}

use crate::db::encryption::{enable_encryption_for_file, unlock_db_file};
use crate::db::{check_integrity, open_connection_with_passphrase as reopen_with_key};

fn seed_transactions(conn: &Connection, count: usize) {
    use ledger_transaction::TransactionInput;
    use ledger_transaction::amount::TransactionKind;
    ledger_accounts::balance::install_balance_refresh_hook();
    tauri_app_lib::transaction_wiring::install_all();
    let account_id = crate::db::new_uuid();
    tauri_app_lib::test_support::seed_account(conn, &account_id, "现金", "cash", "CNY", 0);
    ledger_accounts::balance::refresh_account_balances(conn, &[account_id.as_str()]).unwrap();
    for i in 0..count {
        let input = TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000 + i as i64,
            currency_code: "CNY".into(),
            account_id: Some(account_id.clone()),
            funding: Vec::new(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            funding_account_id: None,
            source_order_no: None,
            note: Some(format!("种子交易 {i}")),
            date: "2026-03-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            to_instrument_id: None,
            to_quantity: None,
            out_amount_cents: None,
            in_amount_cents: None,
            idempotency_key: None,
            origin: None,
            fx_rate: None,
            purchases: Vec::new(),
        };
        ledger_transaction::create_transaction_internal(conn, input).unwrap();
    }
}

fn count_transactions(conn: &Connection) -> i64 {
    conn.query_row(
        "SELECT COUNT(*) FROM transactions WHERE is_deleted = 0",
        [],
        |r| r.get(0),
    )
    .unwrap()
}

fn code_of(err: &crate::error::AppError) -> Option<&str> {
    match err {
        crate::error::AppError::Coded { code, .. } => Some(code),
        _ => None,
    }
}

#[test]
fn enable_encryption_converts_plaintext_db_and_preserves_data() {
    let dir = temp_dir("convert");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 3);
    }
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);

    enable_encryption_for_file(&db, "correct horse").unwrap();

    let bak = db.with_extension("db.bak");
    assert!(bak.exists(), "原明文库应保留为 .bak 副本");
    assert_eq!(probe_file_kind(&bak).unwrap(), DbFileKind::Plaintext);
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    let mut conn = reopen_with_key(&db, "correct horse").unwrap();
    migrations().to_latest(&mut conn).unwrap();
    assert_eq!(count_transactions(&conn), 3, "转换后交易数据应完整无损");
    check_integrity(&conn).unwrap();
}

#[test]
fn enable_encryption_converts_sync_db_too() {
    let dir = temp_dir("convert-sync-on");
    let db = dir.join("ledger.db");
    let sync_path = dir.join(crate::db::connection::SYNC_DB_FILE_NAME);
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        conn.execute("CREATE TABLE sync.pair_probe(x)", []).unwrap();
    }
    assert_eq!(probe_file_kind(&sync_path).unwrap(), DbFileKind::Plaintext);

    crate::db::encryption::enable_encryption_for_file(&db, "correct horse").unwrap();

    assert_eq!(probe_file_kind(&sync_path).unwrap(), DbFileKind::Encrypted);
    let sync_bak = sync_path.with_extension("db.bak");
    assert!(sync_bak.exists(), "旧明文 sync.db 应保留为 .bak 副本");
    assert_eq!(probe_file_kind(&sync_bak).unwrap(), DbFileKind::Plaintext);

    let conn = reopen_with_key(&db, "correct horse").unwrap();
    conn.query_row::<i64, _, _>("SELECT count(*) FROM sync.sqlite_master", [], |r| r.get(0))
        .expect("转换后 sync.db 应可凭新口令同 KEY 挂载读取");
}

#[test]
fn disable_encryption_converts_sync_db_too() {
    let dir = temp_dir("convert-sync-off");
    let db = dir.join("ledger.db");
    let sync_path = dir.join(crate::db::connection::SYNC_DB_FILE_NAME);
    {
        let mut conn = open_connection_with_passphrase(&db, "旧口令").unwrap();
        migrations().to_latest(&mut conn).unwrap();
        conn.execute("CREATE TABLE sync.pair_probe(x)", []).unwrap();
        conn.execute("INSERT INTO sync.pair_probe VALUES (7)", [])
            .unwrap();
    }
    assert_eq!(probe_file_kind(&sync_path).unwrap(), DbFileKind::Encrypted);

    crate::db::encryption::disable_encryption_for_file(&db, "旧口令").unwrap();

    assert_eq!(probe_file_kind(&sync_path).unwrap(), DbFileKind::Plaintext);
    let sync_bak = sync_path.with_extension("db.bak");
    assert_eq!(probe_file_kind(&sync_bak).unwrap(), DbFileKind::Encrypted);
    let old = open_connection_with_passphrase(&sync_bak, "旧口令").unwrap();
    let kept: i64 = old
        .query_row("SELECT x FROM pair_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 7, "旧密文副本应可凭旧口令读回");

    let conn = open_connection(&db).unwrap();
    let kept: i64 = conn
        .query_row("SELECT x FROM sync.pair_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 7, "明文世界的 sync.db 数据应完整");
}

#[test]
fn change_passphrase_rekeys_sync_db_too() {
    let dir = temp_dir("convert-sync-key");
    let db = dir.join("ledger.db");
    let sync_path = dir.join(crate::db::connection::SYNC_DB_FILE_NAME);
    {
        let mut conn = open_connection_with_passphrase(&db, "旧口令").unwrap();
        migrations().to_latest(&mut conn).unwrap();
        conn.execute("CREATE TABLE sync.pair_probe(x)", []).unwrap();
        conn.execute("INSERT INTO sync.pair_probe VALUES (9)", [])
            .unwrap();
    }

    crate::db::encryption::change_passphrase_for_file(&db, "旧口令", "新口令啊").unwrap();

    let conn = reopen_with_key(&db, "新口令啊").unwrap();
    let kept: i64 = conn
        .query_row("SELECT x FROM sync.pair_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 9, "重加密后 sync.db 数据应完整");
    let stale = open_connection_with_passphrase(&sync_path, "旧口令").unwrap();
    let err = stale
        .query_row::<i64, _, _>("SELECT count(*) FROM pair_probe", [], |r| r.get(0))
        .expect_err("旧口令不应再能读重加密后的 sync.db");
    assert!(
        crate::db::connection::is_not_a_database_error(&err),
        "旧口令应报 not-a-database，实际: {err}"
    );
}

#[test]
fn conversion_failure_keeps_both_dbs_intact() {
    let dir = temp_dir("convert-sync-fail");
    let db = dir.join("ledger.db");
    let sync_path = dir.join(crate::db::connection::SYNC_DB_FILE_NAME);
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        conn.execute("CREATE TABLE sync.pair_probe(x)", []).unwrap();
    }
    let main_bytes = std::fs::read(&db).unwrap();
    std::fs::write(&sync_path, b"corrupt sync bytes").unwrap();
    let sync_bytes = std::fs::read(&sync_path).unwrap();

    let err = crate::db::encryption::enable_encryption_for_file(&db, "pw").unwrap_err();
    let _ = code_of(&err);

    assert_eq!(
        std::fs::read(&sync_path).unwrap(),
        sync_bytes,
        "sync.db 字节应保持不变"
    );
    assert_eq!(
        std::fs::read(&db).unwrap(),
        main_bytes,
        "主库字节应保持不变"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);
}

#[test]
fn reset_encrypted_db_moves_sync_db_aside() {
    let dir = temp_dir("reset-sync");
    let db = dir.join("ledger.db");
    let sync_path = dir.join(crate::db::connection::SYNC_DB_FILE_NAME);
    {
        let mut conn = open_connection_with_passphrase(&db, "被遗忘的口令").unwrap();
        migrations().to_latest(&mut conn).unwrap();
        conn.execute("CREATE TABLE sync.pair_probe(x)", []).unwrap();
        conn.execute("INSERT INTO sync.pair_probe VALUES (3)", [])
            .unwrap();
    }

    let conn = crate::db::encryption::reset_encrypted_db_file(&db).unwrap();
    drop(conn);

    assert!(db.with_extension("db.bak").exists(), "旧密文库应保留 .bak");
    let sync_bak = sync_path.with_extension("db.bak");
    assert!(sync_bak.exists(), "旧密文 sync.db 应保留 .bak");
    let old = open_connection_with_passphrase(&sync_bak, "被遗忘的口令").unwrap();
    let kept: i64 = old
        .query_row("SELECT x FROM pair_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 3, "旧密文 sync.db 副本应可凭旧口令读回");

    let conn = open_connection(&db).unwrap();
    conn.query_row::<i64, _, _>("SELECT count(*) FROM sync.sqlite_master", [], |r| r.get(0))
        .expect("新世界 sync.db 应按明文形态挂载可读");
}
fn readonly_trigger_available() -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        match std::fs::metadata("/proc/self") {
            Ok(meta) => meta.uid() != 0,
            Err(_) => true,
        }
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[test]
fn enable_encryption_failure_keeps_original_db_intact() {
    if !readonly_trigger_available() {
        eprintln!(
            "跳过：目录只读触发手段在当前环境不可用（root 架空权限位或非 Unix），失败路径由 Linux 非 root CI 覆盖（issue #791）"
        );
        return;
    }

    let dir = temp_dir("convert-fail");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 2);
    }
    let original_bytes = std::fs::read(&db).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    let err = enable_encryption_for_file(&db, "pw").unwrap_err();
    let _ = code_of(&err);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    assert_eq!(
        std::fs::read(&db).unwrap(),
        original_bytes,
        "原库文件字节应保持不变"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);
    let conn = open_connection(&db).unwrap();
    assert_eq!(count_transactions(&conn), 2, "明文库仍可用且数据完整");
    assert!(!db.with_extension("db.bak").exists());
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        leftovers
            .iter()
            .all(|name| !name.starts_with(".ledger.db.")),
        "不应残留转换临时文件: {leftovers:?}"
    );
}

#[test]
fn unlock_accepts_correct_passphrase_and_retries_after_wrong() {
    let dir = temp_dir("unlock");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 3);
    }
    enable_encryption_for_file(&db, "正确口令").unwrap();
    let encrypted_bytes = std::fs::read(&db).unwrap();

    let err = unlock_db_file(&db, "错误口令").unwrap_err();
    assert_eq!(
        code_of(&err),
        Some("encryption.passphrase-incorrect"),
        "错误口令应报口令错误（而非文件损坏），实际: {err}"
    );
    assert_eq!(
        std::fs::read(&db).unwrap(),
        encrypted_bytes,
        "失败重试不得改动库文件"
    );

    let conn = unlock_db_file(&db, "正确口令").unwrap();
    assert_eq!(count_transactions(&conn), 3);
    check_integrity(&conn).unwrap();
}

#[test]
fn unlock_on_plaintext_file_reports_not_encrypted() {
    let dir = temp_dir("unlock-plain");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    let err = unlock_db_file(&db, "pw").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-encrypted"));
}

#[test]
fn enable_encryption_rejects_empty_passphrase() {
    let dir = temp_dir("convert-empty");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    let err = enable_encryption_for_file(&db, "").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.passphrase-empty"));
    enable_encryption_for_file(&db, "pw").unwrap();
    let err = enable_encryption_for_file(&db, "pw2").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-plaintext"));
}

#[test]
fn probe_detects_plaintext_encrypted_and_empty() {
    let dir = temp_dir("probe");

    let plain = dir.join("plain.db");
    let mut conn = open_connection(&plain).unwrap();
    crate::test_utils::attach_in_memory_sync(&conn);
    migrations().to_latest(&mut conn).unwrap();
    drop(conn);
    assert_eq!(probe_file_kind(&plain).unwrap(), DbFileKind::Plaintext);

    let encrypted = dir.join("encrypted.db");
    let mut conn = open_connection_with_passphrase(&encrypted, "口令").unwrap();
    crate::test_utils::attach_in_memory_sync(&conn);
    migrations().to_latest(&mut conn).unwrap();
    drop(conn);
    assert_eq!(probe_file_kind(&encrypted).unwrap(), DbFileKind::Encrypted);

    let empty = dir.join("empty.db");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(probe_file_kind(&empty).unwrap(), DbFileKind::Empty);

    assert_eq!(
        probe_file_kind(&dir.join("missing.db")).unwrap(),
        DbFileKind::Empty
    );
}

use crate::db::encryption::reset_encrypted_db_file;

#[test]
fn reset_rejects_non_encrypted_file() {
    let dir = temp_dir("reset-guard");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    let err = reset_encrypted_db_file(&db).unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-encrypted"));
    assert!(db.exists(), "明文库应原样保留");
    assert!(!db.with_extension("db.bak").exists(), "不应产生副本");

    let missing = dir.join("missing.db");
    let err = reset_encrypted_db_file(&missing).unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.db-missing"));
}

use crate::db::encryption::{normalize_plaintext_db_file, probe_file};
use crate::test_utils::{
    FOREIGN_RESERVED_BYTES, read_reserved_byte, write_foreign_form_plaintext_db,
};

#[test]
fn foreign_form_plaintext_fixture_is_a_valid_db_with_reserved_bytes() {
    let dir = temp_dir("foreign-fixture");
    let db = dir.join("ledger.db");
    write_foreign_form_plaintext_db(&db, 3);

    let probe = probe_file(&db).unwrap();
    assert_eq!(probe.kind, DbFileKind::Plaintext);
    assert_eq!(probe.reserved_bytes, Some(FOREIGN_RESERVED_BYTES));
    assert_eq!(read_reserved_byte(&db), FOREIGN_RESERVED_BYTES);

    let conn = Connection::open(&db).unwrap();
    check_integrity(&conn).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 3);
}

#[test]
fn foreign_form_db_breaks_vacuum_into_until_normalized() {
    let dir = temp_dir("foreign-vacuum");
    let db = dir.join("ledger.db");
    write_foreign_form_plaintext_db(&db, 3);
    let snapshot = dir.join("snapshot.db");
    let vacuum_into = |path: &Path| format!("VACUUM INTO '{}'", path.display());

    {
        let conn = Connection::open(&db).unwrap();
        let attempt = conn.execute_batch(&vacuum_into(&snapshot));
        assert!(
            attempt.is_err(),
            "外来形态库应复现「不带 KEY 的 ATTACH 打不开」根因（夹具失效即在此变红）"
        );
    }
    std::fs::remove_file(&snapshot).ok();

    normalize_plaintext_db_file(&db).unwrap();

    {
        let conn = open_connection(&db).unwrap();
        conn.execute_batch(&vacuum_into(&snapshot)).unwrap();
    }
    assert_eq!(read_reserved_byte(&snapshot), 0);
}

#[test]
fn normalize_rewrites_foreign_form_db_and_preserves_data() {
    let dir = temp_dir("normalize");
    let db = dir.join("foreign.db");
    write_foreign_form_plaintext_db(&db, 3);
    {
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch("PRAGMA user_version = 42").unwrap();
    }
    let objects_before: i64 = {
        let conn = Connection::open(&db).unwrap();
        conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
            .unwrap()
    };

    normalize_plaintext_db_file(&db).unwrap();

    assert_eq!(read_reserved_byte(&db), 0, "归一化后保留字节应归零");
    assert_eq!(probe_file(&db).unwrap().reserved_bytes, Some(0));
    let conn = open_connection(&db).unwrap();
    check_integrity(&conn).unwrap();
    let objects_after: i64 = conn
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert_eq!(objects_after, objects_before, "对象数应与源一致");
    let user_version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(user_version, 42, "schema 版本应与源一致");
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 3, "数据应完整保留");

    let bak = db.with_extension("db.bak");
    assert!(bak.exists(), "旧形态应按既有转换语义保留 .bak 副本");
    assert_eq!(read_reserved_byte(&bak), FOREIGN_RESERVED_BYTES);
}

#[test]
fn normalize_leaves_app_owned_plaintext_db_untouched() {
    let dir = temp_dir("normalize-owned");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        migrations().to_latest(&mut conn).unwrap();
        seed_transactions(&conn, 2);
    }
    let before = std::fs::read(&db).unwrap();

    normalize_plaintext_db_file(&db).unwrap();

    assert_eq!(
        std::fs::read(&db).unwrap(),
        before,
        "应用自有形态的明文库不应被重写"
    );
    assert!(!db.with_extension("db.bak").exists(), "不应产生副本");
}

#[test]
fn normalize_failure_keeps_original_db_intact() {
    if !readonly_trigger_available() {
        eprintln!(
            "跳过：目录只读触发手段在当前环境不可用（root 架空权限位或非 Unix），失败路径由 Linux 非 root CI 覆盖（issue #791）"
        );
        return;
    }

    let dir = temp_dir("normalize-fail");
    let db = dir.join("ledger.db");
    write_foreign_form_plaintext_db(&db, 2);
    let original_bytes = std::fs::read(&db).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    normalize_plaintext_db_file(&db).unwrap_err();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    assert_eq!(
        std::fs::read(&db).unwrap(),
        original_bytes,
        "归一化失败不得改动原库任何字节"
    );
    assert_eq!(read_reserved_byte(&db), FOREIGN_RESERVED_BYTES);
    assert!(!db.with_extension("db.bak").exists(), "失败不得产生副本");
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        leftovers
            .iter()
            .all(|name| !name.starts_with(".ledger.db.")),
        "不应残留归一化临时文件: {leftovers:?}"
    );
    let conn = Connection::open(&db).unwrap();
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM fixture_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(rows, 2, "原库数据完整可读");
}

#[test]
fn unlock_with_wrong_passphrase_leaves_no_sync_db_artifact() {
    let dir = temp_dir("unlock-no-artifact");
    let db = dir.join("ledger.db");
    let sync_path = dir.join(crate::db::connection::SYNC_DB_FILE_NAME);
    {
        let conn = Connection::open(&db).unwrap();
        conn.pragma_update(None, "key", "正确口令").unwrap();
        conn.execute("CREATE TABLE pair_probe(x)", []).unwrap();
        drop(conn);
    }
    assert!(!sync_path.exists(), "夹具前置：目录无 sync.db");

    let err = unlock_db_file(&db, "错误口令").unwrap_err();
    assert_eq!(
        code_of(&err),
        Some("encryption.passphrase-incorrect"),
        "错误口令应报合并口径错误：{err}"
    );
    assert!(
        !sync_path.exists(),
        "错误口令尝试不得留下 sync.db（错钥匙残留）"
    );

    let conn = unlock_db_file(&db, "正确口令").unwrap();
    check_integrity(&conn).unwrap();
    conn.query_row::<i64, _, _>("SELECT count(*) FROM pair_probe", [], |r| r.get(0))
        .expect("解锁后数据应完整");
}
