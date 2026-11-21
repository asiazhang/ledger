use rusqlite::{Connection, OptionalExtension};

use super::model::ItemInput;
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::TransactionKind;

pub(super) fn link_required_error() -> AppError {
    AppError::coded(
        "item.purchase-link-required",
        "物品必须关联一笔购买交易创建：请在交易页右键一笔支出交易，选择「加入物品」",
    )
}

pub(super) fn apply_purchase_link(conn: &Connection, input: &ItemInput) -> Result<ItemInput> {
    let Some(tx_id) = &input.purchase_transaction_id else {
        return Ok(input.clone());
    };
    let (date, cost_cents, currency) = resolve_purchase_link(conn, tx_id)?;
    Ok(ItemInput {
        purchase_date: date,
        total_cost_cents: cost_cents,
        currency_code: currency,
        ..input.clone()
    })
}

pub(super) fn resolve_purchase_link(
    conn: &Connection,
    tx_id: &str,
) -> Result<(String, i64, String)> {
    let taken: bool = conn
        .query_row(
            "SELECT 1 FROM items WHERE purchase_transaction_id=?1 AND is_deleted=0 LIMIT 1",
            rusqlite::params![tx_id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if taken {
        return Err(AppError::codedp(
            "item.purchase-link-taken",
            format!("该购买交易已创建过物品，不能重复创建（溯源唯一）: {tx_id}"),
            &[tx_id],
        ));
    }
    let row: Option<(String, String, i64, String)> = conn
        .query_row(
            "SELECT kind, date, amount_cents, currency_code FROM transactions \
             WHERE id=?1 AND is_deleted=0",
            rusqlite::params![tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((kind, date, amount_cents, currency)) = row else {
        return Err(AppError::codedp(
            "item.purchase-tx-not-found",
            format!("关联的购买交易不存在: {tx_id}"),
            &[tx_id],
        ));
    };
    if kind != TransactionKind::Expense.as_str() {
        return Err(AppError::codedp(
            "item.purchase-tx-not-expense",
            format!("关联的交易必须是支出类型（实际: {kind}）"),
            &[&kind],
        ));
    }
    Ok((date, amount_cents, currency))
}
