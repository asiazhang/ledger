use tauri_app_lib::test_support::{FIXED_NOW, ScratchDir};

use crate::db::connection::{DB_FILE_NAME, SYNC_DB_FILE_NAME, SYNC_SPLIT_USER_VERSION};
use crate::db::{migrations, open_connection_in};
use crate::error::AppError;

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("db-sync-split-{tag}"))
}

fn pre_split_world(path: &std::path::Path) -> rusqlite::Connection {
    let mut conn = crate::db::open_connection_unmounted(path).expect("别名裸连接");
    migrations()
        .to_version(&mut conn, (SYNC_SPLIT_USER_VERSION - 1) as usize)
        .expect("停在 V035（拆库边界之前）");
    conn.execute(
        &format!(
            "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) \
             VALUES ('dev-1', 5, '{FIXED_NOW}', '2026-01-02T00:00:00Z')"
        ),
        [],
    )
    .unwrap();
    conn.execute(
        &format!(
            "INSERT INTO sync_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at) \
             VALUES ('op-1', 'dev-1', 1, 34, 'transaction', 'txn-1', '{{\"k\":1}}', '{FIXED_NOW}')"
        ),
        [],
    )
    .unwrap();
    conn.execute(
        &format!(
            "INSERT INTO sync_parked_ops (op_id, device_id, clock, schema_version, entity, entity_id, payload, park_code, park_params, park_message, parked_at) \
             VALUES ('pop-1', 'dev-2', 2, 34, 'transaction', 'txn-2', '{{\"k\":2}}', 'sync-engine.op-foreign-key', '[\"acc-1\"]', '挂起', '{FIXED_NOW}')"
        ),
        [],
    )
    .unwrap();
    conn.execute(
        &format!(
            "INSERT INTO sync_stream_positions (device_id, applied_through, updated_at) \
             VALUES ('dev-2', 3, '{FIXED_NOW}')"
        ),
        [],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES ('probe.key', 'probe-value')",
        [],
    )
    .unwrap();
    conn
}

fn assert_split_world(conn: &rusqlite::Connection) {
    let (id, clock, created, updated): (String, i64, String, String) = conn
        .query_row(
            "SELECT id, logical_clock, created_at, updated_at FROM sync.sync_device",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        (id.as_str(), clock, created.as_str(), updated.as_str()),
        ("dev-1", 5, FIXED_NOW, "2026-01-02T00:00:00Z"),
        "sync_device 行应分毫不差"
    );

    let row: (String, String, i64, i64, String, String, String, String) = conn
        .query_row(
            "SELECT op_id, device_id, clock, schema_version, entity, entity_id, payload, recorded_at \
             FROM sync.sync_ops WHERE op_id = 'op-1'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .unwrap();
    assert_eq!(
        row,
        (
            "op-1".into(),
            "dev-1".into(),
            1,
            34,
            "transaction".into(),
            "txn-1".into(),
            "{\"k\":1}".into(),
            FIXED_NOW.into()
        ),
        "sync_ops 行应分毫不差（含 entity_id 迁移列）"
    );

    let parked: (String, String, String, String, String) = conn
        .query_row(
            "SELECT op_id, park_code, park_params, park_message, entity_id \
             FROM sync.sync_parked_ops WHERE op_id = 'pop-1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(
        parked,
        (
            "pop-1".into(),
            "sync-engine.op-foreign-key".into(),
            "[\"acc-1\"]".into(),
            "挂起".into(),
            "txn-2".into()
        ),
        "sync_parked_ops 行应分毫不差"
    );

    let position: (String, i64, String) = conn
        .query_row(
            "SELECT device_id, applied_through, updated_at FROM sync.sync_stream_positions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        position,
        ("dev-2".into(), 3, FIXED_NOW.into()),
        "sync_stream_positions 行应分毫不差"
    );

    for table in crate::db::SYNC_TABLES {
        let in_main: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(in_main, 0, "主库不应残留同步表 {table}");
    }

    let setting: String = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = 'probe.key'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(setting, "probe-value", "业务数据应原样保留在主库");
}

#[test]
fn upgrade_moves_sync_rows_intact_into_sync_db() {
    let dir = temp_dir("upgrade");
    let seed = dir.path().join("seed.db");
    pre_split_world(&seed);
    std::fs::rename(&seed, dir.path().join(DB_FILE_NAME)).expect("归位主库文件名");
    assert!(
        !dir.path().join(SYNC_DB_FILE_NAME).exists(),
        "夹具前置：拆库前世界无 sync.db"
    );

    let conn = open_connection_in(dir.path()).expect("升级建连应成功（补建 + V036 搬迁）");
    assert_split_world(&conn);
}

#[test]
fn failed_migration_rolls_back_both_dbs_and_retry_converges() {
    let dir = temp_dir("interrupted");
    let seed = dir.path().join("seed.db");
    {
        let mut conn = pre_split_world(&seed);
        conn.execute_batch(&format!(
            "ATTACH DATABASE '{}' AS sync; CREATE TABLE sync.sync_device (x)",
            dir.path().join(SYNC_DB_FILE_NAME).display()
        ))
        .unwrap();
        let err = migrations()
            .to_latest(&mut conn)
            .expect_err("搬数应在残缺表上失败");
        assert!(
            err.to_string().contains("sync_device"),
            "失败应发生在 sync_device 搬数：{err}"
        );
        let ops_in_main: i64 = conn
            .query_row("SELECT COUNT(*) FROM main.sync_ops", [], |r| r.get(0))
            .expect("回滚后 main.sync_ops 应原样在位");
        assert_eq!(ops_in_main, 1, "回滚后搬数源行应原样保留");
        let copied: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync.sync_device", [], |r| r.get(0))
            .expect("回滚后 attached 侧应只剩故障注入表");
        assert_eq!(copied, 0, "失败迁移不得留下搬迁残迹");
    }

    std::fs::remove_file(dir.path().join(SYNC_DB_FILE_NAME)).expect("清除故障注入世界");
    std::fs::rename(&seed, dir.path().join(DB_FILE_NAME)).expect("归位主库文件名");
    let conn = open_connection_in(dir.path()).expect("重试应收敛");
    assert_split_world(&conn);
}

#[test]
fn split_world_missing_sync_db_is_coded_mount_error() {
    let dir = temp_dir("missing");
    let conn = open_connection_in(dir.path()).expect("首建双库世界");
    drop(conn);
    std::fs::remove_file(dir.path().join(SYNC_DB_FILE_NAME)).expect("删除 sync.db");

    let err = open_connection_in(dir.path()).expect_err("拆库世界缺失 sync.db 应开启失败");
    match &err {
        AppError::Coded { code, .. } => {
            assert_eq!(
                code.as_str(),
                "db.sync-mount-failed",
                "应报挂载失败码：{err}"
            );
        }
        other => panic!("应报码化错误：{other:?}"),
    }
}

#[test]
fn pre_split_world_with_empty_sync_db_reuses_tables_and_converges() {
    let dir = temp_dir("reuse-empty");
    let seed = dir.path().join("seed.db");
    pre_split_world(&seed);
    std::fs::rename(&seed, dir.path().join(DB_FILE_NAME)).expect("归位主库文件名");
    std::fs::write(dir.path().join(SYNC_DB_FILE_NAME), b"").expect("预置空 sync.db 文件");

    let conn = open_connection_in(dir.path()).expect("升级建连应成功（复用空表搬数）");
    assert_split_world(&conn);
}

#[test]
fn sync_split_user_version_boundary_is_pinned() {
    assert_eq!(
        SYNC_SPLIT_USER_VERSION, 35,
        "拆库边界应钉在 V036 的位置版本 35"
    );

    let conn = tauri_app_lib::test_support::open();
    let latest: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert!(
        latest >= SYNC_SPLIT_USER_VERSION,
        "最新版本 {latest} 不应早于拆库边界 {SYNC_SPLIT_USER_VERSION}"
    );

    let dir = temp_dir("boundary");
    let path = dir.path().join("pre-split.db");
    let mut pre_split = crate::db::open_connection_unmounted(&path).expect("别名裸连接");
    migrations()
        .to_version(&mut pre_split, (SYNC_SPLIT_USER_VERSION - 1) as usize)
        .expect("停在拆库前");
    let in_main: i64 = pre_split
        .query_row(
            "SELECT COUNT(*) FROM main.sqlite_master WHERE type = 'table' AND name = 'sync_ops'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(in_main, 1, "拆库边界之前四表应在 main");
}
