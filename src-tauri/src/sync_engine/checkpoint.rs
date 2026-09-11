//! Checkpoint 检查点快照（issue #857 / ADR-0091 决策 9）：全量快照 + 各设备 op
//! 流已应用位点的产出、新端引导与 OpLog 截断机制。
//!
//! 语义约束只有一条——任何端永远能从 Checkpoint 加其后 op 重建出一致状态：
//! - **产出**（[`create_checkpoint`]）：`VACUUM INTO` 整库一致性快照（与备份
//!   同构，ADR-0016 的文件级快照纪律；继承源库加密形态，文件即真相），配上
//!   本端位点表（[`super::positions`]）。位点与快照须同刻成对——调用方持单
//!   连接互斥锁一次性完成（壳层同步轮次形态），进程内无并发写插入。
//! - **引导**（[`bootstrap_from_checkpoint`]）：快照经 `ATTACH`（密文快照凭
//!   主口令，与密文备份恢复同形态）挂载后整库换入——SQL 级重建（建表 → 拷行
//!   → 索引/触发器/视图），任意加密形态组合（明文/密文库 × 明文/密文快照）
//!   均成立（bundled SQLCipher 对加密库禁用 backup API，SQL 级是官方正解）。
//!   schema 偏斜双向处置：快照更新→拒绝；较旧→对齐后迁移升级。随后换入本机
//!   设备身份、位点表以 Checkpoint 为准重建。快照携带的日志与挂起队列随行
//!   采纳：日志给出位点之前的去重身份（对端全量重投不再重放），挂起行经
//!   重投递幂等覆盖自愈。
//! - **截断**（[`truncate_stream_before`]）：机制原语，三硬约束——只有来源
//!   设备有权截断自己的流；只有位点之前（时钟 ≤ 水位）才可删；挂起 op 不在
//!   日志、位点不越过它，天然不被截断。**v1 默认不启用**（永不截断，取舍见
//!   issue #857：op 无时间保留期限，只有位点安全义务），水位来源（各端上报
//!   位点的最小值 + 已并入对端可达 Checkpoint）是通道层（#859/#862）义务。
//!
//! 引导守卫：目标必须尚未参与同步（无日志、无位点、无挂起）——引导是整库
//! 换入，覆盖既有同步状态等于丢账；残留用户业务数据的库同样拒绝（加入即
//! 新库）。两半守卫收在 [`bootstrap_preflight`] / [`bootstrap_from_channel`]
//! 域内单点（issue #864），壳层退回解包与投影。

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::tx_scope::ensure_transaction;
use crate::db::{self, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::fs_util;

use super::device;
use super::ops;
use super::positions::{self, StreamPosition};
use super::trigger::SyncChannel;

/// 引导期间快照挂载的 ATTACH 别名。
const SNAP_ALIAS: &str = "sync_snap";

/// Checkpoint（检查点快照）：全量数据快照 + 各设备 op 流已应用位点。
///
/// `snapshot` 是整库 SQLite 文件字节（`VACUUM INTO` 产物，继承源库加密形态，
/// 文件即真相）；位点与快照同刻成对，是「快照 + 其后 op = 一致状态」判据的
/// 两个组成部分。通道上的打包与分代命名（SyncEnvelope / manifest 指针）归
/// Transport（#859）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    /// 各设备 op 流已应用位点（快照时刻各流头）。
    pub positions: Vec<StreamPosition>,
    /// 整库全量快照（SQLite 文件字节）。
    pub snapshot: Vec<u8>,
}

/// 任意时刻产出 Checkpoint：整库一致性快照 + 本端位点表。
///
/// 必须在单连接互斥锁内调用（位点与快照同刻成对的保证）；不得处于写事务中
/// （`VACUUM INTO` 无法在事务内执行）。
pub fn create_checkpoint(conn: &Connection) -> Result<Checkpoint> {
    let snapshot_path = temp_snapshot_path("cp");
    let result = (|| -> Result<Checkpoint> {
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![snapshot_path.to_string_lossy()],
        )?;
        Ok(Checkpoint {
            positions: positions::list(conn)?,
            snapshot: std::fs::read(&snapshot_path)?,
        })
    })();
    fs_util::cleanup(&snapshot_path);
    result
}

/// 新端引导：整库换入 Checkpoint 快照并就位位点。
///
/// 目标必须是尚未参与同步的库（守卫拒绝，见模块文档）；`passphrase` 用于
/// 密文快照（继承加密源库的产物，与密文备份恢复同形态：缺口令报
/// `sync-engine.checkpoint-passphrase-required` 可重试，错误口令报
/// `encryption.passphrase-incorrect`），明文快照不消费口令。引导后本机持有
/// 自己的设备身份（既有则保留、无则新生成），位点表以 Checkpoint 为准重建。
///
/// 必须在单连接互斥锁内调用、不得处于写事务中（`ATTACH`/`PRAGMA` 时序与
/// `foreign_keys` 开关要求）。
pub fn bootstrap_from_checkpoint(
    conn: &mut Connection,
    checkpoint: &Checkpoint,
    passphrase: Option<&str>,
) -> Result<()> {
    // 守卫：目标尚未参与同步（三张同步元数据表全空；权威复验，壳层拉取前
    // 已有同序前置）。
    if let Some(table) = first_sync_table_not_empty(conn)? {
        return Err(AppError::codedp(
            "sync-engine.bootstrap-not-fresh",
            format!("本机已参与同步（{table} 非空），拒绝整库引导以免覆盖既有同步状态"),
            &[table],
        ));
    }
    // 引导前捕获/生成本机设备身份（首用生成单点；重建后换回，快照携带的
    // 来源方身份不残留）。
    let own_device = device::device_id(conn)?;
    let own_clock: i64 = conn.query_row(
        "SELECT logical_clock FROM sync_device WHERE id = ?1",
        [&own_device],
        |r| r.get(0),
    )?;

    // 快照落临时文件 → 挂载（密文凭口令）→ 整库重建换入 → 卸载。
    let snapshot_path = temp_snapshot_path("restore");
    std::fs::write(&snapshot_path, &checkpoint.snapshot)?;
    let result = (|| -> Result<()> {
        attach_snapshot(conn, &snapshot_path, passphrase)?;
        let rebuild = (|| -> Result<()> {
            let snapshot_version = read_snapshot_version(conn)?;
            let local_version = db::schema_version(conn)?;
            if snapshot_version > local_version {
                return Err(AppError::codedp(
                    "sync-engine.checkpoint-schema-newer",
                    format!(
                        "检查点快照来自更高版本的应用（快照 schema v{snapshot_version} > 当前 v{local_version}），请升级应用后再引导"
                    ),
                    &[
                        snapshot_version.to_string().as_str(),
                        local_version.to_string().as_str(),
                    ],
                ));
            }
            rebuild_main_from_snapshot(conn, snapshot_version)
        })();
        // 挂载库恒卸载（重建失败亦然），随后以重建结果为准。
        let detach = conn.execute(&format!("DETACH DATABASE {SNAP_ALIAS}"), []);
        rebuild?;
        detach?;
        Ok(())
    })();
    fs_util::cleanup(&snapshot_path);
    result?;

    // 快照 schema 较旧时迁移升级（同版本为无操作）；换入本机身份、位点重建
    // 为 Checkpoint 携带值（与快照内置位点同刻，覆写仅为以信封对为权威）。
    db::init_db(&mut *conn)?;
    db::check_integrity(conn)?;
    ensure_transaction(conn, || {
        conn.execute("DELETE FROM sync_device", [])?;
        conn.execute(
            "INSERT INTO sync_device (id, logical_clock, created_at, updated_at) VALUES (?1, ?2, ?3, ?3)",
            rusqlite::params![own_device, own_clock, now_iso()],
        )?;
        positions::replace_all(conn, &checkpoint.positions)?;
        Ok(())
    })?;
    tracing::info!(
        device = %own_device,
        streams = checkpoint.positions.len(),
        snapshot_bytes = checkpoint.snapshot.len(),
        "已从检查点快照完成新端引导"
    );
    Ok(())
}

/// 挂载快照库：密文快照凭主口令 `ATTACH ... KEY`（与密文备份恢复同形态），
/// 明文快照免口令。SQLCipher 延迟到首条读语句才校验口令——由快照版本读取
/// 归一错误形态（[`read_snapshot_version`]）。
fn attach_snapshot(conn: &Connection, path: &Path, passphrase: Option<&str>) -> Result<()> {
    match db::encryption::probe_file_kind(path)? {
        db::encryption::DbFileKind::Encrypted => {
            let passphrase = passphrase.ok_or_else(|| {
                AppError::coded(
                    "sync-engine.checkpoint-passphrase-required",
                    "该检查点快照为密文，需要主口令才能引导",
                )
            })?;
            attach_sql(
                conn,
                "ATTACH DATABASE ?1 AS sync_snap KEY ?2",
                passphrase,
                path,
            )
        }
        db::encryption::DbFileKind::Plaintext | db::encryption::DbFileKind::Empty => {
            attach_sql(conn, "ATTACH DATABASE ?1 AS sync_snap KEY ?2", "", path)
        }
    }
}

/// ATTACH 语句执行（本仓统一 SQLCipher 构建基座：明文库以 `KEY ''` 挂载）。
/// 挂载后立即以类型化读语句校验口令——`PRAGMA key`/`ATTACH KEY` 本身不校验，
/// 首条读语句才解密页数据；错误口令归一为可重试的码化错误（与密文备份恢复
/// 同款，不裸上抛）。
fn attach_sql(conn: &Connection, sql: &str, key: &str, path: &Path) -> Result<()> {
    conn.execute(sql, rusqlite::params![path.to_string_lossy(), key])
        .map_err(|e| {
            // 错误口令在本 SQLCipher 构建上于 ATTACH 即报 NOTADB，归一为可重试
            // 码化错误；其余错误原样上抛。
            if db::encryption::is_not_a_database(&e) {
                db::encryption::passphrase_incorrect_error()
            } else {
                e.into()
            }
        })?;
    if let Err(e) = conn.query_row("SELECT count(*) FROM sync_snap.sqlite_master", [], |r| {
        r.get::<_, i64>(0)
    }) {
        if db::encryption::is_not_a_database(&e) {
            return Err(db::encryption::passphrase_incorrect_error());
        }
        return Err(e.into());
    }
    Ok(())
}

/// 整库重建：以挂载的快照 schema 与数据换入 main。
///
/// 步骤（单事务；`foreign_keys` 先关后开，外键校验以 `foreign_key_check`
/// 兜底在提交前完成）：清空 main 全部用户对象 → 按快照建表 → 逐表拷行 →
/// 索引/视图/触发器复位 → `user_version` 对齐快照 → 外键校验。bundled
/// SQLCipher 对加密库禁用 backup 页拷贝，SQL 级重建对任意加密形态组合成立。
fn rebuild_main_from_snapshot(conn: &Connection, snapshot_version: i64) -> Result<()> {
    conn.execute("PRAGMA foreign_keys = OFF", [])?;
    let rebuilt = ensure_transaction(conn, || {
        // 清空 main（快照外对象不留残迹）：先从属对象后表，删表自动带走
        // 其索引/触发器，IF EXISTS 兜底重复。
        for drop_sql in drop_statements(conn)? {
            conn.execute(&drop_sql, [])?;
        }
        // 建表（快照 schema 原样）→ 拷行 → 从属对象复位（与快照同名同序）。
        let objects = snapshot_objects(conn)?;
        for sql in objects.tables() {
            conn.execute(sql, [])?;
        }
        for (table, bare) in objects.table_names.iter().zip(&objects.bare_names) {
            // 生成列（GENERATED ALWAYS AS，hidden≠0）不可作为 INSERT 目标，
            // 显式列清单排除之。
            let columns = insertable_columns(conn, bare)?;
            conn.execute(
                &format!("INSERT INTO main.{table} ({columns}) SELECT {columns} FROM {SNAP_ALIAS}.{table}"),
                [],
            )?;
        }
        for sql in objects.subordinates() {
            conn.execute(sql, [])?;
        }
        // user_version 对齐快照（迁移起点），随后 init_db 前向升级。
        conn.execute(&format!("PRAGMA user_version = {snapshot_version}"), [])?;
        // 外键校验兜底（快照自身不一致在此确定性失败，事务整体回滚）。
        let violations: i64 =
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if violations > 0 {
            return Err(AppError::codedp(
                "sync-engine.checkpoint-fk-violation",
                format!("检查点快照外键校验失败（{violations} 处），快照不可用"),
                &[&violations.to_string()],
            ));
        }
        Ok(())
    });
    conn.execute("PRAGMA foreign_keys = ON", [])?;
    rebuilt
}

/// main 库全部用户对象的 DROP 语句（触发器/视图/索引先行，表殿后）。
fn drop_statements(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT type, name FROM main.sqlite_master \
         WHERE name NOT LIKE 'sqlite_%' \
         ORDER BY CASE type WHEN 'table' THEN 1 ELSE 0 END, rowid DESC",
    )?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut statements = Vec::new();
    for row in rows {
        let (kind, name) = row?;
        let keyword = match kind.as_str() {
            "table" => "TABLE",
            "view" => "VIEW",
            "index" => "INDEX",
            "trigger" => "TRIGGER",
            _ => continue,
        };
        statements.push(format!("DROP {keyword} IF EXISTS {}", quote_ident(&name)));
    }
    Ok(statements)
}

/// 快照库对象清单：建表语句 + 表名（引用形/裸名成对）+ 从属对象（索引/视图/
/// 触发器）建语句。
struct SnapshotObjects {
    tables: Vec<String>,
    table_names: Vec<String>,
    bare_names: Vec<String>,
    subordinates: Vec<String>,
}

impl SnapshotObjects {
    fn tables(&self) -> &[String] {
        &self.tables
    }
    fn subordinates(&self) -> &[String] {
        &self.subordinates
    }
}

/// 读取快照库的 schema 对象（虚拟表/内部对象排除；自动索引 sql 为空跳过）。
fn snapshot_objects(conn: &Connection) -> Result<SnapshotObjects> {
    let mut stmt = conn.prepare(&format!(
        "SELECT type, name, sql FROM {SNAP_ALIAS}.sqlite_master \
         WHERE name NOT LIKE 'sqlite_%' AND sql IS NOT NULL"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;
    let mut objects = SnapshotObjects {
        tables: Vec::new(),
        table_names: Vec::new(),
        bare_names: Vec::new(),
        subordinates: Vec::new(),
    };
    for row in rows {
        let (kind, name, sql) = row?;
        match kind.as_str() {
            "table" => {
                objects.tables.push(sql);
                objects.table_names.push(quote_ident(&name));
                objects.bare_names.push(name);
            }
            "view" | "index" | "trigger" => objects.subordinates.push(sql),
            _ => {}
        }
    }
    Ok(objects)
}

/// 表的可写入列清单（逗号连接、各自 quote 引用）：生成列（`GENERATED ALWAYS
/// AS`，`pragma_table_xinfo.hidden ≠ 0`）不可作为 INSERT 目标，排除之；裸名经
/// 单引号转义进表值函数参数。
fn insertable_columns(conn: &Connection, bare_table: &str) -> Result<String> {
    let literal = bare_table.replace('\'', "''");
    let mut stmt = conn.prepare(&format!(
        "SELECT name FROM pragma_table_xinfo('{literal}') WHERE hidden = 0"
    ))?;
    let names = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(names
        .iter()
        .map(|n| quote_ident(n))
        .collect::<Vec<_>>()
        .join(", "))
}

/// 标识符双引号引用（内嵌引号翻倍），防保留字与特殊字符。
fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// 挂载库的 schema 版本（`PRAGMA <别名>.user_version`；别名是本模块常量，
/// 无注入面。SQLCipher 延迟到首条读语句才校验口令——密文快照的口令错误在
/// 此首读归一为可重试码化错误）。
fn read_snapshot_version(conn: &Connection) -> Result<i64> {
    match conn.query_row(&format!("PRAGMA {SNAP_ALIAS}.user_version"), [], |r| {
        r.get::<_, i64>(0)
    }) {
        Ok(v) => Ok(v),
        Err(e) => {
            if db::encryption::is_not_a_database(&e) {
                return Err(db::encryption::passphrase_incorrect_error());
            }
            Err(e.into())
        }
    }
}

/// 截断单流日志前缀（OpLog 截断机制原语，v1 默认不启用）。
///
/// 三硬约束的调用界：只有来源设备有权截断自己的流（本端守卫，违者报
/// `sync-engine.truncate-not-owner`）；`through_position` 是删除上界（时钟 ≤
/// 水位的行删除），调用方须传入各端上报位点的最小值，且仅当水位之前的 op 已
/// 并入对端可达 Checkpoint 时才可调用（通道层义务，#859/#862）。挂起队列
/// op 不在日志、位点不越过它，不受截断影响。
pub fn truncate_stream_before(
    conn: &Connection,
    device_id: &str,
    through_position: i64,
) -> Result<usize> {
    if device_id != device::device_id(conn)? {
        return Err(AppError::coded(
            "sync-engine.truncate-not-owner",
            "只有来源设备有权截断自己的操作日志流",
        ));
    }
    let deleted = ops::delete_stream_before(conn, device_id, through_position)?;
    if deleted > 0 {
        tracing::info!(
            device = %device_id,
            through = through_position,
            deleted,
            "已截断本机日志流前缀"
        );
    }
    Ok(deleted)
}

/// 同步元数据表非空检查（引导守卫的第一半）：返回命中的表名。
/// [`bootstrap_from_checkpoint`] 的权威守卫与 [`bootstrap_preflight`] 共享。
fn first_sync_table_not_empty(conn: &Connection) -> Result<Option<&'static str>> {
    for (table, empty) in [
        ("sync_ops", ops::is_empty(conn)?),
        ("sync_stream_positions", positions::is_empty(conn)?),
        ("sync_parked_ops", super::parked::is_empty(conn)?),
    ] {
        if !empty {
            return Ok(Some(table));
        }
    }
    Ok(None)
}

/// 引导前置守卫（fail fast，[`bootstrap_from_channel`] 在拉取快照前调用；
/// [`bootstrap_from_checkpoint`] 内部对同步三表的复验仍是权威）：目标必须是
/// 「全新空库」——未参与同步且无用户业务数据。已参与同步报
/// `sync-engine.bootstrap-not-fresh`（优先于业务数据判定：已加入同步的端重试
/// 引导，正确提示是「已参与同步」而非「本机有数据」）；残留业务数据报
/// `sync-channel.bootstrap-library-not-empty`。
pub(crate) fn bootstrap_preflight(conn: &Connection) -> Result<()> {
    if let Some(table) = first_sync_table_not_empty(conn)? {
        return Err(AppError::codedp(
            "sync-engine.bootstrap-not-fresh",
            format!("本机已参与同步（{table} 非空），拒绝整库引导以免覆盖既有同步状态"),
            &[table],
        ));
    }
    if library_has_user_data(conn)? {
        return Err(AppError::coded(
            "sync-channel.bootstrap-library-not-empty",
            "本机已有账本数据，不能从通道引导（引导将以快照整库替换本机账本）；请改用备份恢复合并数据，或在本机是全新空账本时重试",
        ));
    }
    Ok(())
}

/// 探针表闭集（引导前置「加入即新库」判据）：各业务域的用户事实行。
const PROBE_TABLES: &[&str] = &[
    "transactions",
    "accounts",
    "categories",
    "merchants",
    "insurers",
    "instruments",
    "scheduled_transactions",
    "budgets",
    "policies",
    "items",
    "physical_assets",
    "exchange_rates",
    "fx_rate_history",
    "market_prices",
    "price_history",
    "security_lots",
];

/// 种子行的来源设备标识（迁移种子数据的既定约定，V004 起种子行
/// `device_id = 'seed'`；非此值即用户事实）。
const SEED_DEVICE_ID: &str = "seed";

/// 本机库是否已有用户业务数据（「加入即新库」守卫的判据，[`bootstrap_preflight`] 消费）。
///
/// 引导是整库换入——带存量业务数据的库被引导等于丢账（快照整库覆盖）。
/// 领域守卫（[`bootstrap_from_checkpoint`]）只挡「已参与同步」；「业务数据
/// 是否残留」是同步边界的知识（业务域清单与 Backup/Restore 迁移边界对齐），
/// 归本域单点、[`bootstrap_preflight`] 消费（issue #864）。探针为闭集清单：各业务域
/// 的用户事实行（种子行以 `device_id = 'seed'` 排除；派生数据——余额/净值
/// 缓存、期次行、持仓批次结转——不属用户事实或被主表探针覆盖，不在清单；
/// 币种字典无来源设备列且全为种子闭集，自建币种残留属可接受边界）。
pub(crate) fn library_has_user_data(conn: &Connection) -> Result<bool> {
    for table in PROBE_TABLES {
        // 表名是本模块闭集常量（非用户输入），无注入面。
        let has_row: bool = conn.query_row(
            &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE device_id <> {SEED_DEVICE_ID:?})"),
            [],
            |r| r.get(0),
        )?;
        if has_row {
            return Ok(true);
        }
    }
    Ok(false)
}

/// 引导结果（壳层 wire 面由此投影）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapOutcome {
    /// 采纳的检查点代数。
    pub generation: i64,
    /// 快照密文字节数。
    pub size: u64,
    /// 引导后本库已从明文转换为本机密文库（重启后需凭主口令解锁）。
    pub reencrypted: bool,
}

/// 新端从通道检查点引导的编排单点（壳层「从通道引导本端」向导的领域接缝，
/// issue #864）：拉取 → 信封形态对齐 → 整库换入 → 簿记清理 → （密文快照 ×
/// 明文本机时）整库转密文。必须在单连接互斥锁内调用（快照拉取/换入与轮次
/// 同一互斥约束）。
///
/// 序列：
/// 1. 前置守卫（fail fast，不浪费拉取）：[`bootstrap_preflight`]——未参与
///    同步且无用户业务数据；
/// 2. 拉取检查点：信封自描述，密文快照凭口令开封（缺口令报
///    `sync-engine.checkpoint-passphrase-required` 可重试，错误口令报
///    `encryption.passphrase-incorrect`）；
/// 3. 信封形态对齐守卫：同一通道的段必须同形态可开（明文端开不了密文段、
///    密文端封的段明文对端开不了）——引导端必须对齐快照形态：密文快照 ×
///    密文本机时输入口令必须就是本机主口令（后续轮次以本机口令封段，两把
///    钥匙会让对端解不开），不一致报 `sync-channel.bootstrap-passphrase-mismatch`；
///    明文快照 × 密文本机拒绝（`sync-channel.bootstrap-form-mismatch`）；
/// 4. 整库换入（[`bootstrap_from_checkpoint`]，同步三表权威复验）；
/// 5. 清「上次成功同步时刻」（本机簿记事实，快照携带的是来源端取值；在
///    文件级转换前执行——转换的原子替换会让本连接指向被换下的旧文件，
///    此后一切写路径不得再经它）；
/// 6. 密文快照 × 明文本机：复用备份域机制整库转密文（文件级原子替换，
///    重启后新连接凭口令打开）。转换失败的可恢复路径：本机已是引导后的
///    完整数据，经既有「开启加密」以同一主口令转换即重新对齐通道形态。
pub fn bootstrap_from_channel(
    conn: &mut Connection,
    db_path: &Path,
    channel: &SyncChannel,
    passphrase: Option<&str>,
) -> Result<BootstrapOutcome> {
    // 1. 前置守卫（fail fast）。
    bootstrap_preflight(conn)?;
    // 2. 拉取检查点（持锁；快照体整库下载）。
    let fetched = channel.fetch_checkpoint(passphrase)?;
    // 3. 信封形态对齐守卫（替换本机数据前判定，失败零副作用）。
    let local_encrypted =
        db::encryption::probe_file_kind(db_path)? == db::encryption::DbFileKind::Encrypted;
    if fetched.sealed && local_encrypted {
        // fetch 成功 ⇒ 口令已验证可开快照（缺口令/错口令在域内归一报错）。
        let passphrase = passphrase.unwrap_or_default();
        if db::encryption::verify_source_passphrase(db_path, passphrase).is_err() {
            return Err(AppError::coded(
                "sync-channel.bootstrap-passphrase-mismatch",
                "输入的主口令与本机主口令不一致：密文库引导须以本机主口令进行（通道上的快照以同一主口令封包）",
            ));
        }
    } else if !fetched.sealed && local_encrypted {
        return Err(AppError::coded(
            "sync-channel.bootstrap-form-mismatch",
            "通道上的检查点为明文，本机为密文库：请先在设置中关闭加密，或让来源端开启加密后重新发布检查点",
        ));
    }
    let reencrypted = fetched.sealed && !local_encrypted;
    // 4. 整库换入（同步三表权威复验）。
    bootstrap_from_checkpoint(conn, &fetched.checkpoint, passphrase)?;
    // 5. 本机簿记事实不采纳来源端取值（时序约束见序列说明）。
    crate::settings::clear(conn, crate::settings::SettingKey::SyncLastSyncAt)?;
    // 6. 密文快照 × 明文本机：整库转密文（文件级原子替换，复用备份域机制）。
    if reencrypted {
        db::encryption::enable_encryption_for_file(db_path, passphrase.unwrap_or_default())?;
    }
    tracing::info!(
        generation = fetched.generation,
        size = fetched.size,
        reencrypted,
        "已从通道检查点完成新端引导"
    );
    Ok(BootstrapOutcome {
        generation: fetched.generation,
        size: fetched.size,
        reencrypted,
    })
}

/// 快照临时文件路径（系统临时目录 + 唯一名，收尾统一 [`fs_util::cleanup`]）。
fn temp_snapshot_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ledger-{tag}-{}.db", new_uuid()))
}
