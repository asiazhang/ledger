//! 购买项读回（读路径，issue #1882 / ADR-0138 决策 9/10）：带购买项的 expense 的
//! 子行读取与列表/详情共用的 attach 入口。
//!
//! 职责：[`attach_purchases`]（落库子行按顺序位稳定读取并填充
//! `Transaction.purchases`）。不变量：落库明细按 `(transaction_id, sort)` 读，
//! 顺序即写入数组序（对账单顺序）；无购买项的行恒空数组（存量行为零变化）；
//! 购买项无 refund 派生（`refund` 不另挂清单，ADR-0138 决策 9）。陷阱：本模块
//! 只读；调用方保证处于同一读事务（列表读闭包与单笔读回均经 `ensure_transaction`
//! 收口，跨语句口径一致）。

use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::params_from_iter;

use ledger_infra::error::Result;

use crate::model::{Transaction, TransactionPurchase};

/// 落库子行读取（`transaction_id IN 页行集`，按 transaction_id + sort 排序），
/// 填充各行的 `purchases`。
///
/// 一次批量 IN 查询取整页子行（列表读路径不做 N+1）。
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
