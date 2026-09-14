//! 只读连接（读路径独立只读连接，issue #1280 / ADR-0117）：只读 flags（写被拒）、
//! 密文库主口令注入、rollback journal 下「写事务在途只读连接照常可读」（路线 B
//! 的连接级机制证据）、成对 [`DbState`] 的读槽换连语义。
//!
//! 文件库不入测试工厂（ADR-0084 决策 3）：建库经产品建缝 `open_connection` +
//! 产品迁移缝 `migrations().to_latest`（boot/tests 先例）。

use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::db::{migrations, new_uuid, open_connection, open_connection_with_passphrase};

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ledger-db-readonly-{tag}-{}", new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn plaintext_db(tag: &str) -> PathBuf {
    let dir = temp_dir(tag);
    let db = dir.join(crate::db::connection::DB_FILE_NAME);
    let mut conn = open_connection(&db).unwrap();
    migrations().to_latest(&mut conn).unwrap();
    tauri_app_lib::test_support::seed_account(&conn, "acct-1", "现金", "cash", "CNY", 12345);
    db
}

/// 只读连接读到已提交真实行；写入被只读约束拒绝（「读路径无写」从纪律变成
/// 运行时约束，ADR-0117 代价 3）。
#[test]
fn readonly_connection_reads_committed_row_and_rejects_writes() {
    let db = plaintext_db("rw");
    let conn = crate::db::open_connection_readonly(&db).unwrap();
    let balance: i64 = conn
        .query_row(
            "SELECT initial_balance_cents FROM accounts WHERE id = 'acct-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(balance, 12345, "只读连接应读到已提交的行");
    let err = conn
        .execute("DELETE FROM accounts WHERE id = 'acct-1'", [])
        .unwrap_err();
    assert!(
        matches!(err, rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ReadOnly),
        "只读连接上的写入应被 ReadOnly 拒绝，实际 {err:?}"
    );
}

/// 密文库：只读连接凭主口令打开可读（密钥注入在建连收尾单点内，ADR-0117 决策 2）；
/// 错误口令在建连时静默、首条读语句报错（与写连接同纪律，ADR-0075）。
#[test]
fn readonly_encrypted_connection_needs_passphrase() {
    let dir = temp_dir("enc");
    let db = dir.join(crate::db::connection::DB_FILE_NAME);
    {
        let mut conn = open_connection_with_passphrase(&db, "主口令-正确").unwrap();
        migrations().to_latest(&mut conn).unwrap();
    }
    let conn = crate::db::open_connection_readonly_with_passphrase(&db, "主口令-正确").unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM sqlite_master", [], |r| r.get(0))
        .unwrap();
    assert!(count > 0, "凭正确口令的只读连接应可读密文库");

    let wrong = crate::db::open_connection_readonly_with_passphrase(&db, "主口令-错误").unwrap();
    assert!(
        crate::db::encryption::is_not_a_database(
            &wrong
                .query_row("SELECT count(*) FROM sqlite_master", [], |r| r
                    .get::<_, i64>(0))
                .unwrap_err()
        ),
        "错误口令应在首条读语句报 not-a-database"
    );
}

/// 路线 B 的连接级机制证据：写连接事务在途（RESERVED）期间，只读连接照常即时
/// 可读——「长写事务不再挡读」（ADR-0117 决策 1）。
#[test]
fn readonly_connection_reads_while_writer_holds_transaction() {
    let db = plaintext_db("inflight");
    let writer = open_connection(&db).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    tauri_app_lib::test_support::seed_account(&writer, "acct-2", "在途", "cash", "CNY", 1);

    let reader = crate::db::open_connection_readonly(&db).unwrap();
    let started = Instant::now();
    let visible: i64 = reader
        .query_row(
            "SELECT count(*) FROM accounts WHERE id = 'acct-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(visible, 1, "只读连接应读到已提交数据（不含在途写入）");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "写事务在途时读应即时返回，实际耗时 {:?}",
        started.elapsed()
    );
    writer.execute_batch("ROLLBACK").unwrap();
}

/// 成对 DbState：写经写连接提交、读经读连接可见（open_db_in 成对换连的连接级
/// 前提：两连接指向同一库文件）。
#[test]
fn open_db_in_pairs_write_and_read_connections() {
    let dir = temp_dir("pair");
    let state = crate::db::open_db_in(&dir).unwrap();
    state
        .write(|conn| {
            tauri_app_lib::test_support::seed_account(conn, "acct-p", "成对", "cash", "CNY", 777);
            Ok(())
        })
        .unwrap();
    let balance: i64 = {
        let guard = state.read_conn.lock().unwrap();
        guard
            .query_row(
                "SELECT initial_balance_cents FROM accounts WHERE id = 'acct-p'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(balance, 777, "读连接应看到写连接提交的数据");
}

/// 读槽换连：原位替换经已持有的 Arc 克隆同步可见（ADR-0080 既有语义）；占位化
/// 后读报错（不静默回落写连接，ADR-0117 决策 3）。
#[test]
fn read_slot_replacement_and_placeholderization() {
    let db = plaintext_db("slot");
    let state = crate::db::open_db_in(db.parent().unwrap()).unwrap();
    let cloned = state.read_conn.clone();

    // 占位化：读槽换成占位内存库（无表），读报错。
    state.placeholderize_read_conn().unwrap();
    {
        let guard = cloned.lock().unwrap();
        assert!(
            guard
                .query_row::<i64, _, _>("SELECT count(*) FROM accounts", [], |r| r.get(0))
                .is_err(),
            "占位化后读应报错（占位内存库无业务表）"
        );
    }

    // 换回真实只读连接：同一 Arc 克隆立即读到真实库。
    state
        .replace_read_conn(crate::db::open_connection_readonly(&db).unwrap())
        .unwrap();
    let balance: i64 = {
        let guard = cloned.lock().unwrap();
        guard
            .query_row(
                "SELECT initial_balance_cents FROM accounts WHERE id = 'acct-1'",
                [],
                |r| r.get(0),
            )
            .unwrap()
    };
    assert_eq!(balance, 12345, "换回后同一克隆应读到真实库");
}
