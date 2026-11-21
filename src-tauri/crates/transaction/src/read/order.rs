use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::db::query::query_all;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

use crate::model::{OrderAccountContribution, ROW_COLUMNS, TransactionOrderSummary};
use crate::read::funding::attach_fundings;
use crate::read::purchase::attach_purchases;

pub fn get_transaction_order_summary(
    conn: &Connection,
    source_order_no: &str,
) -> Result<TransactionOrderSummary> {
    ensure_transaction(conn, || {
        let mut items = query_all::<crate::model::Transaction, _>(
            conn,
            &format!(
                "SELECT {} FROM transactions WHERE source_order_no=?1 AND is_deleted=0 \
                 ORDER BY date ASC, created_at ASC, id ASC",
                ROW_COLUMNS.join(",")
            ),
            params![source_order_no],
        )?;
        attach_fundings(conn, &mut items)?;
        attach_purchases(conn, &mut items)?;

        let row_count = items.len() as i64;
        let total_amount_cents: i64 = items.iter().map(|t| t.amount_cents).sum();
        let mut currencies = items.iter().map(|t| t.currency_code.as_str());
        let currency_code = match currencies.next() {
            Some(first) => (currencies.all(|c| c == first)).then(|| first.to_string()),
            None => None,
        };

        let mut accounts: Vec<OrderAccountContribution> = Vec::new();
        let index_of = |accounts: &Vec<OrderAccountContribution>, id: &str| {
            accounts.iter().position(|a| a.account_id == id)
        };
        for t in items.iter() {
            if !t.fundings.is_empty() {
                for f in t.fundings.iter() {
                    match index_of(&accounts, &f.account_id) {
                        Some(i) => accounts[i].amount_cents += f.amount_cents,
                        None => accounts.push(OrderAccountContribution {
                            account_id: f.account_id.clone(),
                            amount_cents: f.amount_cents,
                        }),
                    }
                }
            } else if let Some(account_id) = t.account_id.as_deref() {
                match index_of(&accounts, account_id) {
                    Some(i) => accounts[i].amount_cents += t.amount_cents,
                    None => accounts.push(OrderAccountContribution {
                        account_id: account_id.to_string(),
                        amount_cents: t.amount_cents,
                    }),
                }
            }
        }

        Ok(TransactionOrderSummary {
            source_order_no: source_order_no.to_string(),
            row_count,
            total_amount_cents,
            currency_code,
            accounts,
            items,
        })
    })
}
