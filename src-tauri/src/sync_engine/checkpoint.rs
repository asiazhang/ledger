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
//! 换入，覆盖既有同步状态等于丢账。目标残留未产出 op 的业务数据属壳层加入
//! 流程（#862）的义务边界：加入即新库。

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::{self, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::fs_util;
use crate::transaction::ensure_transaction;

use super::device;
use super::ops;
use super::positions::{self, StreamPosition};

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
    // 守卫：目标尚未参与同步（三张同步元数据表全空）。
    for (table, empty) in [
        ("sync_ops", ops::is_empty(conn)?),
        ("sync_stream_positions", positions::list(conn)?.is_empty()),
        ("sync_parked_ops", super::parked::is_empty(conn)?),
    ] {
        if !empty {
            return Err(AppError::codedp(
                "sync-engine.bootstrap-not-fresh",
                format!("本机已参与同步（{table} 非空），拒绝整库引导以免覆盖既有同步状态"),
                &[table],
            ));
        }
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
        eprintln!("DBG-ATTACH-ERR: {e:?}");
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
            return Err(AppError::Invalid(format!(
                "检查点快照外键校验失败（{violations} 处），快照不可用"
            )));
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

/// 快照临时文件路径（系统临时目录 + 唯一名，收尾统一 [`fs_util::cleanup`]）。
fn temp_snapshot_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ledger-{tag}-{}.db", new_uuid()))
}
