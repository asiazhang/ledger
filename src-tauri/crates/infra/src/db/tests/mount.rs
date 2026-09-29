//! 同步元数据库挂载（issue #1869 / ADR-0139 决策 3/4）：建连收尾单点把
//! `sync.db` ATTACH 为 `sync`——读侧只读形态挂载（写被拒）、密文形态同主口令
//! KEY 注入、损坏/形态错配码化报错不静默降级、挂载先于迁移 runner（ATTACH
//! 不能在事务内执行，决策 4 的迁移事务外挂载机制）。V036 拆库后（issue
//! #1871）：四张同步表住 attached 侧，写侧文件缺失按世界版本裁决——拆库世界
//! 缺失即元数据丢失报码化错误、拆库前世界自动补建后由 V036 搬迁；
//! unqualified 表名解析优先 main 只在两侧同名并存的搬迁窗口内成立。
//!
//! 文件库不入测试工厂（ADR-0084 决策 3）：建库经产品建缝 `open_connection*`
//! 系（挂载接线即在其中），迁移经产品迁移缝 `migrations().to_latest`（readonly/boot tests
//! 先例；测试侧不直呼 `init_db`，ADR-0084 守门纪律）。别名名下的非 ledger.db
//! 夹具连接不触发挂载，跑迁移链前需自行 `ATTACH ':memory:' AS sync`（内存
//! 世界成对挂载，与产品 `open_in_memory` 同形）。

use std::io::Read;

use tauri_app_lib::test_support::ScratchDir;

use crate::db::connection::{DB_FILE_NAME, SYNC_DB_FILE_NAME, SYNC_TABLES};
use crate::db::encryption::SQLITE_HEADER_MAGIC;
use crate::db::{
    migrations, open_connection, open_connection_in, open_connection_readonly_in,
    open_connection_with_passphrase, reset_db_file,
};

/// 暂存目录（ScratchDir guard，issue #1645）：drop（含 panic unwind）整棵删除。
fn temp_dir(tag: &str) -> ScratchDir {
    ScratchDir::new(&format!("db-mount-{tag}"))
}

/// 已挂载的库别名清单（`PRAGMA database_list` 第 2 列为别名）。
fn attached_aliases(conn: &rusqlite::Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("PRAGMA database_list")
        .expect("准备 database_list");
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .expect("查询 database_list");
    rows.map(|r| r.expect("database_list 行")).collect()
}

/// attached 侧的表名清单。
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

/// 写侧建连挂载 sync.db：文件缺失时自动创建空库；全新安装的迁移链在此世界
/// 直达双库布局——四张同步表恰以闭集落 attached 侧、主库无任何同步表
/// （V036 拆库，ADR-0139 决策 1/4；验收「全新安装直接得到双库布局」）。
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
    // 迁移链尾部的裸 ANALYZE（V016/V025/V030/V031）作用于连接上全部 attached
    // 库，会在 attached 侧产出 sqlite_stat1/4（SQLite 内部统计表，无用户语义；
    // schema_guard 对 sqlite_% 的豁免先例同源）。用户表恰为四表闭集。
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

/// 密文库带 KEY 挂载：sync.db 与主库同口令派生密钥，落盘为密文
/// （SQLCipher 无 KEY 挂载必然失败，同 KEY 显式注入——ADR-0139 决策 3）。
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
    // 同 KEY 挂载后 attached 侧可写（写一次触发落盘，供文件头断言）。
    conn.execute("CREATE TABLE sync.mount_probe(x)", [])
        .expect("同 KEY 挂载后 attached 侧应可写");
    // sync.db 落盘为密文：头部为随机盐，绝非明文魔数。
    let sync_path = dir.path().join(SYNC_DB_FILE_NAME);
    assert_ne!(
        read_header(&sync_path),
        SQLITE_HEADER_MAGIC,
        "密文主库挂载创建的 sync.db 应为密文"
    );
}

/// 只读连接只读形态挂载：挂载成功、对 attached 侧写被拒
/// （读路径无写从纪律变成运行时约束，ADR-0117 代价 3 同款）。
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

/// 只读侧挂载不会把写面放大：只读连接对主库写同样被拒（既有约束回归锚）。
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

/// sync.db 损坏：挂载点码化报错，不静默降级（写侧与只读侧同码）。
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

/// 形态错配（密文 sync.db 遇明文主库——转换未配对的历史残留形态）：
/// 同样码化报错，不静默降级。密文 sync.db 须经真实写入落盘成形
/// （SQLCipher 文件头在首个写事务落盘，空连接不留密文痕迹）。
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

/// expand 之前的旧世界目录（只有 ledger.db、无 sync.db）只读开启不受挂载
/// 影响：跳过挂载，行为与拆库前一致（跨账本只读聚合等场景的兼容面）。
/// 主库须经别名建库后归位（写侧建连会在同目录补建 sync.db，不能作为
/// 旧世界夹具）。
#[test]
fn readonly_side_tolerates_missing_sync_db() {
    let dir = temp_dir("legacy");
    let seed_path = dir.path().join("seed.db");
    {
        let mut conn = open_connection(&seed_path).expect("别名建库（不触发挂载）");
        conn.execute_batch("ATTACH DATABASE ':memory:' AS sync")
            .expect("内存世界成对挂载（别名夹具不触发挂载接线）");
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

/// unqualified 表名解析优先 main（expand 期行为零变化的前提，也是票 05
/// 迁移 `DROP TABLE sync_ops` 语义的落点：主库同名表先命中）。
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

/// 挂载先于迁移 runner 且跨 runner 存续（决策 4：迁移事务外挂载机制）：
/// 迁移 runner（`to_latest`，`init_db` 的迁移核心）跑链期间 attached 已在、跑完仍在；覆盖两库的
/// 单一事务提交/回滚成立（V036 起迁移与业务写入同款前提）。
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

    // 覆盖两库的单一事务：提交同生，回滚共死。
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

/// sync.db 不可开（路径被目录占据，CANTOPEN 类——AC「损坏 / 不可开」的
/// 不可开侧）：挂载点码化报错，与损坏同码。
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

/// 启动失败重置逃生门与挂载共存（issue #601 × ADR-0139 决策 3 配对）：把用户
/// 送进失败恢复屏的原因可能正是 sync.db 损坏，重置必须把遗留文件一并移位
/// 保留、产出可开启的新世界——否则逃生门被同一损坏堵死、无法自愈
/// （回归锚：重置不移 sync.db 即本测试红）。
#[test]
fn reset_db_file_moves_stale_sync_db_aside() {
    let dir = temp_dir("reset-escape");
    // 失败世界夹具：主库经别名建库归位（写侧建连会提前补建 sync.db，不能作
    // 失败世界前置），sync.db 手工置为损坏字节。
    let seed_path = dir.path().join("seed.db");
    {
        let mut conn = open_connection(&seed_path).expect("别名建库");
        conn.execute_batch("ATTACH DATABASE ':memory:' AS sync")
            .expect("内存世界成对挂载（别名夹具不触发挂载接线）");
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

    // 旧世界两份遗留均按重置命名语义保留；新世界可正常开启。
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
