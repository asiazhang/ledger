use rusqlite::Connection;
use rusqlite::params;

use ledger_infra::error::{AppError, Result};

use crate::amount::TransactionKind;
use crate::seams::funding::lookup_funding_account;

pub const LABEL_MAX_CHARS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingItem {
    pub account_id: String,
    pub amount_cents: i64,
    pub label: Option<String>,
}

impl From<&crate::model::TransactionFundingInput> for FundingItem {
    fn from(i: &crate::model::TransactionFundingInput) -> Self {
        Self {
            account_id: i.account_id.clone(),
            amount_cents: i.amount_cents,
            label: i.label.clone(),
        }
    }
}

impl From<&FundingItem> for crate::model::TransactionFundingInput {
    fn from(i: &FundingItem) -> Self {
        Self {
            account_id: i.account_id.clone(),
            amount_cents: i.amount_cents,
            label: i.label.clone(),
        }
    }
}

pub struct FundingContract<'a> {
    pub kind: TransactionKind,
    pub account_id: Option<&'a str>,
    pub amount_cents: i64,
    pub currency: &'a str,
    pub funding: &'a [FundingItem],
    pub existing_funding: &'a [FundingItem],
    pub currency_unchanged: bool,
}
pub fn validate_funding_items(conn: &Connection, contract: FundingContract<'_>) -> Result<()> {
    let FundingContract {
        kind,
        account_id,
        amount_cents,
        currency,
        funding,
        existing_funding,
        currency_unchanged,
    } = contract;
    if funding.is_empty() {
        return Ok(());
    }
    if !matches!(
        kind,
        TransactionKind::Income | TransactionKind::Expense | TransactionKind::Refund
    ) {
        return Err(AppError::codedp(
            "transaction.funding-item-unsupported",
            format!("交易类型 {kind} 不能携带出资分解"),
            &[&kind.to_string()],
        ));
    }
    if account_id.is_some() {
        return Err(AppError::coded(
            "transaction.funding-account-conflict",
            "携带出资分解时不可指定 account_id",
        ));
    }
    let unchanged = currency_unchanged && *funding == *existing_funding;
    let mut total: i64 = 0;
    for item in funding {
        if item.amount_cents <= 0 {
            return Err(AppError::coded(
                "transaction.funding-amount-positive",
                "出资项金额必须大于 0",
            ));
        }
        if item
            .label
            .as_deref()
            .map(str::chars)
            .map(Iterator::count)
            .unwrap_or(0)
            > LABEL_MAX_CHARS
        {
            return Err(AppError::coded(
                "transaction.funding-label-too-long",
                "扣款标签长度不能超过 50 字",
            ));
        }
        if !unchanged {
            validate_item_account(conn, &item.account_id, currency)?;
        }
        total += item.amount_cents;
    }
    if total != amount_cents {
        return Err(AppError::codedp(
            "transaction.funding-sum-mismatch",
            format!("出资分解合计（{total}）与交易金额（{amount_cents}）不一致"),
            &[&total.to_string(), &amount_cents.to_string()],
        ));
    }
    Ok(())
}

fn validate_item_account(conn: &Connection, account_id: &str, currency: &str) -> Result<()> {
    let view = lookup_funding_account(conn, account_id)?.ok_or_else(|| {
        AppError::codedp_not_found(
            "funding.account-not-found",
            format!("出资账户不存在或已删除: {account_id}"),
            &[account_id],
        )
    })?;
    if view.currency_code != currency {
        return Err(AppError::codedp(
            "funding.currency-mismatch",
            format!(
                "出资账户币种（{}）与交易币种（{}）不一致",
                view.currency_code, currency
            ),
            &[&view.currency_code, currency],
        ));
    }
    Ok(())
}

pub fn validate_funding_item_currencies(
    conn: &Connection,
    funding: &[FundingItem],
    currency_code: &str,
) -> Result<()> {
    for item in funding {
        validate_item_account(conn, &item.account_id, currency_code)?;
    }
    Ok(())
}

pub fn insert_rows(conn: &Connection, transaction_id: &str, funding: &[FundingItem]) -> Result<()> {
    for (sort, item) in funding.iter().enumerate() {
        conn.execute(
            "INSERT INTO transaction_fundings \
             (transaction_id,sort,account_id,amount_cents,label) VALUES (?1,?2,?3,?4,?5)",
            params![
                transaction_id,
                sort as i64,
                item.account_id,
                item.amount_cents,
                item.label,
            ],
        )?;
    }
    Ok(())
}

pub fn replace_rows(
    conn: &Connection,
    transaction_id: &str,
    funding: &[FundingItem],
) -> Result<()> {
    conn.execute(
        "DELETE FROM transaction_fundings WHERE transaction_id=?1",
        params![transaction_id],
    )?;
    insert_rows(conn, transaction_id, funding)
}

pub fn item_account_ids(conn: &Connection, transaction_id: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT account_id FROM transaction_fundings \
         WHERE transaction_id=?1 ORDER BY sort",
    )?;
    let ids = stmt
        .query_map(params![transaction_id], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(ids)
}

pub fn read_rows(conn: &Connection, transaction_id: &str) -> Result<Vec<FundingItem>> {
    let mut stmt = conn.prepare(
        "SELECT account_id, amount_cents, label FROM transaction_fundings \
         WHERE transaction_id=?1 ORDER BY sort",
    )?;
    let items = stmt
        .query_map(params![transaction_id], |r| {
            Ok(FundingItem {
                account_id: r.get(0)?,
                amount_cents: r.get(1)?,
                label: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(items)
}
