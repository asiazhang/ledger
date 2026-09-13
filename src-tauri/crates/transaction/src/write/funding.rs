//! 出资账户准入校验（写路径，issue #935 / ADR-0096）：buy/sell 可选出资账户的行为层收口。
//!
//! 职责：谁可以作出资账户——各写入口经 Writer 接缝（通用 kind）与投资域 prepare
//! （buy/sell）自然走到，不另设第二份判定。不变量（准入闭集，ADR-0096 决策 6）：
//! 仅 buy/sell 可携带；账户须存在未软删且为现金类（cash / bank / credit / ewallet /
//! other）；账户币种须与交易币种一致（现金腿不折算）。ADR 指针：ADR-0096 决策 6 /
//! ADR-0113 决策 8。陷阱：归因语义归 `crate::amount` 矩阵单一真源；账户行读取经
//! 接缝 `crate::seams::funding`；错误全部码化（ADR-0050）模板同步 `errors.json`。

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

use crate::amount::TransactionKind;
use crate::seams::funding::{FundingAccountClass, lookup_funding_account};

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
    // 出资账户视图经接缝（issue #1092）：SQL 读取与类型→类别映射在账户域实现侧；
    // 缺行（不存在或已软删除）的码化 NotFound 归本域准入语义（与商户/保单同款）。
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
