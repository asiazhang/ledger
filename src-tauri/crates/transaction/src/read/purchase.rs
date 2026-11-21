use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::params_from_iter;

use ledger_infra::error::Result;

use crate::model::{Transaction, TransactionPurchase};

pub fn attach_purchases(conn: &Connection, items: &mut [Transaction]) -> Result<()> {
    let ids: Vec<&str> = items.iter().map(|t| t.id.as_str()).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!(
        "SELECT transaction_id, name, quantity, category_id, unit_price_cents \
         FROM transaction_purchases \
         WHERE transaction_id IN ({placeholders}) \
         ORDER BY transaction_id, sort"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut by_tx: HashMap<String, Vec<TransactionPurchase>> = HashMap::new();
    let mut rows = stmt.query(params_from_iter(ids.iter()))?;
    while let Some(r) = rows.next()? {
        let tx_id: String = r.get(0)?;
        by_tx.entry(tx_id).or_default().push(TransactionPurchase {
            name: r.get(1)?,
            quantity: r.get(2)?,
            category_id: r.get(3)?,
            unit_price_cents: r.get(4)?,
        });
    }
    for t in items.iter_mut() {
        if let Some(purchases) = by_tx.remove(&t.id) {
            t.purchases = purchases;
        }
    }
    Ok(())
}
