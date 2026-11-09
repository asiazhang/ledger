//! 购买项明细（写路径，issue #1882 / ADR-0138 决策 9/10）：expense 购买项子行的
//! 契约校验与落库。
//!
//! 职责：[`validate_purchases`]（kind 准入 + 逐条约束：名称非空、件数正整数、
//! 单价非负、分类准入）与子行落库三件（[`insert_rows`] / [`replace_rows`] /
//! [`read_rows`]）。与出资项（[`super::funding_items`]）是两个不同源概念：购买项
//! 不承载资金语义（无账户、无金额、不进余额与折算口径），只有展示明细。ADR 指针：
//! ADR-0138 决策 9/10/15。陷阱：子行随主行存亡——软删不改子行，读侧按主行
//! `is_deleted` 过滤；修改为全量替换；仅 `expense` 可带（`refund` 不另挂清单）。
//! kind 准入辖全部 Local 形态（writer::normalize 是本地创建/修改/批量/定时引擎的
//! 单点）；重放形态信任源端归一化行、不重复 kind 准入（与出资项同款，ADR-0091
//! 信任源端——购买项无资金语义，伪造载荷不产生余额/报表漂移，仅脏明细可经重导
//! 覆盖）。

use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::params;

use ledger_infra::error::{AppError, Result};

use crate::amount::TransactionKind;

/// 归一化后的购买项（写路径自有类型，与模型层 `TransactionPurchaseInput` 解耦，
/// 先例 [`super::funding_items::FundingItem`]）。数组顺序即落库顺序位（读回顺序是
/// 用户可见语义——对账单顺序）。
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

/// 校验购买项契约（创建与修改共用；不落库、只判定）。
///
/// - kind 准入（ADR-0138 决策 9）：仅 `expense` 可带；`refund` 不另挂清单，
///   `transfer` / 投资 kind / `dividend` / `split` / `convert` 携带即拒绝；
/// - 逐条约束（决策 9/10）：名称非空；件数为正整数；单价可空、携带时非负
///   （0 是合法标价——赠品；负数不是）；
/// - 分类准入（可选字段携带时）：存在且未软删（码化 NotFound，AI 可读回自纠）；
///   「保持历史引用」（修改路径，与出资项 `existing_funding` 同款语义）：提交
///   明细与该行当前明细逐项相同时跳过逐条分类准入——历史购买项的分类此后软删
///   不阻止编辑其他字段。
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

/// 单条购买项的分类准入（存在、未软删）：码化 NotFound（与商户/保单同款，
/// AI 可读回自纠）。不做分类 kind 判定——交易行分类同样无 kind 交叉校验，
/// 明细指针只要求指向在用分类。
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

/// 子行落库（创建路径）：数组顺序即顺序位（0 起）。
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

/// 子行全量替换（修改路径，与出资项同款全量语义）：先清后插。调用方必须在
/// 既有写事务内调用（与主行 UPDATE 同事务）。
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

/// 读一行落库的购买项（顺序位序）：修改路径「保持历史引用」的比对基准。
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
