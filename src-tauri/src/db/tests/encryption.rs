//! SQLCipher 引擎基座测试（issue #569 / ADR-0075）：依赖切换不变量
//! （未设密钥的连接保持明文）、建连主口令缝（密文库落盘为密文、凭同一
//! 主口令可再次打开、错误主口令被拒）、文件头探测三态。
//!
//! 用户可见的加密流程（解锁、转换、备份语义）由后续票的 BDD（真临时
//! 目录文件库）覆盖；本处钉住引擎基座的连接与文件级行为。

use std::path::{Path, PathBuf};

use rusqlite::params;

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
