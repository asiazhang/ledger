use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

use crate::amount::TransactionKind;
use crate::seams::funding::{FundingAccountClass, lookup_funding_account};

pub fn validate_funding_account(
    conn: &Connection,
    kind: TransactionKind,
    funding_account_id: Option<&str>,
    transaction_currency: &str,
) -> Result<()> {
    let Some(id) = funding_account_id else {
        return Ok(());
    };
    if !matches!(kind, TransactionKind::Buy | TransactionKind::Sell) {
        return Err(AppError::codedp(
            "transaction.funding-unsupported",
            format!("交易类型 {kind} 不能携带出资账户"),
            &[&kind.to_string()],
        ));
    }
    let view = lookup_funding_account(conn, id)?.ok_or_else(|| {
        AppError::codedp_not_found(
            "funding.account-not-found",
            format!("出资账户不存在或已删除: {id}"),
            &[id],
        )
    })?;
    if view.class != FundingAccountClass::CashLike {
        return Err(AppError::codedp(
            "funding.account-type-unsupported",
            format!(
                "出资账户类型不支持: {}（仅现金类账户可作出资账户）",
                view.type_display
            ),
            &[&view.type_display],
        ));
    }
    if view.currency_code != transaction_currency {
        return Err(AppError::codedp(
            "funding.currency-mismatch",
            format!(
                "出资账户币种（{}）与交易币种（{}）不一致",
                view.currency_code, transaction_currency
            ),
            &[&view.currency_code, transaction_currency],
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
