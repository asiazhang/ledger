//! 商户字典域行为（issue #188 / ADR-0028）。
//!
//! 列表 / 创建 / 更新（改名）/ 软删除；`name` 在用行全库唯一——重名创建与改名
//! 撞名都返回明确错误（`AppError::Invalid`）。软删商户不再出现在列表（不可再被
//! 新交易选择），历史交易引用照常保留（交易侧校验见 `transaction::write::writer::normalize`）。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：本模块对备份域零感知，
//! 写入成功后的置脏/到期检查由调用方所在写入口闭包在提交点单点执行。

use rusqlite::{Connection, OptionalExtension};

use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

use super::command::{MerchantCommand, record_local};
use super::model::{Merchant, MerchantInput, MerchantTransactionCount, MerchantUpdateInput};

const MERCHANT_COLUMNS: &str = "id,name,created_at,updated_at,version,device_id,is_deleted";

/// 列表：默认仅未删除商户（字典语义，改名后即按新名排序）；
/// `include_deleted=true` 返回含软删全量（交易列表筛选下拉数据源：
/// 软删商户仍有历史交易，需可被选中过滤，issue #191）。
pub fn list_merchants(conn: &Connection, include_deleted: bool) -> Result<Vec<Merchant>> {
    let where_clause = if include_deleted {
        ""
    } else {
        "WHERE is_deleted=0"
    };
    query_all(
        conn,
        &format!(
            "SELECT {MERCHANT_COLUMNS} FROM merchants {where_clause} ORDER BY name, created_at"
        ),
        [],
    )
}

/// 创建商户，返回新商户 id。在用行同名（含改名目标）→ 明确错误。
pub fn create_merchant(conn: &Connection, input: MerchantInput) -> Result<String> {
    ensure_transaction(conn, || {
        let id = write_create(conn, &new_uuid(), &input.name)?;
        record_local(
            conn,
            MerchantCommand::Create {
                id: id.clone(),
                name: input.name,
            },
        )?;
        Ok(id)
    })
}

/// 商户落库协议（本地创建与重放共用，无 op 产出）：在用行同名检查 + 插入。
fn write_create(conn: &Connection, id: &str, name: &str) -> Result<String> {
    if merchant_name_taken(conn, name, None)? {
        return Err(AppError::codedp(
            "merchant.already-exists",
            format!("商户已存在: {name}"),
            &[name],
        ));
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO merchants (id,name,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,0)",
        rusqlite::params![id, name, now, now, 1, device_id(conn)?],
    )?;
    Ok(id.to_string())
}

/// 按 id 取单个在用商户；不存在（或已软删除）的 id → `AppError::NotFound`
///（码化 `merchant.not-found`，HTTP 修改端点与域层共用同一判定）。
pub fn get_merchant(conn: &Connection, id: &str) -> Result<Merchant> {
    query_all(
        conn,
        &format!("SELECT {MERCHANT_COLUMNS} FROM merchants WHERE id=?1 AND is_deleted=0"),
        rusqlite::params![id],
    )?
    .into_iter()
    .next()
    .ok_or_else(|| {
        AppError::codedp_not_found("merchant.not-found", format!("商户不存在: {id}"), &[id])
    })
}

/// 更新商户（改名）：字段省略即保持原值；入参先 trim（与导入即建同款归一，
/// 防带空白绕开唯一性产生碎商户），trim 后为空 → 明确错误；改名撞在用同名
/// → 明确错误。不存在（或已软删除）的 id → `AppError::NotFound`。
pub fn update_merchant(conn: &Connection, id: &str, input: MerchantUpdateInput) -> Result<()> {
    ensure_transaction(conn, || {
        let name = write_update(conn, id, &input)?;
        record_local(
            conn,
            MerchantCommand::Update {
                id: id.to_string(),
                name,
            },
        )
    })
}

/// 商户改名协议（本地修改与重放共用，无 op 产出）：trim 非空 + 改名撞名检查 +
/// 落库，返回解决后的落定名。
fn write_update(conn: &Connection, id: &str, input: &MerchantUpdateInput) -> Result<String> {
    let existing = get_merchant(conn, id)?;

    let name = match input.name {
        Some(ref raw) => {
            let name = raw.trim();
            if name.is_empty() {
                return Err(AppError::coded("merchant.name-required", "商户名不能为空"));
            }
            name.to_string()
        }
        None => existing.name,
    };
    if merchant_name_taken(conn, &name, Some(id))? {
        return Err(AppError::codedp(
            "merchant.already-exists",
            format!("商户已存在: {name}"),
            &[&name],
        ));
    }

    conn.execute(
        "UPDATE merchants SET name=?1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?4",
        rusqlite::params![&name, now_iso(), device_id(conn)?, id],
    )?;
    // 改名即时生效：交易以 merchant_id 引用，不回刷历史交易行（ADR-0028）。
    Ok(name)
}

/// 软删除商户（`is_deleted=1`）。不存在的 id → `AppError::NotFound`。
/// 历史交易引用保留（交易侧对软删商户仅拦截新写入），照常显示商户名。
pub fn delete_merchant(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, MerchantCommand::Delete { id: id.to_string() })
    })
}

/// 商户软删协议（本地删除与重放共用，无 op 产出）：存在性检查 + 软删。
fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM merchants WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::codedp_not_found(
            "merchant.not-found",
            format!("商户不存在: {id}"),
            &[id],
        ));
    }
    conn.execute(
        "UPDATE merchants SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

/// 商户关联交易计数聚合（issue #445，毛笔数口径）：每个商户（含软删行）被**未删**
/// 流水引用的条数——凡引用该商户的流水各计 1 条（可携带商户的 kind 由行为层收口，
/// 仅 `expense` / `refund` / `income`），不做 refund 冲销、不做净笔数；实时按流水
/// 推导、不落库。无引用商户计 0（LEFT JOIN 保证全字典行都在返回内，消费方无需
/// 自行补零）。软删商户照常计数（历史引用语义，与报表商户排名同款：只滤交易侧
/// 软删、不滤商户侧）；报表商户排名的金额口径（expense_net）不与之混淆。
pub fn transaction_counts(conn: &Connection) -> Result<Vec<MerchantTransactionCount>> {
    query_all(
        conn,
        "SELECT m.id, COUNT(t.id) AS transaction_count \
         FROM merchants m \
         LEFT JOIN transactions t ON t.merchant_id=m.id AND t.is_deleted=0 \
         GROUP BY m.id ORDER BY m.name, m.created_at",
        [],
    )
}

/// 商户名归一化查找（AI 导入契约，issue #194 / ADR-0028）：按名字精确匹配在用商户
/// （trim 归一），命中返回 id，未命中返回 `None`。空名（trim 后）返回 `None`，
/// 由调用方决定报错时机。配合 [`create_merchant_by_name`] 使用：行为层先查、
/// 行内校验全部通过后再即建——失败行不产生碎商户。
pub fn find_merchant_by_name(conn: &Connection, name: &str) -> Result<Option<String>> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(None);
    }
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM merchants WHERE name=?1 AND is_deleted=0",
            rusqlite::params![name],
            |r| r.get(0),
        )
        .optional()?;
    Ok(id)
}

/// 按名字即建商户（trim 归一；trim 后为空 → 明确错误）。名字命中在用商户时
/// 直接复用（find-or-create 语义，不撞唯一索引）；软删商户不算命中，同名即建新行
/// （唯一索引只约束在用行）。归一化责任收口在后端，AI 提交商户名字符串即可，
/// 不负责商户去重。
pub fn create_merchant_by_name(conn: &Connection, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::coded("merchant.name-required", "商户名不能为空"));
    }
    if let Some(id) = find_merchant_by_name(conn, name)? {
        return Ok(id);
    }
    create_merchant(
        conn,
        MerchantInput {
            name: name.to_string(),
        },
    )
}

/// 在用行（`is_deleted=0`）中是否已有同名商户；`exclude_id` 用于改名场景排除自身。
/// 一条 SQL 覆盖两种形态：创建（无排除）与改名（排除自身）。
fn merchant_name_taken(conn: &Connection, name: &str, exclude_id: Option<&str>) -> Result<bool> {
    let found: bool = conn
        .query_row(
            "SELECT 1 FROM merchants WHERE name=?1 AND is_deleted=0 AND (?2 IS NULL OR id<>?2)",
            rusqlite::params![name, exclude_id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    Ok(found)
}

/// 重放执行：创建（同名冲突码化报错 → 挂起待裁决）。
pub(crate) fn replay_create(conn: &Connection, id: &str, name: &str) -> Result<()> {
    write_create(conn, id, name)?;
    Ok(())
}

/// 重放执行：改名（同名冲突码化报错 → 挂起待裁决）。
pub(crate) fn replay_update(conn: &Connection, id: &str, name: &str) -> Result<()> {
    write_update(
        conn,
        id,
        &MerchantUpdateInput {
            name: Some(name.to_string()),
        },
    )?;
    Ok(())
}

/// 重放执行：软删除（同一协议含存在性检查）。
pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    write_delete(conn, id)
}

// ---------------------------------------------------------------------------
// 交易×商户接缝实现（spec #1086 / issue #1092）
// ---------------------------------------------------------------------------

/// 注册商户名归一化实现（核心交易域 `transaction::seams::merchant` 注册点，#1092）：
/// 把按名查找与按名即建两支实现原子装入，壳层启动接线，业务代码不直接调用。
pub fn install_merchant_hooks() {
    ledger_transaction::seams::merchant::register_merchant_hooks(
        ledger_transaction::seams::merchant::MerchantNameHooks {
            find_by_name: find_merchant_by_name,
            create_by_name: create_merchant_by_name,
        },
    );
}
