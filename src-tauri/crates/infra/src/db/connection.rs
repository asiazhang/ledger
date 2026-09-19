//! 建连 / 重置 / 完整性检查 / 内存库（自 `db/mod.rs` 按职责拆出，issue #1127，
//! 纯移动）。建连收尾单点 `finish_open`（本模块私有）：密钥注入 → 外键 → 耗时 hook；
//! 全部生产建连路径统一收口 [`init_db`]（schema 守卫尾部接线，ADR-0100）。

use std::path::Path;
use std::time::Duration;

use rusqlite::Connection;

use super::migrate::init_db;
use super::perf_trace;
use super::runtime::DbState;
use crate::error::{AppError, Result};

/// 库文件名（固定，不可配置；spec：只选目录、文件名由应用固定）。库文件
/// 机制归 db（ADR-0111 决策 4：db 不引用引导层），引导层经
/// [`crate::boot::data_location`] 再导出消费，外部原路径零改动。
pub const DB_FILE_NAME: &str = "ledger.db";

/// 并发容让的 busy_timeout（读路径独立只读连接，issue #1280 / ADR-0117 决策 4）：
/// 读连接在写事务取 EXCLUSIVE 锁的提交瞬间窗口内在本超时内等待——取值与既有
/// 整库转换连接的容让超时同源收口（不在调用点散布），转换连接自本常量取值。
pub const CONCURRENT_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// 在指定目录打开库并完成 schema 迁移，返回裸连接（原位重引导的连接换入用：
/// 换入目标是既有 [`DbState`] 的互斥体内槽，需要裸 `Connection` 才能移入，
/// issue #644 / ADR-0080）。启动期唯一入口：先经 [`crate::boot::data_location::boot`] 解析
/// 库所在目录，再调本函数建连；不要自行拼接库路径。
pub fn open_connection_in(db_dir: &Path) -> Result<Connection> {
    let db_path = db_dir.join(DB_FILE_NAME);
    tracing::info!(db_path = %db_path.display(), "打开数据库");
    let mut conn = open_connection(db_path)?;
    init_db(&mut conn)?;
    Ok(conn)
}

/// 在指定目录打开库并完成 schema 迁移（DataLocation 引导之后的建连步骤）：
/// 写连接 + 只读读连接成对打开（读路径独立只读连接，issue #1280 / ADR-0117）：
/// 迁移在写连接上完成后，读连接以只读形态打开同一库文件。裸连接包成共享锁形态。
pub fn open_db_in(db_dir: &Path) -> Result<DbState> {
    let conn = open_connection_in(db_dir)?;
    let read_conn = open_connection_readonly_in(db_dir)?;
    Ok(DbState::from_pair(conn, read_conn))
}

/// 启动失败重置兜底：把当前库改名 `.bak` 保留后重新打开（新建空库）。
/// 只作用于引导解析出的生效目录，绝不删除任何文件。
pub fn reset_db_in(db_dir: &Path) -> Result<DbState> {
    let conn = reset_db_file(db_dir)?;
    let read_conn = open_connection_readonly_in(db_dir)?;
    Ok(DbState::from_pair(conn, read_conn))
}

/// 重置核心（连接形态，issue #601）：把当前库改名 `.bak` 保留后原位新建
/// 明文空库（建连 + 迁移 + 完整性检查，重置产物验收基准与引导层
/// `boot::encryption` 的 `open_new_plaintext_db` 同口径）。返回连接供启动失败
/// 恢复通道原位换入存活 [`DbState`]（占位连接 → 真实新库，无需重启）。
pub fn reset_db_file(db_dir: &Path) -> Result<Connection> {
    let db_path = db_dir.join(DB_FILE_NAME);
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
/// raw key 字符串。调用方必须先经 [`crate::boot::encryption::probe_file_kind`]
/// 确认文件确为密文库（[`crate::boot::encryption::DbFileKind::Encrypted`]）——对
/// 明文库设密钥后首条读语句会报「file is not a database」。口令错误的
/// 失败同样发生在首条读语句（口令错误 ≠ 库损坏，由探测区分）。
pub fn open_connection_with_passphrase<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    finish_open(Connection::open(path)?, Some(passphrase))
}

/// 只读打开数据库连接（读路径独立只读连接，issue #1280 / ADR-0117 决策 1/4）：
/// `SQLITE_OPEN_READ_ONLY` flags + busy_timeout（[`CONCURRENT_BUSY_TIMEOUT`]），
/// 经建连收尾单点的只读形态收口（外键 + 耗时 hook 同形）。不执行迁移——迁移是
/// 写操作，schema 由写连接的建连路径负责（成对建连时写连接先行）。
/// 明文库路径：不设密钥，行为与密钥基座引入前一致。
pub fn open_connection_readonly<P: AsRef<Path>>(path: P) -> Result<Connection> {
    finish_open_readonly(
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        None,
    )
}

/// 以主口令只读打开密文库连接（issue #1280 / ADR-0117 决策 2）：密钥注入与写
/// 连接同点同纪律（`PRAGMA key` 首条语句，trace 不落口令）；口令来源收口为
/// 换连点在场且已验证的口令，与写连接同源成对。
pub fn open_connection_readonly_with_passphrase<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    finish_open_readonly(
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        Some(passphrase),
    )
}

/// 在指定目录只读打开库（成对建连的读侧步骤，路径口径与 [`open_connection_in`]
/// 同源：目录 + 固定 [`DB_FILE_NAME`]）。
pub fn open_connection_readonly_in(db_dir: &Path) -> Result<Connection> {
    open_connection_readonly(db_dir.join(DB_FILE_NAME))
}

/// 建连收尾单点（只读形态）：密钥注入（如有）→ busy_timeout → 外键 → 耗时 hook。
fn finish_open_readonly(conn: Connection, passphrase: Option<&str>) -> Result<Connection> {
    if let Some(passphrase) = passphrase {
        // 与写连接同纪律：`PRAGMA key` 必须是连接上第一条语句，语句文本不进
        // trace（耗时 hook 尚未安装）。
        conn.pragma_update(None, "key", passphrase)?;
    }
    conn.busy_timeout(CONCURRENT_BUSY_TIMEOUT)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
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

/// 打开已完成 schema 迁移的内存库（统一测试数据库工厂的快速建库形态，
/// spec #1086 / issue #1514）。
///
/// 与 [`open_in_memory`] + [`init_db`] 两行序的建库结果等价，但**不逐次重放迁移链**：
/// 进程内首次调用以产物二进制（`serialize`）固化迁移后的 schema 与默认种子，其后
/// 每次调用经 `deserialize` 还原一份全新的独立内存库。还原走 SQLite 自身的建库
/// 路径（等价于重放结果），而重放 27 个迁移在本机实测约 32ms/次——统一工厂的
/// 逐用例建库成本因此从毫秒级降到微秒级（spec #1086 的「测试执行提速」方向，
/// 不改 CI 实际执行的测试范围）。
///
/// **独立性不变**：模板只是迁移产物的只读快照，每次还原得到的是互不干扰的独立
/// 内存库（内存库本就按连接隔离，非共享缓存），用例间零交叉污染，与逐次
/// [`init_db`] 的语义一致。
///
/// 连接级设置（外键、耗时 hook）按 `finish_open` 收尾单点重新施加——这些是
/// 连接态而非库内容，不进序列化产物。
///
/// **契约**：调用方必须是测试或测试器具；生产建连路径一律走
/// [`open_connection`] / [`open_db_in`] 等文件库入口。模板与还原是同一次构建的
/// 产物，SQLite 二进制格式跨版本兼容性因此不构成约束。
pub fn open_in_memory_initialized() -> Result<Connection> {
    use std::sync::OnceLock;

    // 模板产物按字节持有：`rusqlite::serialize::Data` 借连接（连接非 `Sync`），
    // 而模板要在进程内跨测试线程共享，故取一次拷贝（实测产物约 0.6MB，一次性）。
    // 缓存 `Result` 而非仅在成功时落值：本函数返回 `Result`，不得用 `expect` 把
    // 失败升级为 panic（ADR-0060 门禁辖生产文件），失败面按 `Err` 原样回传。
    static TEMPLATE: OnceLock<std::result::Result<Vec<u8>, String>> = OnceLock::new();
    fn derive_template() -> std::result::Result<Vec<u8>, String> {
        let mut conn = open_in_memory().map_err(|e| e.to_string())?;
        init_db(&mut conn).map_err(|e| e.to_string())?;
        let blob = conn.serialize("main").map_err(|e| e.to_string())?;
        Ok(blob.to_vec())
    }
    let template = TEMPLATE.get_or_init(derive_template);
    let Ok(template) = template.as_ref() else {
        let reason = match template {
            Err(reason) => reason.clone(),
            Ok(_) => String::new(),
        };
        return Err(AppError::coded(
            "db.template-init-failed",
            format!("内存模板库构建失败，无法还原测试库：{reason}"),
        ));
    };
    let mut conn = open_in_memory()?;
    conn.deserialize_read_exact("main", template.as_slice(), template.len(), false)?;
    Ok(conn)
}
