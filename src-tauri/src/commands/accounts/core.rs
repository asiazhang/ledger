use rusqlite::{Connection, OptionalExtension};

use crate::db::query::query_all;
use crate::db::{device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Account, AccountBalance, AccountInput};

fn list_accounts_with_visibility(conn: &Connection, include_hidden: bool) -> Result<Vec<Account>> {
    let where_clause = if include_hidden {
        "is_deleted=0"
    } else {
        "is_deleted=0 AND is_hidden=0"
    };
    query_all(
        conn,
        &format!(
            "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden \
             FROM accounts WHERE {where_clause} ORDER BY created_at"
        ),
        [],
    )
}

pub fn list_accounts_internal(conn: &Connection) -> Result<Vec<Account>> {
    list_accounts_with_visibility(conn, false)
}

/// AI 侧完整账户列表：不过滤 `is_hidden`，返回含 `is_hidden` 字段的完整列表。
pub fn list_accounts_for_api_internal(conn: &Connection) -> Result<Vec<Account>> {
    list_accounts_with_visibility(conn, true)
}

pub fn create_account_internal(conn: &Connection, input: AccountInput) -> Result<String> {
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

/// 按自然键（name + type + currency_code）幂等创建账户：已存在（未删除）时返回已有 id，
/// 不重复插入、不报错。供 HTTP 导入 API 使用。
pub fn create_account_idempotent_internal(
    conn: &Connection,
    input: AccountInput,
) -> Result<String> {
    if let Some(id) = find_account_by_natural_key(conn, &input)? {
        return Ok(id);
    }
    create_account_internal(conn, input)
}

fn find_account_by_natural_key(conn: &Connection, input: &AccountInput) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM accounts \
         WHERE name=?1 AND type=?2 AND currency_code=?3 AND is_deleted=0 LIMIT 1",
    )?;
    let mut rows = stmt.query(rusqlite::params![
        input.name,
        input.kind,
        input.currency_code
    ])?;
    match rows.next()? {
        Some(row) => Ok(Some(row.get(0)?)),
        None => Ok(None),
    }
}

/// 软删除账户（`is_deleted=1`）。不校验引用（与 UI 行为一致：删除有交易的账户后
/// 历史交易仍保留）。不存在的 id 返回 `AppError::NotFound`（HTTP 侧映射 404）。
/// IPC 与 HTTP 端点共用本函数。
pub fn delete_account_internal(conn: &Connection, id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM accounts WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::NotFound(format!("账户不存在: {id}")));
    }
    conn.execute(
        "UPDATE accounts SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

pub(super) fn list_account_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<AccountBalance>> {
    let accounts = list_accounts_with_visibility(conn, include_hidden)?;
    let balances = crate::db::balance::compute_all_balances_with_visibility(conn, include_hidden)?;
    Ok(accounts
        .into_iter()
        .map(|a| {
            let balance_cents = balances.get(&a.id).copied().unwrap_or(0);
            AccountBalance {
                balance_cents,
                account: a,
            }
        })
        .collect())
}

/// AI 侧余额清单：含黑洞账户。
pub fn list_account_balances_for_api_internal(conn: &Connection) -> Result<Vec<AccountBalance>> {
    list_account_balances_with_visibility(conn, true)
}
