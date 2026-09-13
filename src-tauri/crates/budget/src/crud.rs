//! 预算 CRUD 域行为（issue #91 / #183 / #184）。
//!
//! 预算写入与清单读取均以 `&Connection` 为域接缝；IPC 壳只负责参数解包与事务边界。
//! 金额口径由 `transaction::amount` 的 kind→度量矩阵单一真源驱动，预算进度的
//! `ExpenseNet` 计算位于同级 `progress` 模块。

use rusqlite::{Connection, OptionalExtension};

use super::command::{BudgetCommand, record_local};
use super::model::{Budget, BudgetInput, BudgetPeriod};
use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

/// 列出全部未删除预算，排序按创建先后。
pub fn list_budgets(conn: &Connection) -> Result<Vec<Budget>> {
    query_all(
        conn,
        "SELECT id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted \
         FROM budgets WHERE is_deleted=0 ORDER BY created_at",
        [],
    )
}

/// 金额与分类校验（issue #184 起 create 与 update 共用）：金额必须为正数；
/// 只能挂支出分类（收入分类与不存在的分类均拒绝）。
fn validate_amount_and_category(
    conn: &Connection,
    category_id: &str,
    amount_cents: i64,
) -> Result<()> {
    if amount_cents <= 0 {
        return Err(AppError::coded(
            "budget.amount-positive",
            "预算金额必须为正数",
        ));
    }
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![category_id],
            |r| r.get(0),
        )
        .optional()?;
    match kind.as_deref() {
        None => Err(AppError::codedp_not_found(
            "budget.category-not-found",
            format!("分类不存在: {category_id}"),
            &[category_id],
        )),
        Some("expense") => Ok(()),
        Some(other) => Err(AppError::codedp(
            "budget.category-not-expense",
            format!("预算只能设置在支出分类上（该分类为{other}分类）"),
            &[other],
        )),
    }
}

/// 预算写入校验与落库核心（issue #183）：命令外壳与测试共用。三条底线——
/// 1) 金额必须为正数；2) 只能挂支出分类（金额与分类校验经 [`validate_amount_and_category`]）；
/// 3) 同「分类 + 周期」已有未删除预算时明确拒绝（中文提示并引导编辑），不静默覆盖。
pub fn create_budget(conn: &Connection, input: &BudgetInput) -> Result<String> {
    ensure_transaction(conn, || {
        let period = input.period.unwrap_or(BudgetPeriod::Monthly);
        let id = write_create(
            conn,
            &new_uuid(),
            &input.category_id,
            period,
            input.amount_cents,
            &input.start_date,
        )?;
        record_local(
            conn,
            BudgetCommand::Create {
                id: id.clone(),
                category_id: input.category_id.clone(),
                period,
                amount_cents: input.amount_cents,
                start_date: input.start_date.clone(),
            },
        )?;
        Ok(id)
    })
}

/// 预算落库协议（本地创建与重放共用，无 op 产出）：金额 / 支出分类 / 「分类 +
/// 周期」唯一校验 + 插入。
fn write_create(
    conn: &Connection,
    id: &str,
    category_id: &str,
    period: BudgetPeriod,
    amount_cents: i64,
    start_date: &str,
) -> Result<String> {
    validate_amount_and_category(conn, category_id, amount_cents)?;
    let duplicate: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM budgets WHERE category_id=?1 AND period=?2 AND is_deleted=0 LIMIT 1",
            rusqlite::params![category_id, period.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if duplicate.is_some() {
        return Err(match period {
            BudgetPeriod::Monthly => AppError::coded(
                "budget.duplicate-monthly",
                "该分类已存在按月预算，可编辑该预算的金额",
            ),
            BudgetPeriod::Yearly => AppError::coded(
                "budget.duplicate-yearly",
                "该分类已存在按年预算，可编辑该预算的金额",
            ),
        });
    }
    let now = now_iso();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,0)",
        rusqlite::params![id, category_id, period.to_string(), amount_cents, start_date, now, now, device_id(conn)?],
    )?;
    Ok(id.to_string())
}

/// 预算编辑核心（issue #184）：仅修改金额，沿用软删除同一套
/// updated_at/version/device_id 更新机制；金额与支出分类校验复用创建侧逻辑。
/// 分类/周期不可改（改法为删旧建新）。
pub fn update_budget(conn: &Connection, id: &str, amount_cents: i64) -> Result<()> {
    ensure_transaction(conn, || {
        write_update(conn, id, amount_cents)?;
        record_local(
            conn,
            BudgetCommand::Update {
                id: id.to_string(),
                amount_cents,
            },
        )
    })
}

/// 预算编辑协议（本地编辑与重放共用，无 op 产出）：存在性检查 + 金额与支出
/// 分类校验 + 落库。
fn write_update(conn: &Connection, id: &str, amount_cents: i64) -> Result<()> {
    let category_id: String = conn
        .query_row(
            "SELECT category_id FROM budgets WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("budget.not-found", format!("预算不存在: {id}"), &[id])
        })?;
    validate_amount_and_category(conn, &category_id, amount_cents)?;
    conn.execute(
        "UPDATE budgets SET amount_cents=?2, updated_at=?3, version=version+1, device_id=?4 WHERE id=?1",
        rusqlite::params![id, amount_cents, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

/// 预算删除核心：软删除 + 审计字段更新，与创建/编辑共用同一套机制
/// （与创建/编辑核心对齐，测试与 e2e 通过同一域接缝调用）。
pub fn delete_budget(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, BudgetCommand::Delete { id: id.to_string() })
    })
}

/// 预算软删协议（本地删除与重放共用，无 op 产出）：与本地删除同语义，id 不存在
/// 时 UPDATE 静默 0 行、仍返回 Ok（幂等友好）。
pub(crate) fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

/// 重放执行：创建（金额 / 分类 / 唯一性校验原样生效，冲突挂起待裁决）。
pub(crate) fn replay_create(
    conn: &Connection,
    id: &str,
    category_id: &str,
    period: BudgetPeriod,
    amount_cents: i64,
    start_date: &str,
) -> Result<()> {
    write_create(conn, id, category_id, period, amount_cents, start_date)?;
    Ok(())
}

/// 重放执行：编辑金额（同一协议含存在性检查与分类校验）。
pub(crate) fn replay_update(conn: &Connection, id: &str, amount_cents: i64) -> Result<()> {
    write_update(conn, id, amount_cents)
}
