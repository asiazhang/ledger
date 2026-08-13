use std::collections::HashMap;

use rusqlite::Connection;

use crate::db::query::{FromRow, query_all};
use crate::error::Result;

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

/// 计算账户当前余额 = 初始余额 + 收入 - 支出 + 转入 - 转出 + 退款。
///
/// 转账（kind='transfer'）用 account_id 表示转出、to_account_id 表示转入。
pub fn compute_balance(conn: &Connection, account_id: &str) -> Result<i64> {
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

/// 批量计算所有未删除账户的余额，单条 SQL 查询。
///
/// 原理：一次 LEFT JOIN + CASE WHEN 聚合所有维度的金额，
/// 按 `a.id` 分组，初始余额与五类交易在一条 SQL 内完成汇总。
/// 对 N 个账户，从 O(6N) 次数据库往返降为 O(1)。
/// UI 侧不包含黑洞账户；AI 对账需要 `include_hidden = true`。
pub fn compute_all_balances(conn: &Connection) -> Result<HashMap<String, i64>> {
    compute_all_balances_with_visibility(conn, false)
}

/// 批量计算未删除账户余额；`include_hidden` 为 true 时含黑洞账户。
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
                + COALESCE(SUM(CASE WHEN t.kind='income'   THEN t.amount_native_cents ELSE 0 END), 0)
                - COALESCE(SUM(CASE WHEN t.kind='expense'  THEN t.amount_native_cents ELSE 0 END), 0)
                + COALESCE(SUM(CASE WHEN t.kind='transfer' AND t.to_account_id = a.id THEN t.amount_native_cents ELSE 0 END), 0)
                - COALESCE(SUM(CASE WHEN t.kind='transfer' AND t.account_id  = a.id THEN t.amount_native_cents ELSE 0 END), 0)
                + COALESCE(SUM(CASE WHEN t.kind='refund'   THEN t.amount_native_cents ELSE 0 END), 0)
         FROM accounts a
         LEFT JOIN transactions t ON (t.account_id = a.id OR t.to_account_id = a.id) AND t.is_deleted = 0
         WHERE a.is_deleted = 0 {hidden_clause}
         GROUP BY a.id"
    );
    let entries: Vec<AccountBalanceEntry> = query_all(conn, &sql, [])?;

    Ok(entries
        .into_iter()
        .map(|e| (e.id, e.balance_cents))
        .collect())
}
