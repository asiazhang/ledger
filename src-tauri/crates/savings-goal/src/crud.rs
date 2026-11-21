use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use rusqlite::Connection;

use rusqlite::OptionalExtension;

use super::command::{SavingsGoalCommand, record_local};
use super::model::{SavingsGoalInput, SavingsGoalStatus, SavingsGoalUpdateInput};

fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(AppError::coded(
            "savings-goal.name-required",
            "目标名称不能为空",
        ));
    }
    Ok(())
}

fn validate_amount(amount_cents: i64) -> Result<()> {
    if amount_cents <= 0 {
        return Err(AppError::coded(
            "savings-goal.target-amount-positive",
            "目标金额必须为正数",
        ));
    }
    Ok(())
}

fn validate_planned_monthly(planned_monthly_cents: Option<i64>) -> Result<()> {
    if let Some(monthly) = planned_monthly_cents
        && monthly <= 0
    {
        return Err(AppError::coded(
            "savings-goal.planned-monthly-positive",
            "计划月存必须为正数",
        ));
    }
    Ok(())
}

fn validate_delete_balance(conn: &Connection, account_id: &str) -> Result<()> {
    let balance = ledger_accounts::balance::cached_balance(conn, account_id)?;
    if balance != 0 {
        return Err(AppError::coded(
            "savings-goal.delete-balance-nonzero",
            "目标账户仍有余额，请先转出后再删除",
        ));
    }
    Ok(())
}

pub fn create_savings_goal(conn: &Connection, input: &SavingsGoalInput) -> Result<String> {
    validate_amount(input.target_amount_cents)?;
    ensure_transaction(conn, || {
        let account_id = ledger_accounts::create_account(
            conn,
            ledger_accounts::AccountInput {
                name: input.name.clone(),
                kind: ledger_accounts::AccountType::Other,
                currency_code: ledger_transaction::amount::default_currency_code(conn)?,
                initial_balance_cents: None,
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )?;
        let id = new_uuid();
        write_create(
            conn,
            &id,
            &account_id,
            &input.name,
            input.target_amount_cents,
            input.deadline.as_deref(),
        )?;
        record_local(
            conn,
            SavingsGoalCommand::Create {
                id: id.clone(),
                account_id,
                name: input.name.clone(),
                target_amount_cents: input.target_amount_cents,
                deadline: input.deadline.clone(),
            },
        )?;
        Ok(id)
    })
}

fn write_create(
    conn: &Connection,
    id: &str,
    account_id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
) -> Result<()> {
    validate_amount(target_amount_cents)?;
    let now = now_iso();
    conn.execute(
        "INSERT INTO goals \
         (id,name,target_amount_cents,deadline,status,planned_monthly_cents,account_id,is_deleted,version,device_id,created_at,updated_at) \
         VALUES (?1,?2,?3,?4,'active',NULL,?5,0,1,?6,?7,?7)",
        rusqlite::params![
            id,
            name,
            target_amount_cents,
            deadline,
            account_id,
            device_id(conn)?,
            now
        ],
    )?;
    Ok(())
}

pub(crate) fn replay_create(
    conn: &Connection,
    id: &str,
    account_id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
) -> Result<()> {
    write_create(conn, id, account_id, name, target_amount_cents, deadline)
}

pub fn update_savings_goal(
    conn: &Connection,
    id: &str,
    input: &SavingsGoalUpdateInput,
) -> Result<()> {
    ensure_transaction(conn, || {
        let (existing_name, bound_account_id): (String, String) = conn
            .query_row(
                "SELECT name,account_id FROM goals WHERE id=?1 AND is_deleted=0",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                AppError::codedp_not_found(
                    "savings-goal.not-found",
                    format!("攒钱目标不存在: {id}"),
                    &[id],
                )
            })?;

        let name = input.name.trim().to_string();
        validate_name(&name)?;
        validate_amount(input.target_amount_cents)?;
        validate_planned_monthly(input.planned_monthly_cents)?;

        write_update(
            conn,
            id,
            &name,
            input.target_amount_cents,
            input.deadline.as_deref(),
            input.planned_monthly_cents,
        )?;

        if name != existing_name {
            ledger_accounts::update_account(
                conn,
                &bound_account_id,
                ledger_accounts::AccountUpdateInput {
                    name: Some(name.clone()),
                    currency_code: None,
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                    is_archived: None,
                },
            )?;
        }

        record_local(
            conn,
            SavingsGoalCommand::Update {
                id: id.to_string(),
                name,
                target_amount_cents: input.target_amount_cents,
                deadline: input.deadline.clone(),
                planned_monthly_cents: input.planned_monthly_cents,
            },
        )?;
        Ok(())
    })
}

fn write_update(
    conn: &Connection,
    id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
    planned_monthly_cents: Option<i64>,
) -> Result<()> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM goals WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AppError::codedp_not_found(
            "savings-goal.not-found",
            format!("攒钱目标不存在: {id}"),
            &[id],
        ));
    }
    validate_name(name)?;
    validate_amount(target_amount_cents)?;
    validate_planned_monthly(planned_monthly_cents)?;

    conn.execute(
        "UPDATE goals SET name=?2,target_amount_cents=?3,deadline=?4,planned_monthly_cents=?5, \
         updated_at=?6,version=version+1,device_id=?7 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            name,
            target_amount_cents,
            deadline,
            planned_monthly_cents,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    Ok(())
}

pub(crate) fn replay_update(
    conn: &Connection,
    id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
    planned_monthly_cents: Option<i64>,
) -> Result<()> {
    write_update(
        conn,
        id,
        name,
        target_amount_cents,
        deadline,
        planned_monthly_cents,
    )
}

fn set_savings_goal_status(conn: &Connection, id: &str, status: SavingsGoalStatus) -> Result<()> {
    ensure_transaction(conn, || {
        write_status(conn, id, status)?;
        record_local(
            conn,
            if status == SavingsGoalStatus::Archived {
                SavingsGoalCommand::Archive { id: id.to_string() }
            } else {
                SavingsGoalCommand::Unarchive { id: id.to_string() }
            },
        )?;
        Ok(())
    })
}

fn write_status(conn: &Connection, id: &str, status: SavingsGoalStatus) -> Result<()> {
    let exists: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM goals WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AppError::codedp_not_found(
            "savings-goal.not-found",
            format!("攒钱目标不存在: {id}"),
            &[id],
        ));
    }
    conn.execute(
        "UPDATE goals SET status=?2,updated_at=?3,version=version+1,device_id=?4 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id, status.to_string(), now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

pub(crate) fn replay_status(conn: &Connection, id: &str, status: SavingsGoalStatus) -> Result<()> {
    write_status(conn, id, status)
}

pub fn archive_savings_goal(conn: &Connection, id: &str) -> Result<()> {
    set_savings_goal_status(conn, id, SavingsGoalStatus::Archived)
}

pub fn unarchive_savings_goal(conn: &Connection, id: &str) -> Result<()> {
    set_savings_goal_status(conn, id, SavingsGoalStatus::Active)
}

pub fn delete_savings_goal(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        let account_id = bound_account_id(conn, id)?;
        validate_delete_balance(conn, &account_id)?;
        write_delete(conn, id)?;
        ledger_accounts::delete_account(conn, &account_id)?;
        record_local(conn, SavingsGoalCommand::Delete { id: id.to_string() })?;
        Ok(())
    })
}

pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    let account_id = bound_account_id(conn, id)?;
    validate_delete_balance(conn, &account_id)?;
    write_delete(conn, id)
}

fn bound_account_id(conn: &Connection, id: &str) -> Result<String> {
    conn.query_row(
        "SELECT account_id FROM goals WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id],
        |row| row.get(0),
    )
    .optional()?
    .ok_or_else(|| {
        AppError::codedp_not_found(
            "savings-goal.not-found",
            format!("攒钱目标不存在: {id}"),
            &[id],
        )
    })
}

fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE goals SET is_deleted=1,updated_at=?2,version=version+1,device_id=?3 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
}

pub fn ensure_account_not_goal_bound(conn: &Connection, account_id: &str) -> Result<()> {
    if account_has_bound_goal(conn, account_id)? {
        return Err(AppError::coded(
            "savings-goal.account-in-use",
            "该账户是攒钱目标专属账户，请先删除对应的攒钱目标",
        ));
    }
    Ok(())
}

fn account_has_bound_goal(conn: &Connection, account_id: &str) -> Result<bool> {
    let bound: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM goals WHERE account_id=?1 AND is_deleted=0 LIMIT 1",
            rusqlite::params![account_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(bound.is_some())
}

pub fn ensure_account_can_archive(conn: &Connection, account_id: &str) -> Result<()> {
    if account_has_bound_goal(conn, account_id)? {
        return Err(AppError::coded(
            "account.goal-bound-cannot-archive",
            "该账户是攒钱目标专属账户，请归档对应的攒钱目标",
        ));
    }
    Ok(())
}
