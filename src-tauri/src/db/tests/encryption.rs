//! SQLCipher 引擎基座测试（issue #569 / ADR-0075）：依赖切换不变量
//! （未设密钥的连接保持明文）、建连主口令缝（密文库落盘为密文、凭同一
//! 主口令可再次打开、错误主口令被拒）、文件头探测三态。
//!
//! 用户可见的加密流程（解锁、转换、备份语义）由后续票的 BDD（真临时
//! 目录文件库）覆盖；本处钉住引擎基座的连接与文件级行为。

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::db::encryption::{DbFileKind, SQLITE_HEADER_MAGIC, probe_file_kind};
use crate::db::{init_db, new_uuid, open_connection, open_connection_with_passphrase};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-db-enc-{tag}-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn read_header(db: &Path) -> [u8; 16] {
    use std::io::Read;
    let mut file = std::fs::File::open(db).unwrap();
    let mut header = [0u8; 16];
    file.read_exact(&mut header).unwrap();
    header
}

/// 交付不变量（issue #569）：未设密钥的连接保持明文——文件库建库
/// 落盘为明文魔数，建连与迁移行为与依赖切换前一致。
#[test]
fn connection_without_key_stays_plaintext() {
    let dir = temp_dir("plain");
    let db = dir.join("ledger.db");
    let mut conn = open_connection(&db).unwrap();
    init_db(&mut conn).unwrap();
    drop(conn);

    assert_eq!(
        read_header(&db),
        SQLITE_HEADER_MAGIC,
        "未设密钥的库文件头应为明文魔数"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);

    // 切换前既有的明文打开路径可继续打开使用。
    let mut reopened = open_connection(&db).unwrap();
    init_db(&mut reopened).unwrap();
    let currencies: i64 = reopened
        .query_row("SELECT COUNT(*) FROM currencies", [], |r| r.get(0))
        .unwrap();
    assert_eq!(currencies, 11, "种子币种应原样可读");
}

/// 带主口令打开的库文件落盘为密文（头部非明文魔数），凭同一主口令可再次
/// 打开并读回数据（issue #569 / ADR-0075 决策 4）。
#[test]
fn passphrase_connection_writes_ciphertext_and_reopens_with_same_passphrase() {
    let dir = temp_dir("keyed");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection_with_passphrase(&db, "主口令-正确").unwrap();
        init_db(&mut conn).unwrap();
        conn.execute("CREATE TABLE reopen_probe(name TEXT)", [])
            .unwrap();
        conn.execute(
            "INSERT INTO reopen_probe(name) VALUES (?1)",
            params!["数据①"],
        )
        .unwrap();
    }

    // 密文库头部为随机盐，绝非明文魔数；探测判定为密文库。
    assert_ne!(
        read_header(&db),
        SQLITE_HEADER_MAGIC,
        "带主口令打开的库文件落盘不应是明文魔数"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    // 凭同一主口令可再次打开，迁移幂等、数据读回。
    let mut conn = open_connection_with_passphrase(&db, "主口令-正确").unwrap();
    init_db(&mut conn).unwrap();
    let name: String = conn
        .query_row("SELECT name FROM reopen_probe", [], |r| r.get(0))
        .unwrap();
    assert_eq!(name, "数据①");
}

/// 错误主口令打不开密文库：`PRAGMA key` 本身不校验，失败发生在首条读
/// 语句（SQLITE_NOTADB「file is not a database」）；口令错误 ≠ 库损坏
/// 的用户可见区分由文件头探测承担（ADR-0075 决策 5 的基座行为）。
#[test]
fn passphrase_connection_rejects_wrong_passphrase() {
    let dir = temp_dir("wrong-key");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection_with_passphrase(&db, "正确口令").unwrap();
        init_db(&mut conn).unwrap();
    }
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    let mut conn = open_connection_with_passphrase(&db, "错误口令").unwrap();
    let err = init_db(&mut conn).unwrap_err();
    assert!(
        err.to_string().contains("file is not a database"),
        "错误主口令应以 not-a-database 失败，实际: {err}"
    );
}

// ---------------------------------------------------------------------------
// 整库加密转换与解锁（issue #570 / ADR-0075 决策 5/6）
// ---------------------------------------------------------------------------

use crate::db::encryption::{enable_encryption_for_file, unlock_db_file};
use crate::db::{check_integrity, open_connection_with_passphrase as reopen_with_key};

/// 在库中建一笔账户 + N 条种子交易（与真实写路径一致的 Writer 接缝，
/// 账户插入含缓存行不变量，ADR-0067）。
fn seed_transactions(conn: &Connection, count: usize) {
    use crate::transaction::TransactionInput;
    use crate::transaction::amount::TransactionKind;
    let account_id = crate::db::new_uuid();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,'cash','CNY',0,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z',1,'test',0)",
        params![account_id, "现金"],
    )
    .unwrap();
    crate::accounts::balance::refresh_account_balances(conn, &[account_id.as_str()]).unwrap();
    for i in 0..count {
        let input = TransactionInput {
            merchant_name: None,
            policy_id: None,
            kind: TransactionKind::Expense,
            amount_cents: 1000 + i as i64,
            currency_code: "CNY".into(),
            account_id: account_id.clone(),
            to_account_id: None,
            category_id: None,
            merchant_id: None,
            refund_of_transaction_id: None,
            note: Some(format!("种子交易 {i}")),
            date: "2026-03-01".into(),
            instrument_id: None,
            quantity: None,
            price_cents: None,
            fee_cents: None,
            idempotency_key: None,
        };
        crate::transaction::create_transaction_internal(conn, input).unwrap();
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

/// 错误码提取（码化错误契约，ADR-0050）。
fn code_of(err: &crate::error::AppError) -> Option<&str> {
    match err {
        crate::error::AppError::Coded { code, .. } => Some(code),
        _ => None,
    }
}

/// 开启加密——整库一次性转换往返：转换成功、旧库保留为 .bak 明文副本、
/// 新库为密文、凭新口令可打开且数据完整、schema 版本保持（迁移不重跑）。
#[test]
fn enable_encryption_converts_plaintext_db_and_preserves_data() {
    let dir = temp_dir("convert");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        init_db(&mut conn).unwrap();
        seed_transactions(&conn, 3);
    }
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);

    enable_encryption_for_file(&db, "correct horse").unwrap();

    // 原库文件按重置命名语义保留为 .bak，且仍是明文库。
    let bak = db.with_extension("db.bak");
    assert!(bak.exists(), "原明文库应保留为 .bak 副本");
    assert_eq!(probe_file_kind(&bak).unwrap(), DbFileKind::Plaintext);
    // 原位库文件已变为密文库。
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Encrypted);

    // 凭新口令打开：数据完整、迁移幂等（user_version 已对齐，不再变化）。
    let mut conn = reopen_with_key(&db, "correct horse").unwrap();
    init_db(&mut conn).unwrap();
    assert_eq!(count_transactions(&conn), 3, "转换后交易数据应完整无损");
    check_integrity(&conn).unwrap();
}

/// 转换中途失败时原库原样保留：导出无法落盘（目标目录不可写）时，
/// 原库字节不变、仍是明文库、无残留临时产物——不存在半加密状态。
#[test]
fn enable_encryption_failure_keeps_original_db_intact() {
    let dir = temp_dir("convert-fail");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        init_db(&mut conn).unwrap();
        seed_transactions(&conn, 2);
    }
    let original_bytes = std::fs::read(&db).unwrap();

    // 目录置为不可写（探针同名目录预占 + Unix 权限位，跨环境稳定触发）。
    std::fs::create_dir(dir.join(".x")).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    }

    let err = enable_encryption_for_file(&db, "pw").unwrap_err();
    let _ = code_of(&err); // 失败形态不拘（IO/Db），转换失败本身就是断言对象。

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // 原库字节不变、仍为明文库，可正常打开且数据完整。
    assert_eq!(
        std::fs::read(&db).unwrap(),
        original_bytes,
        "原库文件字节应保持不变"
    );
    assert_eq!(probe_file_kind(&db).unwrap(), DbFileKind::Plaintext);
    let conn = open_connection(&db).unwrap();
    assert_eq!(count_transactions(&conn), 2, "明文库仍可用且数据完整");
    // 不存在半加密状态：目录中无 .bak 副本、无转换临时残留。
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

/// 解锁：正确口令成功且数据完整；错误口令报码化错误（可重试）且不改动
/// 文件任何字节；同一文件可无限次重试，最终凭正确口令解锁成功。
#[test]
fn unlock_accepts_correct_passphrase_and_retries_after_wrong() {
    let dir = temp_dir("unlock");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        init_db(&mut conn).unwrap();
        seed_transactions(&conn, 3);
    }
    enable_encryption_for_file(&db, "正确口令").unwrap();
    let encrypted_bytes = std::fs::read(&db).unwrap();

    // 错误口令：码化错误、可重试语义；文件字节不变。
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

    // 无限重试：同一文件上再次尝试，正确口令解锁成功、数据完整。
    let conn = unlock_db_file(&db, "正确口令").unwrap();
    assert_eq!(count_transactions(&conn), 3);
    check_integrity(&conn).unwrap();
}

/// 解锁的状态区分：明文库上解锁报「不是加密状态」而非口令错误，
/// 避免把状态漂移误报成可无限重试的口令问题。
#[test]
fn unlock_on_plaintext_file_reports_not_encrypted() {
    let dir = temp_dir("unlock-plain");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        init_db(&mut conn).unwrap();
    }
    let err = unlock_db_file(&db, "pw").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-encrypted"));
}

/// 开启加密的口令守卫：空口令拒绝。
#[test]
fn enable_encryption_rejects_empty_passphrase() {
    let dir = temp_dir("convert-empty");
    let db = dir.join("ledger.db");
    {
        let mut conn = open_connection(&db).unwrap();
        init_db(&mut conn).unwrap();
    }
    let err = enable_encryption_for_file(&db, "").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.passphrase-empty"));
    // 已是密文库时拒绝再次开启。
    enable_encryption_for_file(&db, "pw").unwrap();
    let err = enable_encryption_for_file(&db, "pw2").unwrap_err();
    assert_eq!(code_of(&err), Some("encryption.not-plaintext"));
}

/// 文件头探测三态判定正确（明文库 / 密文库 / 空文件），且只依赖文件
/// 本身、不依赖任何库外引导状态（issue #569 / ADR-0075 决策 4）。
#[test]
fn probe_detects_plaintext_encrypted_and_empty() {
    let dir = temp_dir("probe");

    // 明文库。
    let plain = dir.join("plain.db");
    let mut conn = open_connection(&plain).unwrap();
    init_db(&mut conn).unwrap();
    drop(conn);
    assert_eq!(probe_file_kind(&plain).unwrap(), DbFileKind::Plaintext);

    // 密文库。
    let encrypted = dir.join("encrypted.db");
    let mut conn = open_connection_with_passphrase(&encrypted, "口令").unwrap();
    init_db(&mut conn).unwrap();
    drop(conn);
    assert_eq!(probe_file_kind(&encrypted).unwrap(), DbFileKind::Encrypted);

    // 空文件（0 字节）→ 按明文新装对待。
    let empty = dir.join("empty.db");
    std::fs::write(&empty, b"").unwrap();
    assert_eq!(probe_file_kind(&empty).unwrap(), DbFileKind::Empty);

    // 文件不存在 → 同空文件（新装语义）。
    assert_eq!(
        probe_file_kind(&dir.join("missing.db")).unwrap(),
        DbFileKind::Empty
    );
}
