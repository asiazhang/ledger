use std::collections::HashMap;

use rusqlite::Connection;

use crate::db::query::{FromRow, query_all};
use crate::error::Result;
use crate::transaction::amount::{TransferSide, account_flow_expr};

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

/// 单账户侧的 `account_flow` 聚合 SQL（口径权威：Amount 接缝的 kind→度量矩阵）。
///
/// 语义与 `account_flow_expr` 的文档一致：转出侧按 `t.account_id` 关联、
/// 转入侧按 `t.to_account_id` 关联，分别求和后相加。
/// 各 kind 对余额的符号（income/refund/sell/dividend 为 +，expense/buy 为 −，
/// transfer 双侧 −/+，split 恒 0）由矩阵单一真源决定。
fn account_flow_sum_sql(side: TransferSide) -> String {
    let (expr, join_col) = match side {
        TransferSide::Out => (account_flow_expr("t", side), "t.account_id"),
        TransferSide::In => (account_flow_expr("t", side), "t.to_account_id"),
    };
    format!(
        "SELECT COALESCE(SUM({expr}),0) FROM transactions t \
         WHERE t.is_deleted=0 AND {join_col}=?1"
    )
}

/// 计算账户当前余额 = 初始余额 + Σ account_flow（转出侧） + Σ account_flow（转入侧）。
pub fn compute_balance(conn: &Connection, account_id: &str) -> Result<i64> {
    let initial: i64 = conn.query_row(
        "SELECT initial_balance_cents FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_out: i64 = conn.query_row(
        &account_flow_sum_sql(TransferSide::Out),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    let flow_in: i64 = conn.query_row(
        &account_flow_sum_sql(TransferSide::In),
        rusqlite::params![account_id],
        |r| r.get(0),
    )?;
    Ok(initial + flow_out + flow_in)
}

/// 批量计算所有未删除账户的余额，单条 SQL 查询。
///
/// 原理：初始余额 + 两个 `account_flow` 关联子查询（转出侧/转入侧）
/// 在一条 SQL 内完成汇总，口径与 [`compute_balance`] 完全一致
/// （同一度量片段、同一关联语义），单个与批量结果恒相等。
/// 对 N 个账户保持 O(1) 次数据库往返。
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
                + COALESCE((SELECT SUM({out_expr}) FROM transactions t \
                            WHERE t.is_deleted=0 AND t.account_id=a.id), 0)
                + COALESCE((SELECT SUM({in_expr}) FROM transactions t \
                            WHERE t.is_deleted=0 AND t.to_account_id=a.id), 0)
         FROM accounts a
         WHERE a.is_deleted = 0 {hidden_clause}",
        out_expr = account_flow_expr("t", TransferSide::Out),
        in_expr = account_flow_expr("t", TransferSide::In),
    );
    let entries: Vec<AccountBalanceEntry> = query_all(conn, &sql, [])?;

    Ok(entries
        .into_iter()
        .map(|e| (e.id, e.balance_cents))
        .collect())
}
