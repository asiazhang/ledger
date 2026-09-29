//! 启动期 schema 漂移守卫（issue #971/#992 / ADR-0100）：迁移后校验「库 schema
//! 与迁移链一致」，漂移即报码化错误，不静默带病运行。
//!
//! 背景与机制决策见 ADR-0100：`rusqlite_migration` 的迁移裁决只比
//! `PRAGMA user_version`，对未发布迁移就地修改（#272 发布约定允许）后，已跑过
//! 旧版迁移的库版本号未变、重跑不触发——应用正常启动，直到业务写路径才炸
//! （#971 实测 `park_params`，先例 `merchant_id`）。
//!
//! 接线在 [`super::init_db`] 尾部单点：全部生产建连路径（明文启动、解锁换连、
//! 重置、备份恢复、加密转换、checkpoint 重建）统一收口，守卫一处接线零遗漏。
//! 漂移失败经 `boot_sequence` Err → `BootFailureGate` → 启动失败恢复屏（既有
//! 通道，#601/#602）；解锁路径失败在解锁屏按码呈现。
//!
//! **双库校验**（ADR-0139 决策 4，issue #1871）：V036 拆库后迁移链的产物横跨
//! main（账本数据）与 attached `sync`（同步元数据四表）两侧，守卫按库分侧比对
//! ——两侧各自与参照的同侧做方向性 diff。attached 侧缺席（别名未挂载）按空侧
//! 参与比对：参照侧四表齐备即漂移，不静默放过「主库在、元数据库不在」的世界。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

use rusqlite::Connection;

use crate::error::{AppError, Result};

/// 启动期 schema 漂移的稳定错误码（issue #992 / ADR-0100 决策 3，ADR-0050
/// 只增不改）：不复用 `boot.db-unreadable`——漂移库打得开且数据完好，恢复通道
/// 中「从备份恢复」应作为首选动作呈现，与「库不可读」（重置优先）的处置顺序
/// 有真实差异，前端按码区分文案（#994）。错误模板见
/// `packages/i18n/src/locales/{zh-CN,en-US}/errors.json` 的 `boot.schema-drift`。
pub const BOOT_SCHEMA_DRIFT: &str = "boot.schema-drift";

/// 比对侧的库对象清单与表列集（一侧 = main 或 attached `sync`）。
#[derive(Default)]
struct SideSchema {
    /// 对象清单（表/视图/索引，`(type, name)` 有序集合——BTreeSet 保证 diff
    /// 结果（诊断日志）顺序确定，不随扫描顺序漂移）。
    objects: BTreeSet<(String, String)>,
    /// 表名 → 列名集合（仅表；列比对只对两边都在场的表进行，视图/索引不入）。
    table_columns: BTreeMap<String, BTreeSet<String>>,
}

/// 参照 schema（迁移链的纯函数产物，按库分侧）。
struct ReferenceSchema {
    main: SideSchema,
    sync: SideSchema,
}

/// 一侧的库对象清单（表/视图/索引，`sqlite_master`，`sqlite_%` 内部对象排除）。
///
/// `sqlite_%` 内部对象（`sqlite_stat1`/`sqlite_stat4` 统计表、
/// `sqlite_sequence` 计数表、`sqlite_autoindex_*` 隐式索引等）由引擎自主
/// 创建回收（`ANALYZE`/`PRAGMA optimize`/AUTOINCREMENT），非迁移链声明
/// 对象；统计表的存在性还随引擎编译选项漂移：bundled 引擎（
/// SQLITE_ENABLE_STAT4）参照库在 V016 尾 `ANALYZE` 必产 `sqlite_stat4`，
/// 而经非 STAT4 构建（如 macOS 系统 sqlite3）刷新过统计的实际库只有
/// `sqlite_stat1`，不排除即把引擎实现细节误判为漂移、启动失败。先例：
/// 迁移审计外键不变量与 checkpoint 对象清单同用 `name NOT LIKE 'sqlite_%'`。
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

/// 一侧里某表的列名集合（schema 限定的 `pragma_table_info` 表值函数形态，
/// 先例：迁移审计测试）。
fn table_columns(conn: &Connection, db: &str, table: &str) -> rusqlite::Result<BTreeSet<String>> {
    let mut stmt = conn.prepare(&format!("SELECT name FROM {db}.pragma_table_info(?1)"))?;
    stmt.query_map([table], |row| row.get(0))?.collect()
}

/// 一侧的完整 schema 快照（对象清单 + 逐表列集）。
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

/// attached `sync` 别名是否在位（`pragma_database_list`；守卫只关心别名——
/// 表级齐备与否是 diff 的结论而非前置）。
fn sync_alias_attached(conn: &Connection) -> bool {
    conn.query_row(
        "SELECT count(*) FROM pragma_database_list WHERE name = 'sync'",
        [],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| n > 0)
    .unwrap_or(false)
}

/// 参照集构建（进程内单次；构建成本受 ADR-0009 100ms 观测线约束，ADR-0100 性能
/// 定语）。直接调迁移链、不经 `init_db`——避免递归守卫。参照世界由
/// [`super::open_in_memory`] 建立成对挂载形态（attached `:memory:` 承载 sync
/// 侧），与被校验世界同构。
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

/// 单侧方向性 diff（参照有而实际缺 = 漂移；实际多出 = 容忍——V005 搜索索引
/// 残留等合法遗留不误报，ADR-0027 / ADR-0100 决策 2）。返回缺失诊断清单
/// （`main:` / `sync:` 侧前缀定位漂移侧别）。
fn diff_side(side: &str, reference: &SideSchema, actual: &SideSchema) -> Vec<String> {
    let mut missing: Vec<String> = Vec::new();
    for (kind, name) in reference.objects.difference(&actual.objects) {
        missing.push(format!("{side}: {kind} {name}"));
    }
    // 列集只对「两边都在场的表」比对：参照表实际缺已属对象级漂移，重复报
    // 只会稀释诊断；实际多出的表按方向性容忍，不入列比对。
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

/// schema 一致性校验（守卫本体，[`super::init_db`] 尾部接线）：用进程内缓存的
/// 参照 schema（见 [`reference_schema`]，迁移链从零构建的纯函数）与真实库做
/// **双库方向性 diff**（ADR-0139 决策 4）：main 与 attached `sync` 两侧各自与
/// 参照同侧比对，参照有而实际缺的对象（表/视图/索引）或列 = 漂移；实际多出 =
/// 容忍。attached 侧缺席按空侧参与比对（参照侧四表即漂移，不静默放过）。
/// `sqlite_%` 内部对象双向排除、不参与比对——引擎自主管理，非迁移链声明
/// （详见 [`schema_objects`]）。参照库由迁移链自动构建，零手工清单维护。
pub(crate) fn verify_schema(actual: &Connection) -> Result<()> {
    let reference = reference_schema()?;

    let actual_main = side_schema(actual, "main")?;
    let actual_sync = if sync_alias_attached(actual) {
        side_schema(actual, "sync")?
    } else {
        SideSchema::default()
    };

    let mut missing: Vec<String> = diff_side("main", &reference.main, &actual_main);
    missing.extend(diff_side("sync", &reference.sync, &actual_sync));

    if missing.is_empty() {
        return Ok(());
    }

    // 诊断（ADR-0100）：失败时附缺失对象/列清单（`侧:` 前缀定位漂移侧别），
    // 定位漂移成因（哪次就地修改漏了前向迁移）。用户文案固定不走插值，清单
    // 只落日志。
    tracing::error!(
        missing = ?missing,
        "schema 漂移：数据库缺少迁移链声明的对象或列"
    );
    Err(AppError::coded(
        BOOT_SCHEMA_DRIFT,
        "数据库结构异常，可从备份恢复或重置",
    ))
}
