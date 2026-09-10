//! 出资账户准入校验（issue #935 / ADR-0096）：buy/sell 可选出资账户的行为层收口。
//!
//! 归因语义（哪端记账）归 [`super::amount`] 的矩阵与 [`super::amount::account_flow_expr`]
//! 单一真源；本模块只做**准入**（谁可以作出资账户），创建/修改/批量导入各写入口
//! 经 Writer 接缝（通用 kind）与投资域 prepare（buy/sell）自然走到，不另设第二份判定。
//!
//! 准入闭集（ADR-0096 决策 6）：
//! - 仅 buy/sell 可携带出资账户（其余 kind 行为层拒绝，schema 层不设 kind 限制——
//!   与 merchant_id/policy_id 同款纪律，放开无需再改表）；
//! - 出资账户必须是现金类账户：cash / bank / credit / ewallet / other；
//!   投资账户（再挂一层无意义）与 receivable / debt（非自有现金）排除；
//! - 出资账户必须存在且未软删除（软删账户不可被**新选择**，历史引用照常保留——
//!   与商户/保单同款语义，issue #188 / ADR-0028）；
//! - 出资账户币种必须与交易币种一致（现金腿不折算，跨币种出资走先换汇再出资）。
//!
//! 错误全部码化（ADR-0050），模板同步于 `src/i18n/locales/*/errors.json`
//! （`transaction.funding-unsupported` 与 `funding.*` 节）。

use rusqlite::Connection;

use crate::accounts::AccountType;
use crate::error::{AppError, Result};

use super::amount::TransactionKind;

/// 出资账户准入的账户类型闭集：现金类账户（ADR-0096 决策 6）。
/// 投资账户与 receivable / debt 排除——非自有现金与再挂一层的出资无记账语义。
const FUNDING_ALLOWED_TYPES: [AccountType; 5] = [
    AccountType::Cash,
    AccountType::Bank,
    AccountType::Credit,
    AccountType::Ewallet,
    AccountType::Other,
];

/// 校验一笔交易输入携带的出资账户（创建与修改共用；不落库、只判定）。
///
/// - `funding_account_id` 为 `None` 恒通过（出资账户是可选字段，缺省即回落
///   「结算账户 = 投资账户」的既有归因）；
/// - 携带时依次判定：kind 准入（仅 buy/sell）→ 账户存在且未软删 → 账户类型
///   闭集 → 币种与 `transaction_currency` 一致。
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
    let (account_type, account_currency): (AccountType, String) = conn
        .query_row(
            "SELECT type, currency_code FROM accounts WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            // 未命中（不存在或已软删除）：码化 NotFound 引导自纠，与商户/保单
            // 的「存在且未软删」校验同款；其余数据库错误原样上抛。
            rusqlite::Error::QueryReturnedNoRows => AppError::codedp_not_found(
                "funding.account-not-found",
                format!("出资账户不存在或已删除: {id}"),
                &[id],
            ),
            other => other.into(),
        })?;
    if !FUNDING_ALLOWED_TYPES.contains(&account_type) {
        return Err(AppError::codedp(
            "funding.account-type-unsupported",
            format!("出资账户类型不支持: {account_type}（仅现金类账户可作出资账户）"),
            &[&account_type.to_string()],
        ));
    }
    if account_currency != transaction_currency {
        return Err(AppError::codedp(
            "funding.currency-mismatch",
            format!("出资账户币种（{account_currency}）与交易币种（{transaction_currency}）不一致"),
            &[&account_currency, transaction_currency],
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
