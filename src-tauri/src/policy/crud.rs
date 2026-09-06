//! 保单档案 CRUD 域行为：列表、创建、编辑与软删除。

use rusqlite::{Connection, OptionalExtension};

use super::model::{Policy, PolicyInput};
use crate::db::query::{query_all, query_one};
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};

use super::validation::validate_input;

/// 保单全列清单（读路径共用，与 `FromRow` 的列名约定一致）。
const POLICY_COLUMNS: &str = "id,insurer_id,policy_number,product_name,start_date,end_date,\
     coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted";

/// 按 `id` 读未删除保单（多命令共用的前检）：不存在（或已软删除）返回 `None`。
fn get_policy_by_id(conn: &Connection, id: &str) -> Result<Option<Policy>> {
    query_one(
        conn,
        &format!("SELECT {POLICY_COLUMNS} FROM policies WHERE id=?1 AND is_deleted=0"),
        [id],
    )
}

/// 列出全部未删除保单，排序按创建先后（created_at 升序），保证列表稳定。
/// 已删保单不进列表；到期状态不在此推导（展示层由保障期间即时推导，不持久化）。
pub fn list_policies(conn: &Connection) -> Result<Vec<Policy>> {
    query_all(
        conn,
        &format!(
            "SELECT {POLICY_COLUMNS} FROM policies WHERE is_deleted=0 ORDER BY created_at, id"
        ),
        [],
    )
}

/// 创建一张保单：校验 → 落库（生成 `id` 与审计字段）→ 成功后调用 `notify`
/// （生产路径发 `ledger:changed`）。校验语义见 `validation::validate_input`。
pub fn create_policy(
    conn: &Connection,
    input: PolicyInput,
    notify: &mut dyn FnMut(),
) -> Result<String> {
    let normalized = validate_input(conn, &input, false)?;

    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO policies \
         (id,insurer_id,policy_number,product_name,start_date,end_date,\
         coverage_amount_cents,coverage_currency_code,note,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10,1,?11,0)",
        rusqlite::params![
            id,
            normalized.insurer_id,
            normalized.policy_number,
            normalized.product_name,
            normalized.start_date,
            normalized.end_date,
            normalized.coverage_amount_cents,
            normalized.coverage_currency_code,
            normalized.note,
            now,
            device_id(),
        ],
    )?;
    // 写入成功 → 通知调用方发出失效信号（生产为 ledger:changed；失败不至此处）。
    notify();
    Ok(id)
}

/// 按 `id` 编辑保单静态要素（全量替换）：保留审计字段（`id` / `created_at` /
/// `is_deleted`），`version` 递增、`updated_at` / `device_id` 刷新（同 Writer
/// 接缝的 `update_row` 约定）。不存在（或已软删除）→ [`AppError::NotFound`]。
/// 成功后调用 `notify`（生产路径发 `ledger:changed`）。
///
/// 保司校验语义与 Writer 接缝一致（`existing_merchant_id` 先例）：提交的保司与
/// 既有行相同 = 维持历史引用（保司后被软删的历史保单仍可编辑其他要素），
/// 换成新保司才校验在用（软删保司不可被新档案选择，ADR-0082）。
pub fn update_policy(
    conn: &Connection,
    id: &str,
    input: PolicyInput,
    notify: &mut dyn FnMut(),
) -> Result<()> {
    let existing = get_policy_by_id(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found("policy.not-found", format!("保单不存在: {id}"), &[id])
    })?;

    let insurer_unchanged = existing.insurer_id == input.insurer_id;
    let normalized = validate_input(conn, &input, insurer_unchanged)?;

    let updated = conn.execute(
        "UPDATE policies SET insurer_id=?2, policy_number=?3, product_name=?4, start_date=?5, \
         end_date=?6, coverage_amount_cents=?7, coverage_currency_code=?8, note=?9, \
         updated_at=?10, version=version+1, device_id=?11 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            normalized.insurer_id,
            normalized.policy_number,
            normalized.product_name,
            normalized.start_date,
            normalized.end_date,
            normalized.coverage_amount_cents,
            normalized.coverage_currency_code,
            normalized.note,
            now_iso(),
            device_id(),
        ],
    )?;
    debug_assert_eq!(
        updated, 1,
        "前置存在性检查已排除 id 不存在/软删除，单连接下不可达"
    );
    notify();
    Ok(())
}

/// 软删除保单（`is_deleted=1`，不物理移除）：标准列表（`WHERE is_deleted=0`）
/// 自动过滤；库内行与既有引用列**原样保留、不置空**（ADR-0051 决策 5：档案的
/// 历史语义不可毁）。不存在（含已删除）的 id → [`AppError::NotFound`]。
/// 成功后调用 `notify`（生产路径发 `ledger:changed`）。
pub fn delete_policy(conn: &Connection, id: &str, notify: &mut dyn FnMut()) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM policies WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::coded_not_found(
            "policy.not-found",
            format!("保单不存在: {id}"),
        ));
    }
    conn.execute(
        "UPDATE policies SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    notify();
    Ok(())
}
