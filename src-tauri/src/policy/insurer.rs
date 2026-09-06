//! 保司字典（Insurer，issue #712 / ADR-0082 决策 1/4）。
//!
//! 保险公司升格为保险域自有独立字典：不复用核心交易域商户（商户是个人消费轨迹，
//! 保司是公共机构名，两画不同构）；单消费方 Policy，不进参考数据域。字典语义
//! 照抄商户先例：名字在用行全库唯一（软删行不占名字）、软删除（已删不进默认
//! 列表，含已删查询可见）、改名即时生效（引用指向 id，不回刷历史行）。
//!
//! 即席创建 find-or-create：[`find_insurer_by_name`] 精确命中（trim 归一）复用、
//! 未命中经 [`create_insurer_by_name`] 即建——保单表单选择器与后续换轨的建档
//! 路径共用此语义；归一化责任收口在本域，调用方提交名字符串即可。
//!
//! 模型与 CRUD 收口本文件（体量小不拆 model/crud）；种子在迁移 V019（确定性
//! UUID v5 + 按名 INSERT OR IGNORE），种子行为普通字典行，本模块无任何特殊处理。

use rusqlite::{Connection, OptionalExtension};

use crate::db::query::{FromRow, query_all};
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};

/// 保司字典行（参考数据模式，与商户同款审计字段；名字字典，无视觉字段）。
#[derive(Debug, serde::Serialize, serde::Deserialize, Clone, utoipa::ToSchema)]
pub struct Insurer {
    pub id: String,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    pub is_deleted: bool,
}

/// 创建入参：名字（在用行全库唯一，重名被拒）。
#[derive(Debug, serde::Deserialize, utoipa::ToSchema)]
pub struct InsurerInput {
    pub name: String,
}

/// 更新入参：`name` 可省略（省略即保持原值，等价空更新）；改名须避开在用同名。
#[derive(Debug, serde::Deserialize)]
pub struct InsurerUpdateInput {
    pub name: Option<String>,
}

impl FromRow for Insurer {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Insurer {
            id: row.get(0)?,
            name: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
            version: row.get(4)?,
            device_id: row.get(5)?,
            is_deleted: row.get::<_, i64>(6)? != 0,
        })
    }
}

const INSURER_COLUMNS: &str = "id,name,created_at,updated_at,version,device_id,is_deleted";

/// 列表：默认仅未删除保司（字典语义，改名后即按新名排序）；
/// `include_deleted=true` 返回含软删全量（保司管理「显示已删」只读切换的数据源，
/// 消费方负责只读语义）。种子保司与即建保司同为普通字典行，本查询不做区分。
pub fn list_insurers(conn: &Connection, include_deleted: bool) -> Result<Vec<Insurer>> {
    let where_clause = if include_deleted {
        ""
    } else {
        "WHERE is_deleted=0"
    };
    query_all(
        conn,
        &format!("SELECT {INSURER_COLUMNS} FROM insurers {where_clause} ORDER BY name, created_at"),
        [],
    )
}

/// 创建保司，返回新保司 id。在用行同名（含改名目标）→ 明确错误（码化，
/// 错误模板 `insurer.already-exists`）；软删行不占名字（唯一索引只约束在用行）。
pub fn create_insurer(conn: &Connection, input: InsurerInput) -> Result<String> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err(AppError::coded("insurer.name-required", "保司名不能为空"));
    }
    if insurer_name_taken(conn, name, None)? {
        return Err(AppError::codedp(
            "insurer.already-exists",
            format!("保司已存在: {name}"),
            &[name],
        ));
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO insurers (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,0)",
        rusqlite::params![id, name, now, now, 1, device_id()],
    )?;
    Ok(id)
}

/// 更新保司（改名）：字段省略即保持原值；改名撞在用同名 → 明确错误。
/// 不存在（或已软删除）的 id → 码化 NotFound。
pub fn update_insurer(conn: &Connection, id: &str, input: InsurerUpdateInput) -> Result<()> {
    let existing: Insurer = query_all(
        conn,
        &format!("SELECT {INSURER_COLUMNS} FROM insurers WHERE id=?1 AND is_deleted=0"),
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| {
        AppError::codedp_not_found("insurer.not-found", format!("保司不存在: {id}"), &[id])
    })?;

    let name = input.name.unwrap_or(existing.name);
    if insurer_name_taken(conn, &name, Some(id))? {
        return Err(AppError::codedp(
            "insurer.already-exists",
            format!("保司已存在: {name}"),
            &[&name],
        ));
    }

    conn.execute(
        "UPDATE insurers SET name=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
        rusqlite::params![name, now_iso(), device_id(), id],
    )?;
    // 改名即时生效：引用指向 insurer_id，不回刷历史行（ADR-0082 决策 1）。
    Ok(())
}

/// 软删除保司（`is_deleted=1`）。不存在的 id → 码化 NotFound。
/// 存量引用保留照常显示（软删保司不可被新保单选择，由消费方校验，本票只管字典）。
pub fn delete_insurer(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM insurers WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::codedp_not_found(
            "insurer.not-found",
            format!("保司不存在: {id}"),
            &[id],
        ));
    }
    conn.execute(
        "UPDATE insurers SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 保司名归一化查找：按名字精确匹配在用保司（trim 归一），命中返回 id，未命中
/// 返回 `None`。空名（trim 后）返回 `None`。配合 [`create_insurer_by_name`] 使用。
pub fn find_insurer_by_name(conn: &Connection, name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM insurers WHERE name=?1 AND is_deleted=0",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id)
}

/// 按名即建保司（find-or-create，trim 归一；trim 后为空 → 明确错误）。名字命中
/// 在用保司时直接复用（不撞唯一索引）；软删保司不算命中，同名即建新行
/// （唯一索引只约束在用行，先例：商户 find-or-create）。
pub fn create_insurer_by_name(conn: &Connection, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::coded("insurer.name-required", "保司名不能为空"));
    }
    if let Some(id) = find_insurer_by_name(conn, name)? {
        return Ok(id);
    }
    create_insurer(
        conn,
        InsurerInput {
            name: name.to_string(),
        },
    )
}

/// 在用行（`is_deleted=0`）中是否已有同名保司；`exclude_id` 用于改名场景排除自身。
/// 一条 SQL 覆盖两种形态：创建（无排除）与改名（排除自身）。
fn insurer_name_taken(conn: &Connection, name: &str, exclude_id: Option<&str>) -> Result<bool> {
    let found: bool = conn
        .query_row(
            "SELECT 1 FROM insurers WHERE name=?1 AND is_deleted=0 AND (?2 IS NULL OR id<>?2)",
            rusqlite::params![name, exclude_id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    Ok(found)
}
