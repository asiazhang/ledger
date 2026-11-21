use rusqlite::Connection;
use rusqlite::OptionalExtension;
use rusqlite::params;

use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;

use crate::amount::{self, TransactionKind};
use crate::model::NormalizedTransaction;
use crate::seams::balance;
use crate::seams::balance::RowAccounts;
#[derive(Debug, Clone, PartialEq)]
pub struct Input {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    pub funding_account_id: Option<String>,
    pub category_id: Option<String>,
    pub funding: Vec<crate::write::funding_items::FundingItem>,
    pub existing_funding: Vec<crate::write::funding_items::FundingItem>,
    pub purchases: Vec<crate::write::purchase_items::PurchaseItem>,
    pub existing_purchases: Vec<crate::write::purchase_items::PurchaseItem>,
    pub merchant_id: Option<String>,
    pub existing_merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub existing_policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub source_order_no: Option<String>,
    pub date: String,
    pub fx_rate: Option<f64>,
    pub fx_edit_baseline: Option<amount::FxEditBaseline>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedRow {
    pub kind: TransactionKind,
    pub amount_cents: i64,
    pub currency_code: String,
    pub amount_native_cents: i64,
    pub fx_rate_used: Option<f64>,
    pub fx_rate_source: Option<amount::FxRateSource>,
    pub account_id: Option<String>,
    pub to_account_id: Option<String>,
    pub funding_account_id: Option<String>,
    pub funding: Vec<crate::write::funding_items::FundingItem>,
    pub purchases: Vec<crate::write::purchase_items::PurchaseItem>,
    pub category_id: Option<String>,
    pub merchant_id: Option<String>,
    pub policy_id: Option<String>,
    pub refund_of_transaction_id: Option<String>,
    pub note: Option<String>,
    pub source_order_no: Option<String>,
    pub date: String,
}

pub fn normalize(conn: &Connection, input: &Input) -> Result<NormalizedRow> {
    match input.kind {
        TransactionKind::Income
        | TransactionKind::Expense
        | TransactionKind::Transfer
        | TransactionKind::Refund => {}
        TransactionKind::Buy
        | TransactionKind::Sell
        | TransactionKind::Dividend
        | TransactionKind::Split
        | TransactionKind::Convert => {
            return Err(AppError::Invalid(format!(
                "writer::normalize 仅处理通用交易类型（income/expense/transfer/refund），收到: {}",
                input.kind
            )));
        }
    }
    if input.kind == TransactionKind::Transfer && input.to_account_id.is_none() {
        return Err(AppError::coded(
            "transfer.to-account-required",
            "转账必须指定目标账户",
        ));
    }
    if input.amount_cents <= 0 {
        return Err(AppError::coded(
            "transaction.amount-positive",
            "金额必须大于 0",
        ));
    }
    let (category_id, account_id, currency_code, merchant_id, refund_of_id) =
        if input.kind == TransactionKind::Refund {
            let ref_id = input.refund_of_transaction_id.clone().ok_or_else(|| {
                AppError::coded("refund.source-required", "退款必须关联原支出交易")
            })?;
            let (cat, acc, cur, mer, okind): (
                Option<String>,
                Option<String>,
                String,
                Option<String>,
                TransactionKind,
            ) = conn
                .query_row(
                    "SELECT category_id, account_id, currency_code, merchant_id, kind \
                 FROM transactions WHERE id=?1 AND is_deleted=0",
                    params![ref_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .optional()?
                .ok_or_else(|| {
                    AppError::codedp_not_found(
                        "refund.source-not-found",
                        format!("退款关联的原支出交易不存在: {ref_id}"),
                        &[ref_id.as_str()],
                    )
                })?;
            if okind != TransactionKind::Expense {
                return Err(AppError::coded(
                    "refund.source-not-expense",
                    "退款只能关联支出交易",
                ));
            }
            (cat, acc, cur, mer, Some(ref_id))
        } else {
            if let Some(merchant_id) = &input.merchant_id {
                let unchanged = input.existing_merchant_id.as_deref() == Some(merchant_id.as_str());
                if !unchanged {
                    validate_merchant_active(conn, Some(merchant_id))?;
                }
            }
            if let Some(policy_id) = &input.policy_id {
                let unchanged = input.existing_policy_id.as_deref() == Some(policy_id.as_str());
                if !unchanged {
                    validate_policy_active(conn, Some(policy_id))?;
                }
            }
            (
                input.category_id.clone(),
                input.account_id.clone(),
                input.currency_code.clone(),
                input.merchant_id.clone(),
                None,
            )
        };
    let currency_unchanged = input
        .fx_edit_baseline
        .as_ref()
        .map(|b| b.currency_code == currency_code)
        .unwrap_or(true);
    crate::write::funding_items::validate_funding_items(
        conn,
        crate::write::funding_items::FundingContract {
            kind: input.kind,
            account_id: input.account_id.as_deref(),
            amount_cents: input.amount_cents,
            currency: &currency_code,
            funding: &input.funding,
            existing_funding: &input.existing_funding,
            currency_unchanged,
        },
    )?;
    crate::write::purchase_items::validate_purchases(
        conn,
        crate::write::purchase_items::PurchaseContract {
            kind: input.kind,
            purchases: &input.purchases,
            existing_purchases: &input.existing_purchases,
        },
    )?;
    let account_id = if input.funding.is_empty() {
        if account_id.is_none() && input.kind != TransactionKind::Refund {
            return Err(AppError::coded(
                "transaction.account-required",
                "必须指定账户",
            ));
        }
        account_id
    } else {
        None
    };
    validate_currency_consistency(
        conn,
        account_id.as_deref(),
        input.to_account_id.as_deref(),
        &currency_code,
    )?;

    crate::write::funding::validate_funding_account(
        conn,
        input.kind,
        input.funding_account_id.as_deref(),
        &currency_code,
    )?;
    let native = amount::convert_to_native_on_edit(
        conn,
        input.amount_cents,
        &currency_code,
        &input.date,
        input.fx_rate,
        input.fx_edit_baseline.as_ref(),
    )?;
    let policy_id = input.policy_id.clone();
    let to_account_id = if input.kind == TransactionKind::Transfer {
        input.to_account_id.clone()
    } else {
        None
    };
    Ok(NormalizedRow {
        kind: input.kind,
        amount_cents: input.amount_cents,
        currency_code,
        amount_native_cents: native.native_cents,
        fx_rate_used: native.fx_rate_used,
        fx_rate_source: native.fx_rate_source,
        account_id,
        to_account_id,
        funding: input.funding.clone(),
        purchases: input.purchases.clone(),
        funding_account_id: input.funding_account_id.clone(),
        category_id,
        merchant_id,
        policy_id,
        refund_of_transaction_id: refund_of_id,
        note: input.note.clone(),
        source_order_no: input.source_order_no.clone(),
        date: input.date.clone(),
    })
}

pub fn validate_merchant_active(conn: &Connection, merchant_id: Option<&str>) -> Result<()> {
    if let Some(id) = merchant_id {
        let active: bool = conn
            .query_row(
                "SELECT 1 FROM merchants WHERE id=?1 AND is_deleted=0",
                params![id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if !active {
            return Err(AppError::codedp(
                "merchant.not-active",
                format!("商户不存在或已删除: {id}"),
                &[id],
            ));
        }
    }
    Ok(())
}

pub fn validate_policy_active(conn: &Connection, policy_id: Option<&str>) -> Result<()> {
    if let Some(id) = policy_id {
        let active: bool = conn
            .query_row(
                "SELECT 1 FROM policies WHERE id=?1 AND is_deleted=0",
                params![id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if !active {
            return Err(AppError::codedp(
                "policy.not-active",
                format!("保单不存在或已删除: {id}"),
                &[id],
            ));
        }
    }
    Ok(())
}

pub fn validate_accounts_alive(
    conn: &Connection,
    account_id: Option<&str>,
    to_account_id: Option<&str>,
    funding_account_id: Option<&str>,
    funding_item_accounts: &[String],
) -> Result<()> {
    let mut ids: Vec<&str> = vec![account_id, to_account_id, funding_account_id]
        .into_iter()
        .flatten()
        .collect();
    ids.extend(funding_item_accounts.iter().map(String::as_str));
    for id in &ids {
        let alive: bool = conn
            .query_row(
                "SELECT 1 FROM accounts WHERE id=?1 AND is_deleted=0",
                params![*id],
                |_| Ok(true),
            )
            .optional()?
            .is_some();
        if !alive {
            return Err(AppError::codedp_not_found(
                "account.not-found",
                format!("账户不存在或已删除: {id}"),
                &[*id],
            ));
        }
    }
    Ok(())
}

pub fn validate_currency_consistency(
    conn: &Connection,
    account_id: Option<&str>,
    to_account_id: Option<&str>,
    currency_code: &str,
) -> Result<()> {
    let ids = [account_id, to_account_id];
    for id in ids.iter().flatten() {
        let account_currency: Option<String> = conn
            .query_row(
                "SELECT currency_code FROM accounts WHERE id=?1",
                params![*id],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(account_currency) = account_currency
            && account_currency != currency_code
        {
            return Err(AppError::codedp(
                "transaction.currency-mismatch",
                format!("账户币种（{account_currency}）与交易币种（{currency_code}）不一致"),
                &[account_currency.as_str(), currency_code],
            ));
        }
    }
    Ok(())
}

pub fn insert_row(conn: &Connection, row: &NormalizedRow) -> Result<String> {
    let id = new_uuid();
    insert_row_with_id(conn, &id, row)?;
    Ok(id)
}

pub fn insert_row_with_id(conn: &Connection, id: &str, row: &NormalizedRow) -> Result<()> {
    let now = now_iso();
    conn.execute(
        "INSERT INTO transactions \
         (id,kind,amount_cents,currency_code,amount_native_cents,account_id,to_account_id,\
         funding_account_id,category_id,merchant_id,policy_id,refund_of_transaction_id,note,source_order_no,date,created_at,updated_at,version,device_id,\
         fx_rate_used,fx_rate_source,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,0)",
        params![
            id,
            row.kind.as_str(),
            row.amount_cents,
            row.currency_code,
            row.amount_native_cents,
            row.account_id,
            row.to_account_id,
            row.funding_account_id,
            row.category_id,
            row.merchant_id,
            row.policy_id,
            row.refund_of_transaction_id,
            row.note,
            row.source_order_no,
            row.date,
            now,
            now,
            1,
            device_id(conn)?,
            row.fx_rate_used,
            row.fx_rate_source.map(amount::FxRateSource::as_str),
        ],
    )?;
    crate::write::funding_items::insert_rows(conn, id, &row.funding)?;
    crate::write::purchase_items::insert_rows(conn, id, &row.purchases)?;
    let item_accounts = crate::write::funding_items::item_account_ids(conn, id)?;
    let item_refs: Vec<&str> = item_accounts.iter().map(String::as_str).collect();
    balance::refresh_affected_balances(
        conn,
        None,
        Some(RowAccounts {
            account_id: row.account_id.as_deref(),
            to_account_id: row.to_account_id.as_deref(),
            funding_account_id: row.funding_account_id.as_deref(),
            funding_items: &item_refs,
        }),
    )?;
    Ok(())
}

pub fn update_row(conn: &Connection, id: &str, row: &NormalizedRow) -> Result<()> {
    let (old_account_id, old_to_account_id, old_funding_account_id): (
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn.query_row(
        "SELECT account_id, to_account_id, funding_account_id FROM transactions WHERE id=?1",
        params![id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let old_item_accounts = crate::write::funding_items::item_account_ids(conn, id)?;
    conn.execute(
        "UPDATE transactions \
         SET kind=?2, amount_cents=?3, currency_code=?4, amount_native_cents=?5, account_id=?6, \
         to_account_id=?7, funding_account_id=?8, category_id=?9, merchant_id=?10, policy_id=?11, refund_of_transaction_id=?12, note=?13, date=?14, \
         updated_at=?15, version=version+1, device_id=?16, fx_rate_used=?17, fx_rate_source=?18, source_order_no=?19 \
         WHERE id=?1",
        params![
            id,
            row.kind.as_str(),
            row.amount_cents,
            row.currency_code,
            row.amount_native_cents,
            row.account_id,
            row.to_account_id,
            row.funding_account_id,
            row.category_id,
            row.merchant_id,
            row.policy_id,
            row.refund_of_transaction_id,
            row.note,
            row.date,
            now_iso(),
            device_id(conn)?,
            row.fx_rate_used,
            row.fx_rate_source.map(amount::FxRateSource::as_str),
            row.source_order_no,
        ],
    )?;
    crate::write::funding_items::replace_rows(conn, id, &row.funding)?;
    crate::write::purchase_items::replace_rows(conn, id, &row.purchases)?;
    let new_item_accounts = crate::write::funding_items::item_account_ids(conn, id)?;
    let old_item_refs: Vec<&str> = old_item_accounts.iter().map(String::as_str).collect();
    let new_item_refs: Vec<&str> = new_item_accounts.iter().map(String::as_str).collect();
    balance::refresh_affected_balances(
        conn,
        Some(RowAccounts {
            account_id: old_account_id.as_deref(),
            to_account_id: old_to_account_id.as_deref(),
            funding_account_id: old_funding_account_id.as_deref(),
            funding_items: &old_item_refs,
        }),
        Some(RowAccounts {
            account_id: row.account_id.as_deref(),
            to_account_id: row.to_account_id.as_deref(),
            funding_account_id: row.funding_account_id.as_deref(),
            funding_items: &new_item_refs,
        }),
    )?;
    Ok(())
}

impl TryFrom<&NormalizedTransaction> for NormalizedRow {
    type Error = AppError;

    fn try_from(norm: &NormalizedTransaction) -> Result<Self> {
        Ok(NormalizedRow {
            kind: norm.kind,
            amount_cents: norm.amount_cents,
            currency_code: norm.currency_code.clone(),
            amount_native_cents: norm.amount_native_cents,
            fx_rate_used: norm.fx_rate_used,
            fx_rate_source: norm.fx_rate_source,
            account_id: norm.account_id.clone(),
            to_account_id: norm.to_account_id.clone(),
            funding_account_id: norm.funding_account_id.clone(),
            funding: norm
                .funding
                .iter()
                .map(crate::write::funding_items::FundingItem::from)
                .collect(),
            category_id: norm.category_id.clone(),
            purchases: norm
                .purchases
                .iter()
                .map(crate::write::purchase_items::PurchaseItem::from)
                .collect(),
            merchant_id: norm.merchant_id.clone(),
            policy_id: norm.policy_id.clone(),
            refund_of_transaction_id: norm.refund_of_transaction_id.clone(),
            note: norm.note.clone(),
            source_order_no: norm.source_order_no.clone(),
            date: norm.date.clone(),
        })
    }
}

impl From<&NormalizedRow> for NormalizedTransaction {
    fn from(row: &NormalizedRow) -> Self {
        NormalizedTransaction {
            kind: row.kind,
            amount_cents: row.amount_cents,
            currency_code: row.currency_code.clone(),
            amount_native_cents: row.amount_native_cents,
            fx_rate_used: row.fx_rate_used,
            fx_rate_source: row.fx_rate_source,
            account_id: row.account_id.clone(),
            to_account_id: row.to_account_id.clone(),
            funding_account_id: row.funding_account_id.clone(),
            funding: row
                .funding
                .iter()
                .map(crate::model::TransactionFundingInput::from)
                .collect(),
            purchases: row
                .purchases
                .iter()
                .map(crate::model::TransactionPurchaseInput::from)
                .collect(),
            category_id: row.category_id.clone(),
            merchant_id: row.merchant_id.clone(),
            policy_id: row.policy_id.clone(),
            refund_of_transaction_id: row.refund_of_transaction_id.clone(),
            note: row.note.clone(),
            source_order_no: row.source_order_no.clone(),
            date: row.date.clone(),
        }
    }
}

#[cfg(test)]
mod tests;
