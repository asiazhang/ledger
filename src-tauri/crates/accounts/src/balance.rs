//! ```compile_fail
//! use tauri_app_lib::ledger_accounts::balance::affected_accounts;
//! ```

use std::collections::HashMap;

use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::{TransferSide, account_flow_expr, funding_item_flow_expr};
use ledger_transaction::seams::balance::RowAccounts;
use rusqlite::{Connection, OptionalExtension};

use super::model::{Account, AccountBalance};

struct AccountBalanceEntry {
    id: String,
    balance_cents: i64,
}

impl FromRow for AccountBalanceEntry {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(AccountBalanceEntry {
            id: row.get(0)?,
            balance_cents: row.get(1)?,
        })
    }
}

fn join_column(side: TransferSide) -> &'static str {
    match side {
        TransferSide::Out => "t.account_id",
        TransferSide::In => "t.to_account_id",
        TransferSide::Funding => "t.funding_account_id",
    }
}

fn funding_item_flow_subquery(account_ref: &str) -> String {
    format!(
        "(SELECT COALESCE(SUM({expr}),0) FROM transaction_fundings f \
         JOIN transactions t ON t.id = f.transaction_id \
         WHERE t.is_deleted=0 AND f.account_id={account_ref})",
        expr = funding_item_flow_expr("f", "t"),
    )
}

fn account_flow_subquery(side: TransferSide, account_ref: &str) -> String {
    format!(
        "(SELECT COALESCE(SUM({expr}),0) FROM transactions t \
         WHERE t.is_deleted=0 AND {col}={account_ref})",
        expr = account_flow_expr("t", side),
        col = join_column(side),
    )
}

pub fn compute_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    let initial: i64 = conn.query_row(
        "SELECT initial_balance_cents FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_out: i64 = conn.query_row(
        &format!("SELECT {}", account_flow_subquery(TransferSide::Out, "?1")),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_in: i64 = conn.query_row(
        &format!("SELECT {}", account_flow_subquery(TransferSide::In, "?1")),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_funding: i64 = conn.query_row(
        &format!(
            "SELECT {}",
            account_flow_subquery(TransferSide::Funding, "?1")
        ),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_funding_items: i64 = conn.query_row(
        &format!("SELECT {}", funding_item_flow_subquery("?1")),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    Ok(initial + flow_out + flow_in + flow_funding + flow_funding_items)
}

pub fn compute_all_balances(conn: &Connection) -> Result<HashMap<String, i64>> {
    compute_all_balances_with_visibility(conn, false)
}

pub fn compute_all_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<HashMap<String, i64>> {
    let hidden_clause = if include_hidden {
        ""
    } else {
        "AND a.is_hidden = 0"
    };
    let sql = format!(
        "SELECT a.id,
                a.initial_balance_cents
                + COALESCE({out}, 0)
                + COALESCE({tin}, 0)
                + COALESCE({funding}, 0)
                + COALESCE({funding_items}, 0)
         FROM accounts a
         WHERE a.is_deleted = 0 {hidden_clause}",
        out = account_flow_subquery(TransferSide::Out, "a.id"),
        tin = account_flow_subquery(TransferSide::In, "a.id"),
        funding = account_flow_subquery(TransferSide::Funding, "a.id"),
        funding_items = funding_item_flow_subquery("a.id"),
    );
    let entries: Vec<AccountBalanceEntry> = query_all(conn, &sql, [])?;

    Ok(entries
        .into_iter()
        .map(|e| (e.id, e.balance_cents))
        .collect())
}

pub fn list_accounts_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<Account>> {
    let where_clause = if include_hidden {
        "is_deleted=0"
    } else {
        "is_deleted=0 AND is_hidden=0"
    };
    query_all(
        conn,
        &format!(
            "SELECT id,name,type,currency_code,initial_balance_cents,created_at,updated_at,version,device_id,is_deleted,is_hidden,is_archived,credit_limit_cents,statement_day,due_day \
             FROM accounts WHERE {where_clause} ORDER BY created_at"
        ),
        [],
    )
}

pub fn list_account_balances_with_visibility(
    conn: &Connection,
    include_hidden: bool,
) -> Result<Vec<AccountBalance>> {
    let accounts = list_accounts_with_visibility(conn, include_hidden)?;
    let balances = cached_all_balances(conn)?;
    accounts
        .into_iter()
        .map(|a| {
            let balance_cents = balances.get(&a.id).copied().ok_or_else(|| {
                AppError::codedp(
                    "balance.cache-row-missing",
                    format!("账户 {} 缺少余额缓存行，请执行余额缓存审计修复", a.name),
                    &[a.id.as_str()],
                )
            })?;
            Ok(AccountBalance {
                balance_cents,
                account: a,
            })
        })
        .collect()
}

fn affected_accounts<'a>(
    old: Option<RowAccounts<'a>>,
    new: Option<RowAccounts<'a>>,
) -> Vec<&'a str> {
    let mut affected: Vec<&'a str> = Vec::new();
    for row in [old, new].into_iter().flatten() {
        for reference in [row.account_id, row.to_account_id, row.funding_account_id]
            .into_iter()
            .flatten()
            .chain(row.funding_items.iter().copied())
        {
            if !affected.contains(&reference) {
                affected.push(reference);
            }
        }
    }
    affected
}

fn now_iso_millis() -> String {
    chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string()
}

fn balance_refresh_hook(
    conn: &Connection,
    old: Option<RowAccounts<'_>>,
    new: Option<RowAccounts<'_>>,
) -> ledger_infra::error::Result<()> {
    let affected = affected_accounts(old, new);
    refresh_account_balances(conn, &affected)
}

pub fn install_balance_refresh_hook() {
    ledger_transaction::seams::balance::register_balance_refresh_hook(balance_refresh_hook);
}

pub fn refresh_account_balances(conn: &Connection, account_ids: &[&str]) -> Result<()> {
    if account_ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; account_ids.len()].join(",");
    let sql = refresh_upsert_sql(&format!("WHERE a.id IN ({placeholders})"));
    let now = now_iso_millis();
    let params: Vec<&str> = std::iter::once(now.as_str())
        .chain(account_ids.iter().copied())
        .collect();
    conn.execute(&sql, rusqlite::params_from_iter(params))?;
    Ok(())
}

pub fn refresh_all_account_balances(conn: &Connection) -> Result<()> {
    let sql = refresh_upsert_sql("WHERE true");
    conn.execute(&sql, rusqlite::params![now_iso_millis()])?;
    Ok(())
}

fn refresh_upsert_sql(account_filter: &str) -> String {
    format!(
        "INSERT INTO account_balance_cache (account_id, balance_cents, updated_at) \
         SELECT a.id, \
                a.initial_balance_cents + COALESCE({out}, 0) + COALESCE({tin}, 0) \
                 + COALESCE({funding}, 0) + COALESCE({funding_items}, 0), \
                ? \
         FROM accounts a {account_filter} \
         ON CONFLICT(account_id) DO UPDATE SET \
             balance_cents = excluded.balance_cents, \
             updated_at = excluded.updated_at",
        out = account_flow_subquery(TransferSide::Out, "a.id"),
        tin = account_flow_subquery(TransferSide::In, "a.id"),
        funding = account_flow_subquery(TransferSide::Funding, "a.id"),
        funding_items = funding_item_flow_subquery("a.id"),
    )
}

pub fn cached_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    cached_balance_optional(conn, account_id)?.ok_or_else(|| {
        AppError::codedp(
            "balance.cache-row-missing",
            "账户缺少余额缓存行，请执行余额缓存审计修复",
            &[account_id],
        )
    })
}

pub fn cached_balance_optional(conn: &Connection, account_id: &str) -> Result<Option<i64>> {
    conn.query_row(
        "SELECT balance_cents FROM account_balance_cache WHERE account_id = ?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .optional()
    .map_err(Into::into)
}

fn cached_all_balances(conn: &Connection) -> Result<HashMap<String, i64>> {
    let entries: Vec<AccountBalanceEntry> = query_all(
        conn,
        "SELECT account_id, balance_cents FROM account_balance_cache",
        [],
    )?;
    Ok(entries
        .into_iter()
        .map(|e| (e.id, e.balance_cents))
        .collect())
}

#[cfg(test)]
mod tests;
