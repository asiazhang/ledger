//! 出资项读回（读路径，issue #1860 / ADR-0138 决策 5/6）：分解行的子行读取、
//! refund 缺省派生与列表/详情共用的 attach 入口。
//!
//! 职责：[`attach_fundings`]（落库子行按顺序位稳定读取并填充 `Transaction.fundings`）
//! 与 refund 缺省派生（关联分解交易的退款**读时推导、不落库**——按出资比例换算，
//! 整数分尾差归顺序位最小的出资项）。不变量：落库分解按 `(transaction_id, sort)`
//! 读，顺序即写入顺序；派生条目带 `derived=true`、落库条目恒 `false`。ADR 指针：
//! ADR-0138 决策 5。陷阱：本模块只读；调用方保证处于同一读事务（列表读闭包与
//! 单笔读回均经 `ensure_transaction` 收口，跨语句口径一致）。

use std::collections::HashMap;

use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::error::Result;

use crate::model::{Transaction, TransactionFunding};

/// 落库子行读取（`transaction_id IN 页行集`，按 transaction_id + sort 排序），
/// 填充各行的 `fundings`；refund 缺省派生在填充时按行补算。
///
/// 一次批量 IN 查询取整页子行（列表读路径不做 N+1）；派生只对「refund 行 ∧
/// 主表账户为 NULL ∧ 无落库子行」触发（显式覆盖 refund 有落库子行、不派生）。
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
        // refund 缺省派生（ADR-0138 决策 5）：原支出为分解行时退款无单一账户
        // 可继承（主表 account_id 落 NULL、无落库子行），读时按出资比例推导。
        if t.kind == crate::amount::TransactionKind::Refund
            && t.account_id.is_none()
            && let Some(ref_id) = t.refund_of_transaction_id.as_deref()
        {
            t.fundings = derive_refund_fundings(conn, ref_id, t.amount_cents)?;
        }
    }
    Ok(())
}

/// refund 缺省派生：读原支出（未删除）的落库分解，按比例换算到退款金额。
///
/// 换算取整数分 floor，尾差（`refund_total − Σ floor`，恒非负且小于出资方数）
/// 归顺序位最小的出资项——与分期尾差同一哲学（可预测、不随机，ADR-0138 决策 5）。
/// 原支出无落库分解（单出资）时退回空——该形态 refund 已继承单一账户，不走本路径。
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
        // 尾差归顺序位最小项（首项）；负尾差（换算超过退款额，比例 > 1 不可能
        // 出现，防御性钳制）同样由首项吸收。
        derived[0].amount_cents += remainder;
    }
    Ok(derived)
}
