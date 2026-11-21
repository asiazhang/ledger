use rusqlite::Connection;

use super::model::{
    InvestmentTransactionListFilter, InvestmentTransactionListResult, InvestmentTransactionRow,
};
use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount::TransactionKind;

const LEDGER_TAB_KINDS: [TransactionKind; 5] = [
    TransactionKind::Buy,
    TransactionKind::Sell,
    TransactionKind::Convert,
    TransactionKind::Split,
    TransactionKind::Dividend,
];

const ROW_COLUMNS: &str = "t.id, t.kind, t.amount_cents, t.account_id, t.funding_account_id, \
     t.date, st.instrument_id, i.symbol, i.name, i.instrument_type, \
    st.quantity, st.price_cents, st.fee_cents, \
    st.to_instrument_id, ti.symbol, st.to_quantity, st.out_amount_cents, st.in_amount_cents, \
    t.note, t.currency_code, t.amount_native_cents, t.source_order_no";

pub fn list_investment_transactions(
    conn: &Connection,
    filter: &InvestmentTransactionListFilter,
) -> Result<InvestmentTransactionListResult> {
    ensure_transaction(conn, || {
        let mut where_clause = String::from("WHERE t.is_deleted=0");
        let mut params: Vec<String> = Vec::new();
        let placeholders = vec!["?"; LEDGER_TAB_KINDS.len()].join(",");
        where_clause.push_str(&format!(" AND t.kind IN ({placeholders})"));
        LEDGER_TAB_KINDS
            .iter()
            .for_each(|k| params.push(k.as_str().to_string()));
        if let Some(from) = filter.from.as_deref() {
            where_clause.push_str(" AND t.date >= ?");
            params.push(from.to_string());
        }
        if let Some(to) = filter.to.as_deref() {
            where_clause.push_str(" AND t.date <= ?");
            params.push(to.to_string());
        }
        if let Some(account_id) = filter.account_id.as_deref() {
            where_clause.push_str(" AND (t.account_id = ? OR t.funding_account_id = ?)");
            params.push(account_id.to_string());
            params.push(account_id.to_string());
        }
        if let Some(instrument_id) = filter.instrument_id.as_deref() {
            where_clause.push_str(" AND (st.instrument_id = ? OR st.to_instrument_id = ?)");
            params.push(instrument_id.to_string());
            params.push(instrument_id.to_string());
        }
        if let Some(kinds) = filter.kinds.as_ref().filter(|k| !k.is_empty()) {
            let placeholders = vec!["?"; kinds.len()].join(",");
            where_clause.push_str(&format!(" AND t.kind IN ({placeholders})"));
            kinds
                .iter()
                .for_each(|k| params.push(k.as_str().to_string()));
        }

        let total: i64 = conn.query_row(
            &format!(
                "SELECT COUNT(*) FROM transactions t \
                 JOIN security_transactions st ON st.transaction_id = t.id \
                 {where_clause}"
            ),
            rusqlite::params_from_iter(params.iter()),
            |r| r.get(0),
        )?;

        let mut sql = format!(
            "SELECT {ROW_COLUMNS} \
             FROM transactions t \
             JOIN security_transactions st ON st.transaction_id = t.id \
             JOIN instruments i ON i.id = st.instrument_id \
             LEFT JOIN instruments ti ON ti.id = st.to_instrument_id \
             {where_clause} ORDER BY t.date DESC, t.created_at DESC, t.id DESC"
        );
        if let Some(page_size) = filter.page_size {
            let page_size = i64::try_from(page_size.max(1)).unwrap_or(i64::MAX);
            let page = i64::try_from(filter.page.unwrap_or(1).max(1)).unwrap_or(i64::MAX);
            let offset = page.saturating_sub(1).saturating_mul(page_size);
            sql.push_str(&format!(" LIMIT {page_size} OFFSET {offset}"));
        }
        let items = query_all::<InvestmentTransactionRow, _>(
            conn,
            &sql,
            rusqlite::params_from_iter(params),
        )?;
        Ok(InvestmentTransactionListResult { items, total })
    })
}
