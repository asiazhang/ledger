use std::io::Read;

use tauri_app_lib::test_support::ScratchDir;

use crate::db::connection::{DB_FILE_NAME, SYNC_DB_FILE_NAME, SYNC_TABLES};
use crate::db::encryption::SQLITE_HEADER_MAGIC;
use crate::db::{
    migrations, open_connection, open_connection_in, open_connection_readonly_in,
    open_connection_with_passphrase, reset_db_file,
};

fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("db-mount-{tag}"))
}

fn attached_aliases(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("PRAGMA database_list")
        .expect("准备 database_list");
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .expect("查询 database_list");
    rows.map(|r| r.expect("database_list 行")).collect()
}

fn attached_tables(conn: &rusqlite::Connection, db: &str) -> Vec<String> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT name FROM {db}.sqlite_master WHERE type = 'table'"
        ))
        .expect("准备 attached sqlite_master 查询");
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).expect("查询");
    rows.map(|r| r.expect("sqlite_master 行")).collect()
}

fn read_header(db: &std::path::Path) -> [u8; 16] {
    let mut file = std::fs::File::open(db).expect("打开库文件");
    let mut header = [0u8; 16];
    file.read_exact(&mut header).expect("读取文件头");
    header
}

#[test]
fn fresh_install_lands_dual_layout_four_tables_in_attached() {
    let dir = temp_dir("write-create");
    let conn = open_connection_in(dir.path()).expect("建连");

    let aliases = attached_aliases(&conn);
    assert!(
        aliases.iter().any(|a| a == "sync"),
        "写连接应挂载 sync：实际 {aliases:?}"
    );
    let sync_path = dir.path().join(SYNC_DB_FILE_NAME);
    assert!(sync_path.exists(), "挂载应创建 sync.db 文件");
    let tables = attached_tables(&conn, "sync");
    let user_tables: Vec<_> = tables
        .iter()
        .filter(|t| !t.starts_with("sqlite_"))
        .collect();
    assert_eq!(
        user_tables.len(),
        SYNC_TABLES.len(),
        "attached 侧用户表应恰为四表闭集：实际 {tables:?}"
    );
    let main_tables = attached_tables(&conn, "main");
    for table in SYNC_TABLES {
        assert!(
            tables.iter().any(|t| t == table),
            "同步表 {table} 应在 attached 侧"
        );
        assert!(
            !main_tables.iter().any(|t| t == table),
            "同步表 {table} 不应留在主库"
        );
    }
}

#[test]
fn encrypted_write_side_mounts_with_key() {
    let dir = temp_dir("enc-key");
    let conn = open_connection_with_passphrase(dir.path().join(DB_FILE_NAME), "主口令-正确")
        .expect("建连");

    let aliases = attached_aliases(&conn);
    assert!(
        aliases.iter().any(|a| a == "sync"),
        "密文写连接应带 KEY 挂载 sync：实际 {aliases:?}"
    );
    conn.execute("CREATE TABLE sync.mount_probe(x)", [])
        .expect("同 KEY 挂载后 attached 侧应可写");
    let sync_path = dir.path().join(SYNC_DB_FILE_NAME);
    assert_ne!(
        read_header(&sync_path),
        SQLITE_HEADER_MAGIC,
        "密文主库挂载创建的 sync.db 应为密文"
    );
}

#[test]
fn readonly_connection_mounts_sync_and_rejects_writes() {
    let dir = temp_dir("ro-form");
    drop(open_connection_in(dir.path()).expect("写侧建连（建立双文件）"));

    let ro = open_connection_readonly_in(dir.path()).expect("只读建连");
    let aliases = attached_aliases(&ro);
    assert!(
        aliases.iter().any(|a| a == "sync"),
        "只读连接应挂载 sync：实际 {aliases:?}"
    );
    let err = ro
        .execute("CREATE TABLE sync.mount_probe(x)", [])
        .expect_err("只读连接对 attached 侧写应被拒");
    assert!(
        matches!(
            err,
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ReadOnly
        ),
        "应报只读库错误（SQLITE_READONLY）：{err}"
    );
}

#[test]
fn readonly_connection_still_rejects_main_writes() {
    let dir = temp_dir("ro-main");
    drop(open_connection_in(dir.path()).expect("写侧建连"));
    let ro = open_connection_readonly_in(dir.path()).expect("只读建连");
    let err = ro
        .execute("CREATE TABLE mount_probe(x)", [])
        .expect_err("只读连接对主库写应被拒");
    assert!(
        matches!(
            err,
            rusqlite::Error::SqliteFailure(f, _) if f.code == rusqlite::ErrorCode::ReadOnly
        ),
        "应报只读库错误（SQLITE_READONLY）：{err}"
    );
}

#[test]
fn corrupt_sync_db_reports_coded_error() {
    let dir = temp_dir("corrupt");
    std::fs::write(dir.path().join(SYNC_DB_FILE_NAME), b"corrupt bytes").expect("写损坏文件");

    let err = open_connection_in(dir.path()).expect_err("损坏 sync.db 应挂载失败");
    assert_eq!(
        err.code(),
        Some("db.sync-mount-failed"),
        "写侧应报码化挂载错误：{err}"
    );
    let err = open_connection_readonly_in(dir.path()).expect_err("损坏 sync.db 应挂载失败");
    assert_eq!(
        err.code(),
        Some("db.sync-mount-failed"),
        "只读侧应报码化挂载错误：{err}"
    );
}

#[test]
fn form_mismatched_sync_db_reports_coded_error() {
    let dir = temp_dir("form-mismatch");
    let sync_path = dir.path().join(SYNC_DB_FILE_NAME);
    {
        let conn = open_connection_with_passphrase(&sync_path, "旧口令").expect("建立密文 sync.db");
        conn.execute("CREATE TABLE mount_probe(x)", [])
            .expect("写入触发密文落盘");
    }

    let err = open_connection_in(dir.path()).expect_err("形态错配应挂载失败");
    assert_eq!(
        err.code(),
        Some("db.sync-mount-failed"),
        "应报码化挂载错误：{err}"
    );
}

#[test]
fn readonly_side_tolerates_missing_sync_db() {
    let dir = temp_dir("legacy");
    let seed_path = dir.path().join("seed.db");
    {
        let mut conn = open_connection(&seed_path).expect("别名建库（不触发挂载）");
        crate::test_utils::attach_in_memory_sync(&conn);
        migrations().to_latest(&mut conn).expect("迁移");
        drop(conn);
        std::fs::rename(&seed_path, dir.path().join(DB_FILE_NAME)).expect("归位主库文件名");
    }
    assert!(
        !dir.path().join(SYNC_DB_FILE_NAME).exists(),
        "夹具前置：旧世界目录不应有 sync.db"
    );
    let ro = open_connection_readonly_in(dir.path()).expect("旧世界目录只读开启应成功");
    let aliases = attached_aliases(&ro);
    assert!(
        !aliases.iter().any(|a| a == "sync"),
        "无 sync.db 的旧世界目录不应挂载：实际 {aliases:?}"
    );
}

#[test]
fn unqualified_table_names_resolve_to_main_first() {
    let dir = temp_dir("resolve-main");
    let conn = open_connection_in(dir.path()).expect("建连");

    conn.execute("CREATE TABLE sync.mount_probe(v TEXT)", [])
        .expect("attached 侧建表");
    conn.execute("CREATE TABLE main.mount_probe(v TEXT)", [])
        .expect("main 侧建表");
    conn.execute("INSERT INTO main.mount_probe VALUES ('main')", [])
        .expect("写 main 侧");
    conn.execute("INSERT INTO sync.mount_probe VALUES ('sync')", [])
        .expect("写 attached 侧");

    let v: String = conn
        .query_row("SELECT v FROM mount_probe", [], |r| r.get(0))
        .expect("unqualified 查询应命中");
    assert_eq!(v, "main", "unqualified 表名应解析到 main");
}

#[test]
fn mount_precedes_migrations_and_spans_both_dbs() {
    let dir = temp_dir("pre-migrate");
    let mut conn = open_connection(dir.path().join(DB_FILE_NAME)).expect("建连（挂载在内）");

    assert!(
        attached_aliases(&conn).iter().any(|a| a == "sync"),
        "迁移 runner 之前 attached 应已就位"
    );
    migrations().to_latest(&mut conn).expect("迁移");
    let aliases = attached_aliases(&conn);
    assert!(
        aliases.iter().any(|a| a == "sync"),
        "迁移后挂载应存续：实际 {aliases:?}"
    );

    conn.execute_batch(
        "BEGIN;
         CREATE TABLE main.mount_probe(x INTEGER);
         CREATE TABLE sync.mount_probe(x INTEGER);
         INSERT INTO main.mount_probe VALUES (1);
         INSERT INTO sync.mount_probe VALUES (2);
         COMMIT;",
    )
    .expect("跨库事务提交");
    let main_v: i64 = conn
        .query_row("SELECT x FROM main.mount_probe", [], |r| r.get(0))
        .expect("main 侧提交可见");
    let sync_v: i64 = conn
        .query_row("SELECT x FROM sync.mount_probe", [], |r| r.get(0))
        .expect("attached 侧提交可见");
    assert_eq!((main_v, sync_v), (1, 2));

    conn.execute_batch(
        "BEGIN;
         DELETE FROM main.mount_probe;
         DELETE FROM sync.mount_probe;
         ROLLBACK;",
    )
    .expect("跨库事务回滚");
    let main_v: i64 = conn
        .query_row("SELECT x FROM main.mount_probe", [], |r| r.get(0))
        .expect("main 侧回滚恢复");
    let sync_v: i64 = conn
        .query_row("SELECT x FROM sync.mount_probe", [], |r| r.get(0))
        .expect("attached 侧回滚恢复");
    assert_eq!((main_v, sync_v), (1, 2), "回滚应覆盖两库");
}

#[test]
fn unopenable_sync_db_reports_coded_error() {
    let dir = temp_dir("unopenable");
    std::fs::create_dir(dir.path().join(SYNC_DB_FILE_NAME)).expect("以目录占据 sync.db 路径");
    let err = open_connection_in(dir.path()).expect_err("不可开的 sync.db 应挂载失败");
    assert_eq!(
        err.code(),
        Some("db.sync-mount-failed"),
        "应报码化挂载错误：{err}"
    );
}

#[test]
fn reset_db_file_moves_stale_sync_db_aside() {
    let dir = temp_dir("reset-escape");
    let seed_path = dir.path().join("seed.db");
    {
        let mut conn = open_connection(&seed_path).expect("别名建库");
        crate::test_utils::attach_in_memory_sync(&conn);
        migrations().to_latest(&mut conn).expect("迁移");
        drop(conn);
        std::fs::rename(&seed_path, dir.path().join(DB_FILE_NAME)).expect("归位主库文件名");
    }
    let sync_path = dir.path().join(SYNC_DB_FILE_NAME);
    std::fs::write(&sync_path, b"corrupt sync bytes").expect("制造损坏 sync.db");
    assert!(
        open_connection_in(dir.path()).is_err(),
        "前置：损坏 sync.db 的世界应开启失败（失败恢复屏来由）"
    );

    let conn = reset_db_file(dir.path()).expect("重置应成功（不被损坏 sync.db 堵死）");
    drop(conn);

    assert!(
        dir.path().join("ledger.db.bak").exists(),
        "旧主库应保留 .bak"
    );
    let sync_bak = sync_path.with_extension("db.bak");
    assert!(sync_bak.exists(), "旧 sync.db 应保留 .bak 副本");
    assert_eq!(
        std::fs::read(&sync_bak).unwrap(),
        b"corrupt sync bytes",
        "移位副本应原样保留"
    );
    let conn = open_connection_in(dir.path()).expect("新世界应可正常开启");
    assert!(
        attached_aliases(&conn).iter().any(|a| a == "sync"),
        "新世界 sync.db 应由挂载接线按明文形态补建并挂载"
    );
}
