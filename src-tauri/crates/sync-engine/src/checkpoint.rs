use std::path::{Path, PathBuf};

use rusqlite::Connection;

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{self, new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_infra::fs_util;

use ledger_sync_protocol::device;
use ledger_sync_protocol::position::{self as positions, StreamPosition};

use super::channel::FetchedCheckpoint;
use super::ops;
use super::trigger::SyncChannel;

const SNAP_ALIAS: &str = "sync_snap";

const SNAP_META_ALIAS: &str = "sync_snap_meta";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkpoint {
    pub positions: Vec<StreamPosition>,
    pub snapshot: Vec<u8>,
    pub sync_snapshot: Vec<u8>,
}

pub fn create_checkpoint(conn: &Connection) -> Result<Checkpoint> {
    let snapshot_path = temp_snapshot_path("cp");
    let sync_snapshot_path = temp_snapshot_path("cp-sync");
    let result = (|| -> Result<Checkpoint> {
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![snapshot_path.to_string_lossy()],
        )?;
        db::snapshot_sync_tables_into(conn, &sync_snapshot_path)?;
        Ok(Checkpoint {
            positions: positions::list(conn)?,
            snapshot: std::fs::read(&snapshot_path)?,
            sync_snapshot: std::fs::read(&sync_snapshot_path)?,
        })
    })();
    fs_util::cleanup(&snapshot_path);
    fs_util::cleanup(&sync_snapshot_path);
    result
}

pub fn bootstrap_from_checkpoint(
    conn: &mut Connection,
    checkpoint: &Checkpoint,
    passphrase: Option<&str>,
) -> Result<()> {
    if let Some(table) = first_sync_table_not_empty(conn)? {
        return Err(AppError::codedp(
            "sync-engine.bootstrap-not-fresh",
            format!("本机已参与同步（{table} 非空），拒绝整库引导以免覆盖既有同步状态"),
            &[table],
        ));
    }
    let own_device = device::device_id(conn)?;
    let own_clock: i64 = conn.query_row(
        "SELECT logical_clock FROM sync_device WHERE id = ?1",
        [&own_device],
        |r| r.get(0),
    )?;

    let snapshot_path = temp_snapshot_path("restore");
    let sync_snapshot_path = temp_snapshot_path("restore-sync");
    std::fs::write(&snapshot_path, &checkpoint.snapshot)?;
    if !checkpoint.sync_snapshot.is_empty() {
        std::fs::write(&sync_snapshot_path, &checkpoint.sync_snapshot)?;
    }
    let result = (|| -> Result<()> {
        attach_snapshot(conn, SNAP_ALIAS, &snapshot_path, passphrase)?;
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
            rebuild_main_from_snapshot(conn, snapshot_version)?;
            if checkpoint.sync_snapshot.is_empty() {
                Ok(())
            } else {
                consume_sync_snapshot(conn, &sync_snapshot_path, passphrase)
            }
        })();
        let detach = conn.execute(&format!("DETACH DATABASE {SNAP_ALIAS}"), []);
        rebuild?;
        detach?;
        Ok(())
    })();
    fs_util::cleanup(&snapshot_path);
    fs_util::cleanup(&sync_snapshot_path);
    result?;

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
        sync_snapshot_bytes = checkpoint.sync_snapshot.len(),
        "已从检查点快照完成新端引导"
    );
    Ok(())
}

fn consume_sync_snapshot(
    conn: &Connection,
    sync_snapshot_path: &Path,
    passphrase: Option<&str>,
) -> Result<()> {
    attach_snapshot(conn, SNAP_META_ALIAS, sync_snapshot_path, passphrase)?;
    let rebuild = rebuild_sync_tables_from_snapshot(conn);
    let detach = conn.execute(&format!("DETACH DATABASE {SNAP_META_ALIAS}"), []);
    rebuild?;
    detach?;
    Ok(())
}

fn rebuild_sync_tables_from_snapshot(conn: &Connection) -> Result<()> {
    ensure_transaction(conn, || {
        for table in db::SYNC_TABLES {
            conn.execute(&format!("DROP TABLE IF EXISTS sync.{table}"), [])?;
            let ddl: String = conn.query_row(
                &format!(
                    "SELECT sql FROM {SNAP_META_ALIAS}.sqlite_master \
                     WHERE type = 'table' AND name = '{table}'"
                ),
                [],
                |r| r.get(0),
            )?;
            let qualified = ddl.replacen("CREATE TABLE ", "CREATE TABLE sync.", 1);
            conn.execute(&qualified, [])?;
            conn.execute(
                &format!("INSERT INTO sync.{table} SELECT * FROM {SNAP_META_ALIAS}.{table}"),
                [],
            )?;
        }
        let placeholders = db::SYNC_TABLES
            .iter()
            .map(|t| format!("'{t}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT sql FROM {SNAP_META_ALIAS}.sqlite_master \
             WHERE type = 'index' AND sql IS NOT NULL AND tbl_name IN ({placeholders})"
        ))?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        let index_ddl = rows.collect::<std::result::Result<Vec<_>, _>>()?;
        for sql in index_ddl {
            let qualified = sql
                .replacen("CREATE UNIQUE INDEX ", "CREATE UNIQUE INDEX sync.", 1)
                .replacen("CREATE INDEX ", "CREATE INDEX sync.", 1);
            conn.execute(&qualified, [])?;
        }
        Ok(())
    })
}

fn attach_snapshot(
    conn: &Connection,
    alias: &str,
    path: &Path,
    passphrase: Option<&str>,
) -> Result<()> {
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
                &format!("ATTACH DATABASE ?1 AS {alias} KEY ?2"),
                passphrase,
                path,
                alias,
            )
        }
        db::encryption::DbFileKind::Plaintext | db::encryption::DbFileKind::Empty => attach_sql(
            conn,
            &format!("ATTACH DATABASE ?1 AS {alias} KEY ?2"),
            "",
            path,
            alias,
        ),
    }
}

fn attach_sql(conn: &Connection, sql: &str, key: &str, path: &Path, alias: &str) -> Result<()> {
    conn.execute(sql, rusqlite::params![path.to_string_lossy(), key])
        .map_err(|e| {
            if db::encryption::is_not_a_database(&e) {
                db::encryption::passphrase_incorrect_error()
            } else {
                e.into()
            }
        })?;
    if let Err(e) = conn.query_row(
        &format!("SELECT count(*) FROM {alias}.sqlite_master"),
        [],
        |r| r.get::<_, i64>(0),
    ) {
        if db::encryption::is_not_a_database(&e) {
            return Err(db::encryption::passphrase_incorrect_error());
        }
        return Err(e.into());
    }
    Ok(())
}

fn rebuild_main_from_snapshot(conn: &Connection, snapshot_version: i64) -> Result<()> {
    conn.execute("PRAGMA foreign_keys = OFF", [])?;
    let rebuilt = ensure_transaction(conn, || {
        for drop_sql in drop_statements(conn)? {
            conn.execute(&drop_sql, [])?;
        }
        let objects = snapshot_objects(conn)?;
        for sql in objects.tables() {
            conn.execute(sql, [])?;
        }
        for (table, bare) in objects.table_names.iter().zip(&objects.bare_names) {
            let columns = insertable_columns(conn, bare)?;
            conn.execute(
                &format!("INSERT INTO main.{table} ({columns}) SELECT {columns} FROM {SNAP_ALIAS}.{table}"),
                [],
            )?;
        }
        for sql in objects.subordinates() {
            conn.execute(sql, [])?;
        }
        conn.execute(&format!("PRAGMA user_version = {snapshot_version}"), [])?;
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

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

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

const SEED_DEVICE_ID: &str = "seed";

pub(crate) fn library_has_user_data(conn: &Connection) -> Result<bool> {
    for table in PROBE_TABLES {
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapOutcome {
    pub generation: i64,
    pub size: u64,
    pub reencrypted: bool,
}

pub trait BootstrapConnSegments {
    fn with_conn<T>(&self, use_conn: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T>;
}

pub fn bootstrap_from_channel<S: BootstrapConnSegments>(
    segments: &S,
    db_path: &Path,
    channel: &SyncChannel,
    passphrase: Option<&str>,
) -> Result<BootstrapOutcome> {
    segments.with_conn(|conn| bootstrap_preflight(conn))?;
    let fetched = channel.fetch_checkpoint(passphrase)?;
    segments.with_conn(|conn| bootstrap_apply(conn, db_path, &fetched, passphrase))
}

fn bootstrap_apply(
    conn: &mut Connection,
    db_path: &Path,
    fetched: &FetchedCheckpoint,
    passphrase: Option<&str>,
) -> Result<BootstrapOutcome> {
    bootstrap_preflight(conn)?;
    let local_encrypted =
        db::encryption::probe_file_kind(db_path)? == db::encryption::DbFileKind::Encrypted;
    if fetched.sealed && local_encrypted {
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
    bootstrap_from_checkpoint(conn, &fetched.checkpoint, passphrase)?;
    ledger_infra::settings::clear(conn, ledger_infra::settings::SettingKey::SyncLastSyncAt)?;
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

fn temp_snapshot_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("ledger-{tag}-{}.db", new_uuid()))
}
