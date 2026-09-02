//! 物品溯源守卫（ADR-0025 创建唯一入口的准入接缝，issue #207 / #119）：关联购买
//! 交易的解析、校验与自动带出。
//!
//! - [`apply_purchase_link`]：入参带交易 id → 校验并用交易值覆盖入参的
//!   购买日期/总成本/币种（自动带出）；不带关联时原样返回。
//!   创建（溯源必填）与换关（溯源只增不减）两条路径共用。
//! - [`resolve_purchase_link`]：查验交易存在（未删除）且为 `expense`、
//!   未被其他未删除物品关联（溯源唯一）。
//!
//! 守卫只做准入判定与带出，不落库；调用方（`super::domain`）继续走统一校验后写入。

use rusqlite::{Connection, OptionalExtension};

use crate::error::{AppError, Result};
use crate::models::ItemInput;
use crate::transaction::amount::TransactionKind;

/// 解析关联购买交易并自动带出（issue #119）：入参带交易 id 时校验交易
/// 存在、未删除且为 `expense`，用交易值覆盖入参的购买日期/总成本/币种
/// （自动带出）；不带关联时原样返回。返回有效入参，调用方继续走统一校验。
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

/// 查验关联购买交易：存在（未删除）且为 `expense`，返回（交易日期，金额分，币种）。不存在/已删除 → 参数错误；非 expense → 参数错误；
/// 该交易已被其他未删除物品关联 → 参数错误（溯源唯一，创建与换关两条路径共用本守卫：
/// 同一笔购买只能对应一件物品，避免每天成本被重复计算；软删除物品不占坑，可重新创建）。
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
