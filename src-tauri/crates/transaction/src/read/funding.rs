use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::error::Result;

use crate::model::{Transaction, TransactionFunding};

pub fn attach_fundings(conn: &Connection, items: &mut [Transaction]) -> Result<()> {
    let ids: Vec<&str> = items.iter().map(|t| t.id.as_str()).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT transaction_id, account_id, amount_cents, label \
         FROM transaction_fundings \
         WHERE transaction_id IN ({placeholders}) \
         ORDER BY transaction_id, sort"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut by_tx: HashMap<String, Vec<TransactionFunding>> = HashMap::new();
    let mut rows = stmt.query(rusqlite::params_from_iter(ids.iter()))?;
    while let Some(r) = rows.next()? {
        let tx_id: String = r.get(0)?;
        by_tx.entry(tx_id).or_default().push(TransactionFunding {
            account_id: r.get(1)?,
            amount_cents: r.get(2)?,
            label: r.get(3)?,
            derived: false,
        });
    }
    for t in items.iter_mut() {
        if let Some(fundings) = by_tx.remove(&t.id) {
            t.fundings = fundings;
            continue;
        }
        if t.kind == crate::amount::TransactionKind::Refund
            && t.account_id.is_none()
            && let Some(ref_id) = t.refund_of_transaction_id.as_deref()
        {
            t.fundings = derive_refund_fundings(conn, ref_id, t.amount_cents)?;
        }
    }
    Ok(())
}

fn derive_refund_fundings(
    conn: &Connection,
    refund_of_id: &str,
    refund_total: i64,
) -> Result<Vec<TransactionFunding>> {
    let mut stmt = conn.prepare(
        "SELECT f.account_id, f.amount_cents, f.label \
         FROM transaction_fundings f \
         JOIN transactions t ON t.id = f.transaction_id \
         WHERE f.transaction_id=?1 AND t.is_deleted=0 \
         ORDER BY f.sort",
    )?;
    let original: Vec<(String, i64, Option<String>)> = stmt
        .query_map(params![refund_of_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?
        .collect::<std::result::Result<_, _>>()?;
    if original.is_empty() {
        return Ok(Vec::new());
    }
    let original_total: i128 = original.iter().map(|(_, a, _)| *a as i128).sum();
    if original_total <= 0 {
        return Ok(Vec::new());
    }
    let mut derived: Vec<TransactionFunding> = original
        .into_iter()
        .map(|(account_id, amount_cents, label)| {
            let share = (refund_total as i128 * amount_cents as i128 / original_total) as i64;
            TransactionFunding {
                account_id,
                amount_cents: share,
                label,
                derived: true,
            }
        })
        .collect();
    let allocated: i64 = derived.iter().map(|f| f.amount_cents).sum();
    let remainder = refund_total - allocated;
    if remainder != 0 {
        derived[0].amount_cents += remainder;
    }
    Ok(derived)
}
