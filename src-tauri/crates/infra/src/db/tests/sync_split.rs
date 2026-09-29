//! V036 同步元数据拆库迁移（issue #1871 / ADR-0139 决策 1/4）：升级世界四表
//! 数据分毫不差迁入 sync.db、主库无任何同步表；迁移失败两库整体回滚、重试
//! 收敛（非 WAL master journal 集合级原子的行为面）；sync.db 缺失按世界版本
//! 裁决（拆库世界码化报错、拆库前世界补建后搬迁）；拆库边界版本钉。
//!
//! 拆库前世界夹具经别名裸连接构造（`to_version` 停在 V035、四表在 main，无需
//! attached 侧），归位 `ledger.db` 后走产品建缝（挂载 + 迁移 + 守卫）。

use tauri_app_lib::test_support::{FIXED_NOW, ScratchDir};

use crate::db::connection::{DB_FILE_NAME, SYNC_DB_FILE_NAME, SYNC_SPLIT_USER_VERSION};
use crate::db::{migrations, open_connection_in};
use crate::error::AppError;

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("db-sync-split-{tag}"))
}

/// 拆库前世界（V035 形态：四表在 main、user_version = V035）＋四表各一行数据
/// ＋一行业务数据（app_settings KV，无外键）。别名裸连接：文件名非
/// `ledger.db`，不触发挂载；V035 及之前的迁移只写 main。
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

/// 断言四表数据已分毫不差迁入 attached 侧（逐行逐列比对夹具字面量），
/// 主库不留任何同步表，业务数据原样。
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

/// 升级：带四表数据的拆库前世界迁到最新后，数据分毫不差入 sync.db、主库无
/// 任何 sync 表（验收「升级」；删除迁移搬数步骤即红——行不迁入、断言失败）。
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

/// 迁移中断 = 整体回退（非 WAL master journal 集合级原子，ADR-0139 决策 2）：
/// 搬数中途失败时两库同滚——main 四表与数据原样、attached 无搬迁残迹；排除
/// 故障后重试（重启重跑同一迁移）收敛到拆库形态（验收「中断后再启动收敛」）。
#[test]
fn failed_migration_rolls_back_both_dbs_and_retry_converges() {
    let dir = temp_dir("interrupted");
    let seed = dir.path().join("seed.db");
    {
        let mut conn = pre_split_world(&seed);
        // 故障注入：attached 侧预置残缺同名表（形状漂移的假 sync.db），搬数
        // 在第一条 INSERT 上确定性失败。
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
        // 两库同滚：main 四表与数据原样在位。
        let ops_in_main: i64 = conn
            .query_row("SELECT COUNT(*) FROM main.sync_ops", [], |r| r.get(0))
            .expect("回滚后 main.sync_ops 应原样在位");
        assert_eq!(ops_in_main, 1, "回滚后搬数源行应原样保留");
        // 失败发生在第一条搬数 INSERT：attached 侧除故障注入表外不得有任何
        // 搬迁产物（表未建、行未拷）。
        let copied: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync.sync_device", [], |r| r.get(0))
            .expect("回滚后 attached 侧应只剩故障注入表");
        assert_eq!(copied, 0, "失败迁移不得留下搬迁残迹");
    }

    // 排除故障后重试（新连接 = 重启；open_connection_in 走挂载 + 迁移 + 守卫）。
    std::fs::remove_file(dir.path().join(SYNC_DB_FILE_NAME)).expect("清除故障注入世界");
    std::fs::rename(&seed, dir.path().join(DB_FILE_NAME)).expect("归位主库文件名");
    let conn = open_connection_in(dir.path()).expect("重试应收敛");
    assert_split_world(&conn);
}

/// 拆库世界的 sync.db 缺失 = 同步元数据丢失：建连码化报错，不静默补建空库
/// 冒充完好世界（ADR-0139 决策 3 / spec 用户故事 17）。
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

/// 搬迁窗口的 IF NOT EXISTS 收敛（票 07 跨版本引导与旧形态恢复的底层机制）：
/// attached 侧已有同名空表时复用之、照常搬数——预置空 sync.db 的拆库前世界
/// 升级不因表已存在而失败。
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

/// 拆库边界版本钉：`SYNC_SPLIT_USER_VERSION` = 迁移链位置版本 35（V036，V005
/// 缺位不回填）；边界之前（34 = V035）四表仍在 main。迁移链插位变化在此变红。
#[test]
fn sync_split_user_version_boundary_is_pinned() {
    // 拆库后世界：工厂（建库唯一入口）迁到最新，user_version 恰为边界版本。
    let conn = tauri_app_lib::test_support::open();
    let latest: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(
        latest, SYNC_SPLIT_USER_VERSION,
        "最新版本应恰为拆库边界（V036 的位置版本）"
    );

    // 拆库前世界（与夹具同形的别名裸连接，停在 V035）：四表仍在 main。
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
