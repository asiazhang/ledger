use rusqlite::Connection;

use crate::model::{ROW_COLUMNS, Transaction, TransactionListFilter, TransactionListResult};
use crate::read::funding::attach_fundings;
use crate::read::purchase::attach_purchases;
use crate::read::source::{attach_convert_fields, attach_sources};
use ledger_infra::db::query::{query_all, query_one};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};

pub use get_transaction_internal as get_transaction;
pub use list_transactions_internal as list_transactions;

pub fn list_transactions_internal(
    conn: &Connection,
    filter: &TransactionListFilter,
) -> Result<TransactionListResult> {
    ensure_transaction(conn, || {
        let mut where_clause = String::from("WHERE is_deleted=0");
        let mut params: Vec<String> = Vec::new();
        if let Some(from) = filter.from.as_deref() {
            where_clause.push_str(" AND date >= ?");
            params.push(from.to_string());
        }
        if let Some(to) = filter.to.as_deref() {
            where_clause.push_str(" AND date <= ?");
            params.push(to.to_string());
        }
        if let Some(account_id) = filter.account_id.as_deref() {
            where_clause.push_str(" AND account_id = ?");
            params.push(account_id.to_string());
        }
        if let Some(account_id) = filter.involving_account_id.as_deref() {
            where_clause.push_str(
                " AND (account_id = ? OR to_account_id = ? OR funding_account_id = ? \
                 OR id IN (SELECT transaction_id FROM transaction_fundings WHERE account_id = ?))",
            );
            params.push(account_id.to_string());
            params.push(account_id.to_string());
            params.push(account_id.to_string());
            params.push(account_id.to_string());
        }
        if let Some(merchant_id) = filter.merchant_id.as_deref() {
            where_clause.push_str(" AND merchant_id = ?");
            params.push(merchant_id.to_string());
        }
        if let Some(category_id) = filter.category_id.as_deref() {
            where_clause.push_str(" AND category_id = ?");
            params.push(category_id.to_string());
        }
        if filter.uncategorized_only == Some(true) {
            where_clause.push_str(" AND category_id IS NULL");
        }
        if let Some(instrument_id) = filter.instrument_id.as_deref() {
            where_clause.push_str(
                " AND id IN (SELECT transaction_id FROM security_transactions \
             WHERE instrument_id = ? OR to_instrument_id = ?)",
            );
            params.push(instrument_id.to_string());
            params.push(instrument_id.to_string());
        }
        if let Some(kinds) = filter.kinds.as_ref().filter(|k| !k.is_empty()) {
            let placeholders = vec!["?"; kinds.len()].join(",");
            where_clause.push_str(&format!(" AND kind IN ({placeholders})"));
            kinds
                .iter()
                .for_each(|k| params.push(k.as_str().to_string()));
        }
        if filter.hide_investment_related == Some(true) {
            where_clause.push_str(
                " AND NOT EXISTS (SELECT 1 FROM accounts a WHERE a.type = 'investment' \
                 AND a.id = transactions.account_id) \
                 AND NOT EXISTS (SELECT 1 FROM accounts a WHERE a.type = 'investment' \
                 AND a.id = transactions.to_account_id) \
                 AND NOT EXISTS (SELECT 1 FROM accounts a WHERE a.type = 'investment' \
                 AND a.id = transactions.funding_account_id) \
                 AND NOT EXISTS (SELECT 1 FROM transaction_fundings f \
                 JOIN accounts a ON a.id = f.account_id AND a.type = 'investment' \
                 WHERE f.transaction_id = transactions.id)",
            );
        }

        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM transactions {where_clause}"),
            rusqlite::params_from_iter(params.iter()),
            |r| r.get(0),
        )?;

        let mut sql = format!(
            "SELECT {} FROM transactions {where_clause} ORDER BY date DESC, created_at DESC, id DESC",
            ROW_COLUMNS.join(",")
        );
        if let Some(page_size) = filter.page_size {
            let page_size = i64::try_from(page_size.max(1)).unwrap_or(i64::MAX);
            let page = filter.page.unwrap_or(1).max(1);
            let offset = i64::try_from(page.saturating_sub(1).saturating_mul(page_size as usize))
                .unwrap_or(i64::MAX);
            sql.push_str(&format!(" LIMIT {page_size} OFFSET {offset}"));
        } else if let Some(n) = filter.limit {
            sql.push_str(&format!(" LIMIT {n}"));
        }
        let mut items = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
        attach_sources(conn, &mut items)?;
        attach_convert_fields(conn, &mut items)?;
        attach_fundings(conn, &mut items)?;
        attach_purchases(conn, &mut items)?;
        Ok(TransactionListResult { items, total })
    })
}

pub fn get_transaction_internal(conn: &Connection, id: &str) -> Result<Transaction> {
    ensure_transaction(conn, || {
        let mut tx = query_one::<Transaction, _>(
            conn,
            &format!(
                "SELECT {} FROM transactions WHERE id=?1 AND is_deleted=0",
                ROW_COLUMNS.join(",")
            ),
            rusqlite::params![id],
        )?
        .ok_or_else(|| {
            AppError::codedp_not_found("transaction.not-found", format!("交易不存在: {id}"), &[id])
        })?;
        attach_fundings(conn, std::slice::from_mut(&mut tx))?;
        attach_purchases(conn, std::slice::from_mut(&mut tx))?;
        Ok(tx)
    })
}
