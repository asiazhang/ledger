use rusqlite::Connection;
use tauri::State;

use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Account, AccountBalance, AccountInput};

#[tauri::command]
pub fn list_accounts(db: State<'_, DbState>) -> Result<Vec<Account>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
         FROM accounts WHERE is_deleted=0 ORDER BY created_at",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(Account {
            id: r.get(0)?,
            name: r.get(1)?,
            kind: r.get(2)?,
            currency_code: r.get(3)?,
            initial_balance_cents: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
            version: r.get(7)?,
            device_id: r.get(8)?,
            is_deleted: r.get::<_, i64>(9)? != 0,
        })
    })?;
    rows.collect::<std::result::Result<Vec<_>, _>>()
        .map_err(Into::into)
}

#[tauri::command]
pub fn create_account(db: State<'_, DbState>, input: AccountInput) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO accounts (id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![
            id,
            input.name,
            input.kind,
            input.currency_code,
            input.initial_balance_cents.unwrap_or(0),
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn delete_account(db: State<'_, DbState>, id: String) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

/// 计算账户当前余额 = 初始余额 + 收入 - 支出（转账从转出账户减，加到转入账户）。
fn account_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    let initial: i64 = conn.query_row(
        "SELECT initial_balance_cents FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let income: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='income' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let expense: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='expense' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_in: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE to_account_id=?1 AND kind='transfer' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let transfer_out: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='transfer' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    let refund: Option<i64> = conn
        .query_row(
            "SELECT COALESCE(SUM(amount_native_cents),0) FROM transactions \
             WHERE account_id=?1 AND kind='refund' AND is_deleted=0",
            rusqlite::params![account_id],
            |r| r.get(0),
        )
        .ok();
    Ok(
        initial + income.unwrap_or(0) - expense.unwrap_or(0) + transfer_in.unwrap_or(0)
            - transfer_out.unwrap_or(0)
            + refund.unwrap_or(0),
    )
}

#[tauri::command]
pub fn list_account_balances(db: State<'_, DbState>) -> Result<Vec<AccountBalance>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted \
         FROM accounts WHERE is_deleted=0 ORDER BY created_at",
    )?;
    let accounts: Vec<Account> = stmt
        .query_map([], |r| {
            Ok(Account {
                id: r.get(0)?,
                name: r.get(1)?,
                kind: r.get(2)?,
                currency_code: r.get(3)?,
                initial_balance_cents: r.get(4)?,
                created_at: r.get(5)?,
                updated_at: r.get(6)?,
                version: r.get(7)?,
                device_id: r.get(8)?,
                is_deleted: r.get::<_, i64>(9)? != 0,
            })
        })?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);
    accounts
        .into_iter()
        .map(|a| {
            Ok(AccountBalance {
                balance_cents: account_balance(&conn, &a.id)?,
                account: a,
            })
        })
        .collect()
}
