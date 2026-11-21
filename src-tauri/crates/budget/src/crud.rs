use rusqlite::{Connection, OptionalExtension};

use super::command::{BudgetCommand, record_local};
use super::model::{Budget, BudgetInput, BudgetPeriod};
use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

pub fn list_budgets(conn: &Connection) -> Result<Vec<Budget>> {
    query_all(
        conn,
        "SELECT id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted \
         FROM budgets WHERE is_deleted=0 ORDER BY created_at",
        [],
    )
}

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

fn write_create(
    conn: &Connection,
    id: &str,
    category_id: &str,
    period: BudgetPeriod,
    amount_cents: i64,
    start_date: &str,
) -> Result<String> {
    validate_amount_and_category(conn, category_id, amount_cents)?;
    reject_duplicate_period(conn, category_id, period, None)?;
    let now = now_iso();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,1,?8,0)",
        rusqlite::params![id, category_id, period.to_string(), amount_cents, start_date, now, now, device_id(conn)?],
    )?;
    Ok(id.to_string())
}

pub fn update_budget(
    conn: &Connection,
    id: &str,
    amount_cents: i64,
    period: Option<BudgetPeriod>,
) -> Result<()> {
    ensure_transaction(conn, || {
        write_update(conn, id, amount_cents, period)?;
        record_local(
            conn,
            BudgetCommand::Update {
                id: id.to_string(),
                amount_cents,
                period,
            },
        )
    })
}

fn reject_duplicate_period(
    conn: &Connection,
    category_id: &str,
    period: BudgetPeriod,
    exclude_id: Option<&str>,
) -> Result<()> {
    let duplicate: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM budgets WHERE category_id=?1 AND period=?2 AND is_deleted=0 \
             AND (?3 IS NULL OR id != ?3) LIMIT 1",
            rusqlite::params![category_id, period.to_string(), exclude_id],
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
    Ok(())
}

fn write_update(
    conn: &Connection,
    id: &str,
    amount_cents: i64,
    period: Option<BudgetPeriod>,
) -> Result<()> {
    let (category_id, current_period): (String, BudgetPeriod) = conn
        .query_row(
            "SELECT category_id,period FROM budgets WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("budget.not-found", format!("预算不存在: {id}"), &[id])
        })?;
    validate_amount_and_category(conn, &category_id, amount_cents)?;
    let target_period = period.unwrap_or(current_period);
    reject_duplicate_period(conn, &category_id, target_period, Some(id))?;
    conn.execute(
        "UPDATE budgets SET amount_cents=?2, period=?3, updated_at=?4, version=version+1, device_id=?5 WHERE id=?1",
        rusqlite::params![id, amount_cents, target_period.to_string(), now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

pub fn delete_budget(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        write_delete(conn, id)?;
        record_local(conn, BudgetCommand::Delete { id: id.to_string() })
    })
}

pub(crate) fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

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

pub(crate) fn replay_update(
    conn: &Connection,
    id: &str,
    amount_cents: i64,
    period: Option<BudgetPeriod>,
) -> Result<()> {
    write_update(conn, id, amount_cents, period)
}
