//! 建连 / 重置 / 完整性检查 / 内存库（自 `db/mod.rs` 按职责拆出，issue #1127，
//! 纯移动）。建连收尾单点 [`finish_open`]：密钥注入 → 外键 → 耗时 hook；
//! 全部生产建连路径统一收口 [`init_db`]（schema 守卫尾部接线，ADR-0100）。

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use super::migrate::init_db;
use super::runtime::DbState;
use super::{data_location, perf_trace};
use crate::error::{AppError, Result};

/// 在指定目录打开库并完成 schema 迁移，返回裸连接（原位重引导的连接换入用：
/// 换入目标是既有 [`DbState`] 的互斥体内槽，需要裸 `Connection` 才能移入，
/// issue #644 / ADR-0080）。启动期唯一入口：先经 [`super::data_location::boot`] 解析
/// 库所在目录，再调本函数建连；不要自行拼接库路径。
pub fn open_connection_in(db_dir: &Path) -> Result<Connection> {
    let db_path = db_dir.join(data_location::DB_FILE_NAME);
    tracing::info!(db_path = %db_path.display(), "打开数据库");
    let mut conn = open_connection(db_path)?;
    init_db(&mut conn)?;
    Ok(conn)
}

/// 在指定目录打开库并完成 schema 迁移（DataLocation 引导之后的建连步骤）：
/// 裸连接包成共享锁形态。
pub fn open_db_in(db_dir: &Path) -> Result<DbState> {
    let conn = open_connection_in(db_dir)?;
    Ok(DbState {
        conn: Arc::new(Mutex::new(conn)),
    })
}

/// 启动失败重置兜底：把当前库改名 `.bak` 保留后重新打开（新建空库）。
/// 只作用于引导解析出的生效目录，绝不删除任何文件。
pub fn reset_db_in(db_dir: &Path) -> Result<DbState> {
    Ok(DbState {
        conn: Arc::new(Mutex::new(reset_db_file(db_dir)?)),
    })
}

/// 重置核心（连接形态，issue #601）：把当前库改名 `.bak` 保留后原位新建
/// 明文空库（建连 + 迁移 + 完整性检查，重置产物验收基准与
/// [`super::encryption::open_new_plaintext_db`] 同口径）。返回连接供启动失败
/// 恢复通道原位换入存活 [`DbState`]（占位连接 → 真实新库，无需重启）。
pub fn reset_db_file(db_dir: &Path) -> Result<Connection> {
    let db_path = db_dir.join(data_location::DB_FILE_NAME);
    let bak_path = db_path.with_extension("db.bak");
    std::fs::rename(&db_path, &bak_path).ok();
    tracing::info!(bak = %bak_path.display(), "已备份原数据库并重置");
    let mut conn = open_connection(&db_path)?;
    init_db(&mut conn)?;
    check_integrity(&conn)?;
    Ok(conn)
}

/// 校验数据库文件完整性（`PRAGMA integrity_check` 应返回 `ok`）。
pub fn check_integrity(conn: &Connection) -> Result<()> {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .map_err(AppError::from)?;
    if result != "ok" {
        // ADR-0050 码化收口（#1072）：构造点在基础设施（启动/备份恢复/checkpoint
        // 三域共用），码归 `db.*` 而非 `boot.*`——按启动场景命名会把备份与同步的
        // 同名失败误标为启动条件；message 逐字保留、检查结果进 params。
        return Err(AppError::codedp(
            "db.integrity-check-failed",
            format!("数据库完整性检查失败: {result}"),
            &[result.as_str()],
        ));
    }
    Ok(())
}

/// 打开数据库连接并启用外键约束（SQLite 默认关闭，需每次连接显式开启）。
/// 所有数据库连接都应通过此函数或其派生函数创建，以保证外键生效。
/// 同时注册耗时 hook（[`super::perf_trace`]），覆盖所有 SQL 执行上下文。
/// 明文路径：不设密钥，行为与 SQLCipher 引擎基座引入前完全一致
/// （issue #569 不变量：未设密钥的连接保持明文）。
pub fn open_connection<P: AsRef<Path>>(path: P) -> Result<Connection> {
    finish_open(Connection::open(path)?, None)
}

/// 以主口令打开数据库连接（issue #569 / ADR-0075）。
///
/// 建连接缝单点的密钥侧：密钥注入集中在本函数内部一处，业务路径
/// 不散布密钥知识。参数是**主口令**（passphrase，SQLCipher 按默认
/// KDF 参数派生密钥，ADR-0075 决策 3），不是派生密钥本体，也不是
/// raw key 字符串。调用方必须先经 [`super::encryption::probe_file_kind`]
/// 确认文件确为密文库（[`super::encryption::DbFileKind::Encrypted`]）——对
/// 明文库设密钥后首条读语句会报「file is not a database」。口令错误的
/// 失败同样发生在首条读语句（口令错误 ≠ 库损坏，由探测区分）。
pub fn open_connection_with_passphrase<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    finish_open(Connection::open(path)?, Some(passphrase))
}

/// 建连收尾单点：密钥注入（如有）→ 外键 → 耗时 hook。
fn finish_open(conn: Connection, passphrase: Option<&str>) -> Result<Connection> {
    if let Some(passphrase) = passphrase {
        // `PRAGMA key` 必须是连接上第一条语句；PRAGMA 不支持绑定参数，
        // 经 `pragma_update` 以转义后的 SQL 字面量注入。耗时 hook 尚未安装，
        // 语句文本（含主口令）不会进入 trace 输出（ADR-0075：日志与 trace
        // 不落主口令）——调整顺序即破坏该纪律。
        conn.pragma_update(None, "key", passphrase)?;
    }
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
}

/// 打开内存数据库连接并启用外键约束（用于测试和 BDD 集成测试）。
/// 与文件库路径共用建连收尾单点（外键、耗时 hook）。
pub fn open_in_memory() -> Result<Connection> {
    finish_open(Connection::open_in_memory()?, None)
}
