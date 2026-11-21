use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use rusqlite::Connection;

use crate::error::{AppError, Result};

pub const BOOT_SCHEMA_DRIFT: &str = "boot.schema-drift";

#[derive(Default)]
struct SideSchema {
    objects: BTreeSet<(String, String)>,
    table_columns: BTreeMap<String, BTreeSet<String>>,
}

struct ReferenceSchema {
    main: SideSchema,
    sync: SideSchema,
}

fn schema_objects(conn: &Connection, db: &str) -> rusqlite::Result<BTreeSet<(String, String)>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT type, name FROM {db}.sqlite_master \
         WHERE type IN ('table', 'view', 'index') AND name NOT LIKE 'sqlite_%'"
    ))?;
    stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?
    .collect()
}

fn table_columns(conn: &Connection, db: &str, table: &str) -> rusqlite::Result<BTreeSet<String>> {
    let mut stmt = conn.prepare(&format!("SELECT name FROM {db}.pragma_table_info(?1)"))?;
    stmt.query_map([table], |row| row.get(0))?.collect()
}

fn side_schema(conn: &Connection, db: &str) -> rusqlite::Result<SideSchema> {
    let objects = schema_objects(conn, db)?;
    let mut table_columns_map = BTreeMap::new();
    for (kind, name) in &objects {
        if kind != "table" {
            continue;
        }
        let columns = table_columns(conn, db, name)?;
        table_columns_map.insert(name.clone(), columns);
    }
    Ok(SideSchema {
        objects,
        table_columns: table_columns_map,
    })
}

fn reference_schema() -> Result<&'static ReferenceSchema> {
    static REFERENCE: OnceLock<std::result::Result<ReferenceSchema, AppError>> = OnceLock::new();
    REFERENCE
        .get_or_init(|| {
            let mut reference = super::open_in_memory()?;
            super::migrations().to_latest(&mut reference)?;
            Ok(ReferenceSchema {
                main: side_schema(&reference, "main")?,
                sync: side_schema(&reference, "sync")?,
            })
        })
        .as_ref()
        .map_err(AppError::clone)
}

fn diff_side(side: &str, reference: &SideSchema, actual: &SideSchema) -> Vec<String> {
    let mut missing: Vec<String> = Vec::new();
    for (kind, name) in reference.objects.difference(&actual.objects) {
        missing.push(format!("{side}: {kind} {name}"));
    }
    for (name, reference_columns) in &reference.table_columns {
        if !actual
            .objects
            .contains(&("table".to_string(), name.clone()))
        {
            continue;
        }
        if let Some(actual_columns) = actual.table_columns.get(name) {
            for column in reference_columns.difference(actual_columns) {
                missing.push(format!("{side}: {name}.{column}"));
            }
        }
    }
    missing
}

pub(crate) fn verify_schema(actual: &Connection) -> Result<()> {
    let reference = reference_schema()?;

    let actual_main = side_schema(actual, "main")?;
    let actual_sync = if super::connection::sync_alias_attached(actual) {
        side_schema(actual, "sync")?
    } else {
        SideSchema::default()
    };

    let mut missing: Vec<String> = diff_side("main", &reference.main, &actual_main);
    missing.extend(diff_side("sync", &reference.sync, &actual_sync));

    if missing.is_empty() {
        return Ok(());
    }

    tracing::error!(
        missing = ?missing,
        "schema 漂移：数据库缺少迁移链声明的对象或列"
    );
    Err(AppError::coded(
        BOOT_SCHEMA_DRIFT,
        "数据库结构异常，可从备份恢复或重置",
    ))
}
