//! 建连 / 重置 / 完整性检查 / 内存库（自 `db/mod.rs` 按职责拆出，issue #1127，
//! 纯移动）。建连收尾单点 `finish_open`（本模块私有）：密钥注入 → 外键 →
//! 同步元数据库挂载 → 耗时 hook；
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

/// 同步元数据库文件名（ADR-0139 决策 1：四张同步表迁出主库落独立库文件，
/// 与主库同目录；决策 3：建连收尾单点成对挂载）。文件名应用固定，不可配置。
pub const SYNC_DB_FILE_NAME: &str = "sync.db";

/// 同步元数据库挂载失败的稳定错误码（ADR-0139 决策 3：挂载失败报码化错误、
/// 不静默降级；损坏 / 口令错误 / 形态错配 / 不可开共用此码，原因进 params）。
pub const SYNC_MOUNT_FAILED: &str = "db.sync-mount-failed";

/// 主库路径 → 同目录同步元数据库路径（转换配对 / 重置移位 / 恢复移位 / 挂载
/// 共用的单一推导点，ADR-0139 决策 1「与主库同目录」）。
pub fn sync_db_path(main_db_path: &Path) -> std::path::PathBuf {
    main_db_path.with_file_name(SYNC_DB_FILE_NAME)
}

/// 同步元数据四表闭集（ADR-0139 决策 1；表 DDL 权威在迁移链 V020/V021/V022，
/// 票 05 / V034 迁移后四表以同名同形迁入 attached 侧）。
const SYNC_TABLES: [&str; 4] = [
    "sync_device",
    "sync_ops",
    "sync_parked_ops",
    "sync_stream_positions",
];

/// 同步四表的双源回退判据（ADR-0139 拆库 expand 期判据，票 04 引入、票 06 删
/// 除）：attached `sync` 侧在位且四表闭集齐备 → 同步元数据归 attached；否则
///（连接未挂载、或票 05 迁移前四表仍在 main）回退 main。checkpoint 产出段
///（快照源库）与引导段（同步件消费与否）、备份产出段（成对件与 paired 标记）
/// 共用本单点——同一连接形态恒同判，票 05 迁移后统一命中 attached 分支。
pub fn sync_tables_live_attached(conn: &Connection) -> bool {
    let attached: i64 = conn
        .query_row(
            "SELECT count(*) FROM pragma_database_list WHERE name = 'sync'",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    if attached == 0 {
        return false;
    }
    // 别名在位后逐一核对四表闭集（别名与表名均为闭集字面量，无注入面）。
    SYNC_TABLES.iter().all(|table| {
        let present: i64 = conn
            .query_row(
                &format!("SELECT count(*) FROM sync.sqlite_master WHERE name = '{table}'"),
                [],
                |r| r.get(0),
            )
            .unwrap_or(0);
        present > 0
    })
}

/// 并发容让的 busy_timeout（读路径独立只读连接，issue #1280 / ADR-0117 决策 4；
/// 写连接同值显式收口，issue #1699）：读事务在写事务取 EXCLUSIVE 锁的提交瞬间
/// 窗口内、写事务在读事务持 SHARED 锁的窗口内，各在本超时内等待——取值与既有
/// 整库转换连接的容让超时同源收口（不在调用点散布），转换连接与两个建连收尾
/// （`finish_open` / `finish_open_readonly`）自本常量取值。
///
/// 说明：rusqlite 建连默认即 5000ms（与本常量巧合同值），两侧收尾的显式设置
/// 不改行为，作用是把契约从库默认收回源码单点、不随依赖版本漂移。
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
///
/// 同步元数据库一并移位（ADR-0139 决策 3 配对纪律，issue #1869）：把用户
/// 送进失败恢复屏的原因可能正是 sync.db 损坏 / 形态错配（挂载码化失败），
/// 遗留文件若留守，重置后的新世界开启仍挂在同一处——逃生门被堵死、无法
/// 自愈。故在动主库之前先移（失败即中止，现场可重试），按同款重置命名
/// 语义保留 `sync.db.bak` 副本（永不删除）；新世界的 sync.db 由挂载接线
/// 按明文形态补建。
pub fn reset_db_file(db_dir: &Path) -> Result<Connection> {
    let db_path = db_dir.join(DB_FILE_NAME);
    let sync_path = sync_db_path(&db_path);
    if sync_path.exists() {
        std::fs::rename(&sync_path, sync_path.with_extension("db.bak"))?;
    }
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
    let path = path.as_ref();
    finish_open(Connection::open(path)?, None, Some(path))
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
    let path = path.as_ref();
    finish_open(Connection::open(path)?, Some(passphrase), Some(path))
}

/// 生命周期连接打开（搬迁 / 恢复 / 转换验证等 ADR-0117 决策 4 生命周期操作）
/// 不挂载同步元数据库的显式形态：操作对象是库文件本体（VACUUM INTO 复制 /
/// 替换 / 校验），attached 侧与其无涉；且源库可能损坏（搬迁回退场景），
/// 挂载验证读会因主库 schema 加载失败把主库损坏误报为挂载失败，缺失时还会
/// 对源目录旁挂出 sync.db——都背离挂载接线「主库判别」的意图
///（ADR-0139 决策 3：生命周期连接不属建连收尾管辖，临时文件名判别之外的
/// 显式出口）。收尾其余步骤（密钥注入 / busy_timeout / 外键 / 耗时 hook）
/// 与产品建缝同形，密钥注入仍收口单点（ADR-0075）。
pub fn open_connection_unmounted<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let path = path.as_ref();
    finish_open(Connection::open(path)?, None, None)
}

/// 同 [`open_connection_unmounted`]，密文形态凭主口令（密钥注入同纪律：
/// `PRAGMA key` 连接首条语句，trace 不落口令）。
pub fn open_connection_with_passphrase_unmounted<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    let path = path.as_ref();
    finish_open(Connection::open(path)?, Some(passphrase), None)
}

/// 只读打开数据库连接（读路径独立只读连接，issue #1280 / ADR-0117 决策 1/4）：
/// `SQLITE_OPEN_READ_ONLY` flags + busy_timeout（[`CONCURRENT_BUSY_TIMEOUT`]），
/// 经建连收尾单点的只读形态收口（外键 + 耗时 hook 同形）。不执行迁移——迁移是
/// 写操作，schema 由写连接的建连路径负责（成对建连时写连接先行）。
/// 明文库路径：不设密钥，行为与密钥基座引入前一致。
pub fn open_connection_readonly<P: AsRef<Path>>(path: P) -> Result<Connection> {
    let path = path.as_ref();
    finish_open_readonly(
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        None,
        Some(path),
    )
}

/// 以主口令只读打开密文库连接（issue #1280 / ADR-0117 决策 2）：密钥注入与写
/// 连接同点同纪律（`PRAGMA key` 首条语句，trace 不落口令）；口令来源收口为
/// 换连点在场且已验证的口令，与写连接同源成对。
pub fn open_connection_readonly_with_passphrase<P: AsRef<Path>>(
    path: P,
    passphrase: &str,
) -> Result<Connection> {
    let path = path.as_ref();
    finish_open_readonly(
        Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?,
        Some(passphrase),
        Some(path),
    )
}

/// 在指定目录只读打开库（成对建连的读侧步骤，路径口径与 [`open_connection_in`]
/// 同源：目录 + 固定 [`DB_FILE_NAME`]）。
pub fn open_connection_readonly_in(db_dir: &Path) -> Result<Connection> {
    open_connection_readonly(db_dir.join(DB_FILE_NAME))
}

/// SQL 字符串字面量转义（单引号加倍）：仅用于 ATTACH 的路径与主口令注入
/// （ATTACH KEY 系语法不支持绑定参数）。调用点必须在耗时 hook 安装前，
/// 语句文本（含主口令）不进 trace（ADR-0075）。
pub(crate) fn sql_string_literal(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 同步元数据库挂载（ADR-0139 决策 3/4，issue #1869）：把与主库同目录的
/// [`SYNC_DB_FILE_NAME`] ATTACH 为 `sync`，挂载点即建连收尾（密钥注入与外键
/// 之后、耗时 hook 之前——ATTACH 文本含主口令，不进 trace）。
///
/// - **成对挂载**：写连接与只读连接（ADR-0117 成对连接）各自挂载；只读侧
///   挂在只读连接上，对 attached 侧写被拒（只读形态）。
/// - **同 KEY 注入**：密文形态以主口令同源派生密钥显式挂载——SQLCipher 无
///   KEY 挂载必然失败，继承推断只是隐式行为，显式注入是决策 3 的裁决。
/// - **迁移事务外**：ATTACH 不能在事务内执行，本函数在迁移 runner
///   （[`init_db`] → to_latest）之前执行；V036 起迁移语句可直接引用
///   attached 侧（决策 4「runner 前置挂载」机制，票 05 消费）。
///
/// **主库判别**：只有打开的是主库文件名（[`DB_FILE_NAME`]）才挂载——转换 /
/// 搬迁 / 恢复 / 备份的临时副本连接（生命周期操作，ADR-0117 决策 4 不属建连
/// 收尾管辖）不挂载，避免对临时目录旁挂出 sync.db、对同目录真实 sync.db
/// 误挂或以错配口令触碰。
///
/// **expand 期文件策略**（本票不搬数据，票 05 / V036 迁移前四张同步表仍在
/// 主库，unqualified 表名解析优先 main，生产行为零变化）：
/// - 写侧文件缺失 → 自动创建空库（双库布局自此成形；V036 落地后「缺失」
///   须改判码化错误——彼时 sync.db 缺失即设备身份丢失，由票 05 翻转此策略）；
/// - 只读侧文件缺失 → 跳过挂载：expand 之前的世界没有 sync.db，跨账本只读
///   聚合等只读开启不得因它失败；活动账本写连接已在同目录补建，成对不变。
///
/// 挂载失败（损坏、口令错、形态错配、不可开）报码化错误
/// [`SYNC_MOUNT_FAILED`]，不静默降级（ADR-0139 决策 3；spec 用户故事 17）。
fn attach_sync_db(
    conn: &Connection,
    main_db_path: &Path,
    passphrase: Option<&str>,
    readonly_side: bool,
) -> Result<()> {
    if main_db_path.file_name() != Some(std::ffi::OsStr::new(DB_FILE_NAME)) {
        return Ok(());
    }
    let sync_path = sync_db_path(main_db_path);
    if readonly_side && !sync_path.exists() {
        return Ok(());
    }
    let key_clause = passphrase
        .map(|pass| format!(" KEY {}", sql_string_literal(pass)))
        .unwrap_or_default();
    let attach_sql = format!(
        "ATTACH DATABASE {} AS sync{key_clause}",
        sql_string_literal(&sync_path.to_string_lossy()),
    );
    if let Err(e) = conn.execute_batch(&attach_sql) {
        return Err(mount_failed_error(&e, passphrase));
    }
    // 挂载点即验证点：SQLite 惰性读页，损坏 / 口令错误 / 形态错配在 ATTACH
    // 本身不报错，首个读语句才暴露——强制读一次 attached 侧文件头，失败
    // 此刻码化上抛，不带病运行。
    if let Err(e) =
        conn.query_row::<i64, _, _>("SELECT count(*) FROM sync.sqlite_master", [], |r| r.get(0))
    {
        return Err(mount_failed_error(&e, passphrase));
    }
    tracing::debug!(sync_db = %sync_path.display(), "同步元数据库已挂载");
    Ok(())
}

/// 口令错误/损坏的合并口径码化错误（ADR-0075 决策 5 修订 / issue #603）：
/// SQLCipher 下错误口令与损坏同为 not-a-database、运行期不可靠区分，统一以
/// 「口令错误或文件损坏」上报（可就地重试，不误报损坏）。单一构造点住 db
/// （建连收尾的带 KEY 挂载与引导层解锁/转换共用同一码与文案），引导层
/// [`crate::boot::encryption::passphrase_incorrect_error`] 经此委托。
pub(crate) fn passphrase_incorrect_error() -> AppError {
    AppError::coded(
        "encryption.passphrase-incorrect",
        "口令错误或文件损坏，请重试",
    )
}

/// not-a-database 判读：带 KEY 挂载下口令错误与 sync.db 损坏共用此形态；
/// 引导层同语义谓词（`boot::encryption::is_not_a_database`，跨 crate `pub`）
/// 经此委托，单一实现点。
pub(crate) fn is_not_a_database_error(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(f, _)
            if f.extended_code == rusqlite::ffi::SQLITE_NOTADB
    )
}

/// 挂载失败的码化错误构造（单一构造点）：`rusqlite::Error` 的 Display 可能
/// 内嵌语句文本——ATTACH 语句含主口令（ADR-0075：错误与日志不落主口令），
/// 只取引擎 errmsg（无密钥材料）进 message 与 params。带 KEY 挂载的
/// not-a-database 走合并口径（见 [`passphrase_incorrect_error`]）；明文库
/// 挂载（无 KEY）的 not-a-database 是确定的形态错配/损坏，报挂载失败码。
fn mount_failed_error(e: &rusqlite::Error, passphrase: Option<&str>) -> AppError {
    if passphrase.is_some() && is_not_a_database_error(e) {
        return passphrase_incorrect_error();
    }
    let reason = match e {
        rusqlite::Error::SqliteFailure(f, _) => f.to_string(),
        other => other.to_string(),
    };
    AppError::codedp(
        SYNC_MOUNT_FAILED,
        format!("同步元数据库挂载失败: {reason}"),
        &[reason.as_str()],
    )
}

/// 建连收尾单点（只读形态）：密钥注入（如有）→ busy_timeout → 外键 →
/// 同步元数据库挂载 → 耗时 hook。
fn finish_open_readonly(
    conn: Connection,
    passphrase: Option<&str>,
    main_db_path: Option<&Path>,
) -> Result<Connection> {
    if let Some(passphrase) = passphrase {
        // 与写连接同纪律：`PRAGMA key` 必须是连接上第一条语句，语句文本不进
        // trace（耗时 hook 尚未安装）。
        conn.pragma_update(None, "key", passphrase)?;
    }
    conn.busy_timeout(CONCURRENT_BUSY_TIMEOUT)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    if let Some(path) = main_db_path {
        attach_sync_db(&conn, path, passphrase, true)?;
    }
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
}

/// 建连收尾单点：密钥注入（如有）→ busy_timeout → 外键 → 同步元数据库挂载 →
/// 耗时 hook。
fn finish_open(
    conn: Connection,
    passphrase: Option<&str>,
    main_db_path: Option<&Path>,
) -> Result<Connection> {
    if let Some(passphrase) = passphrase {
        // `PRAGMA key` 必须是连接上第一条语句；PRAGMA 不支持绑定参数，
        // 经 `pragma_update` 以转义后的 SQL 字面量注入。耗时 hook 尚未安装，
        // 语句文本（含主口令）不会进入 trace 输出（ADR-0075：日志与 trace
        // 不落主口令）——调整顺序即破坏该纪律。
        conn.pragma_update(None, "key", passphrase)?;
    }
    // 并发容让（issue #1699；与只读侧同值同源，ADR-0117 决策 4 的收口纪律）：
    // 多语句读闭包收进读事务后，rollback journal 下读事务持 SHARED 锁横跨语句，
    // 本连接提交须等其让出——无容让即 SQLITE_BUSY 即时失败，「读一致性」会换成
    // 用户可见写失败。rusqlite 建连默认恰为同值，此处显式设置不改行为，只把契约
    // 从库默认收回源码单点（见 [`CONCURRENT_BUSY_TIMEOUT`] 说明）。
    conn.busy_timeout(CONCURRENT_BUSY_TIMEOUT)?;
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    if let Some(path) = main_db_path {
        attach_sync_db(&conn, path, passphrase, false)?;
    }
    perf_trace::install_perf_trace(&conn, perf_trace::DEFAULT_SLOW_QUERY_THRESHOLD);
    Ok(conn)
}

/// 打开内存数据库连接并启用外键约束（用于测试和 BDD 集成测试）。
/// 与文件库路径共用建连收尾单点（外键、耗时 hook）。
pub fn open_in_memory() -> Result<Connection> {
    finish_open(Connection::open_in_memory()?, None, None)
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
