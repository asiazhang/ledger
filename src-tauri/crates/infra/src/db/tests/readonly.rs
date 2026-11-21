use std::time::{Duration, Instant};

use tauri_app_lib::test_support::{ScratchDir, ScratchFile};

use crate::db::{migrations, open_connection, open_connection_with_passphrase};

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("db-readonly-{tag}"))
}

fn seeded_db(tag: &str, account: &str, balance: i64) -> ScratchFile {
    let file = ScratchFile::new(
        &format!("db-readonly-{tag}"),
        crate::db::connection::DB_FILE_NAME,
    );
    let mut conn = open_connection(&file).unwrap();
    migrations().to_latest(&mut conn).unwrap();
    tauri_app_lib::test_support::seed_account(&conn, account, "现金", "cash", "CNY", balance);
    file
}

fn plaintext_db(tag: &str) -> ScratchFile {
    seeded_db(tag, "acct-1", 12345)
}

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

#[test]
fn paired_connections_share_concurrent_busy_timeout() {
    let dir = temp_dir("busy");
    let state = crate::db::open_db_in(&dir).unwrap();
    for (label, conn) in [("write", &state.conn), ("read", &state.read_conn)] {
        let busy_ms: i64 = conn
            .lock()
            .unwrap()
            .query_row("PRAGMA busy_timeout", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            busy_ms,
            crate::db::CONCURRENT_BUSY_TIMEOUT.as_millis() as i64,
            "{label} 连接应持同源并发 busy 容让，实际 {busy_ms}ms"
        );
    }
}

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

    let wrong = crate::db::open_connection_readonly_with_passphrase(&db, "主口令-错误");
    let err = wrong.expect_err("错误口令的只读建连应在挂载探针即失败");
    assert_eq!(
        err.code(),
        Some("encryption.passphrase-incorrect"),
        "错误口令应报合并口径码化错误，实际: {err}"
    );
}

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

#[test]
fn open_db_in_pairs_write_and_read_connections() {
    let dir = temp_dir("pair");
    let state = crate::db::open_db_in(&dir).unwrap();
    {
        let guard = state.conn.lock().unwrap_or_else(|e| e.into_inner());
        crate::db::write_locked(&guard, |conn| {
            tauri_app_lib::test_support::seed_account(conn, "acct-p", "成对", "cash", "CNY", 777);
            Ok(())
        })
        .unwrap();
    }
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

#[test]
fn read_slot_replacement_and_placeholderization() {
    let db = plaintext_db("slot");
    let state = crate::db::open_db_in(db.parent().unwrap()).unwrap();
    let cloned = state.read_conn.clone();

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

#[test]
fn swap_pair_swaps_both_slots_visible_via_held_clones() {
    let old_db = seeded_db("swap-old", "acct-old", 1111);
    let new_db = seeded_db("swap-new", "acct-new", 2222);
    let state = crate::db::open_db_in(old_db.parent().unwrap()).unwrap();
    let write_clone = state.conn.clone();
    let read_clone = state.read_conn.clone();

    state
        .swap_pair(
            crate::db::open_connection_in(new_db.parent().unwrap()).unwrap(),
            crate::db::open_connection_readonly_in(new_db.parent().unwrap()).unwrap(),
        )
        .unwrap();

    for (slot_name, slot) in [("写槽", &write_clone), ("读槽", &read_clone)] {
        let guard = slot.lock().unwrap();
        let balance: i64 = guard
            .query_row(
                "SELECT initial_balance_cents FROM accounts WHERE id = 'acct-new'",
                [],
                |r| r.get(0),
            )
            .unwrap_or_else(|e| panic!("{slot_name} 应指向新库（acct-new 可读），实际 {e}"));
        assert_eq!(balance, 2222, "{slot_name} 读到的应为新库数据");
        assert!(
            guard
                .query_row::<i64, _, _>(
                    "SELECT initial_balance_cents FROM accounts WHERE id = 'acct-old'",
                    [],
                    |r| r.get(0),
                )
                .is_err(),
            "{slot_name} 不应再看到旧库数据（acct-old 仅存在于旧库）"
        );
    }
}
