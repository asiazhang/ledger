use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::params;

use ledger_infra::error::{AppError, Result};

use crate::amount::TransactionKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseItem {
    pub name: String,
    pub quantity: i64,
    pub category_id: Option<String>,
    pub unit_price_cents: Option<i64>,
}

impl From<&crate::model::TransactionPurchaseInput> for PurchaseItem {
    fn from(i: &crate::model::TransactionPurchaseInput) -> Self {
        Self {
            name: i.name.clone(),
            quantity: i.quantity,
            category_id: i.category_id.clone(),
            unit_price_cents: i.unit_price_cents,
        }
    }
}

impl From<&PurchaseItem> for crate::model::TransactionPurchaseInput {
    fn from(i: &PurchaseItem) -> Self {
        Self {
            name: i.name.clone(),
            quantity: i.quantity,
            category_id: i.category_id.clone(),
            unit_price_cents: i.unit_price_cents,
        }
    }
}

pub struct PurchaseContract<'a> {
    pub kind: TransactionKind,
    pub purchases: &'a [PurchaseItem],
    pub existing_purchases: &'a [PurchaseItem],
}
pub fn validate_purchases(conn: &Connection, contract: PurchaseContract<'_>) -> Result<()> {
    let PurchaseContract {
        kind,
        purchases,
        existing_purchases,
    } = contract;
    if purchases.is_empty() {
        return Ok(());
    }
    if kind != TransactionKind::Expense {
        return Err(AppError::codedp(
            "transaction.purchase-item-unsupported",
            format!("交易类型 {kind} 不能携带购买项"),
            &[&kind.to_string()],
        ));
    }
    let unchanged = *purchases == *existing_purchases;
    for item in purchases {
        if item.name.is_empty() {
            return Err(AppError::coded(
                "transaction.purchase-name-required",
                "购买项名称不能为空",
            ));
        }
        if item.quantity <= 0 {
            return Err(AppError::coded(
                "transaction.purchase-quantity-positive",
                "购买项件数必须大于 0",
            ));
        }
        if item.unit_price_cents.is_some_and(|p| p < 0) {
            return Err(AppError::coded(
                "transaction.purchase-price-negative",
                "购买项单价不能为负数",
            ));
        }
        if !unchanged && let Some(category_id) = item.category_id.as_deref() {
            validate_item_category(conn, category_id)?;
        }
    }
    Ok(())
}

fn validate_item_category(conn: &Connection, category_id: &str) -> Result<()> {
    let active: bool = conn
        .query_row(
            "SELECT 1 FROM categories WHERE id=?1 AND is_deleted=0",
            params![category_id],
            |_| Ok(true),
        )
        .optional()?
        .is_some();
    if !active {
        return Err(AppError::codedp_not_found(
            "purchase.category-not-found",
            format!("购买项分类不存在或已删除: {category_id}"),
            &[category_id],
        ));
    }
    Ok(())
}

pub fn insert_rows(
    conn: &Connection,
    transaction_id: &str,
    purchases: &[PurchaseItem],
) -> Result<()> {
    for (sort, item) in purchases.iter().enumerate() {
        conn.execute(
            "INSERT INTO transaction_purchases \
             (transaction_id,sort,name,quantity,category_id,unit_price_cents) \
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                transaction_id,
                sort as i64,
                item.name,
                item.quantity,
                item.category_id,
                item.unit_price_cents,
            ],
        )?;
    }
    Ok(())
}

pub fn replace_rows(
    conn: &Connection,
    transaction_id: &str,
    purchases: &[PurchaseItem],
) -> Result<()> {
    conn.execute(
        "DELETE FROM transaction_purchases WHERE transaction_id=?1",
        params![transaction_id],
    )?;
    insert_rows(conn, transaction_id, purchases)
}

pub fn read_rows(conn: &Connection, transaction_id: &str) -> Result<Vec<PurchaseItem>> {
    let mut stmt = conn.prepare(
        "SELECT name, quantity, category_id, unit_price_cents FROM transaction_purchases \
         WHERE transaction_id=?1 ORDER BY sort",
    )?;
    let items = stmt
        .query_map(params![transaction_id], |r| {
            Ok(PurchaseItem {
                name: r.get(0)?,
                quantity: r.get(1)?,
                category_id: r.get(2)?,
                unit_price_cents: r.get(3)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}
