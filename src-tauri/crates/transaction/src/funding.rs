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
//!
//! 账户行的读取与类型判读自 issue #1092 起经接缝反转（挂载点形态与
//! [`super::write_effects`] 同族）：本域定义出资账户视图钩子（[`register_funding_account_lookup_hook`]）
//! 与准入判读所需的类别投影（[`FundingAccountClass`]），账户行的 SQL 读取与
//! 「哪些账户类型属现金类」映射（ADR-0096 决策 6 的类型闭集词汇）归账户域实现
//! （`accounts::install_funding_account_hook`），壳层启动接线——本域对账户域零
//! 直接依赖（原 AccountType 类型只读认许边随之消亡）。

use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

use super::amount::TransactionKind;

// ---------------------------------------------------------------------------
// 出资账户视图接缝（issue #1092）
// ---------------------------------------------------------------------------

/// 出资准入的账户类别投影（ADR-0096 决策 6）：本域准入只判「现金类 / 非现金类」；
/// 账户类型闭集的词汇与「哪些类型属现金类」映射归账户域实现侧（类型闭集单一
/// 来源不复制进本域）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundingAccountClass {
    /// 现金类账户（cash / bank / credit / ewallet / other）——可作出资账户。
    CashLike,
    /// 非现金类（投资账户、receivable / debt 等）——不可作出资账户。
    Ineligible,
}

/// 出资账户视图（钩子返回的行投影）：准入判读所需的最小字段集。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingAccountView {
    /// 账户类别（现金类判定由实现侧映射，ADR-0096 决策 6 类型闭集词汇归账户域）。
    pub class: FundingAccountClass,
    /// 账户类型展示串（规范字符串，供码化错误文案原样透传）。
    pub type_display: String,
    /// 账户币种（现金腿不折算的币种一致性判定输入）。
    pub currency_code: String,
}

/// 出资账户视图钩子：按 id 读在用账户的类别/类型展示串/币种投影；不存在或已
/// 软删除返回 `None`（「存在且未软删」准入判定的输入，缺行语义归本域判读）。
pub type FundingAccountLookup = fn(&Connection, &str) -> Result<Option<FundingAccountView>>;

static FUNDING_ACCOUNT_LOOKUP: OnceLock<FundingAccountLookup> = OnceLock::new();

/// 注册出资账户视图实现（幂等：进程级一次，重复注册保留首次）。调用点在账户域
/// `install_funding_account_hook`，壳层启动接线，业务代码不直接调用。
pub fn register_funding_account_lookup_hook(hook: FundingAccountLookup) {
    let _ = FUNDING_ACCOUNT_LOOKUP.set(hook);
}

/// 未注册错误的单一构造（纯函数，可测）：码化 Invalid——接线缺失是程序缺陷。
fn funding_account_lookup_missing_error() -> AppError {
    AppError::coded(
        "transaction.funding-account-lookup-unregistered",
        "出资账户视图钩子未注册：出资准入校验被拒绝（壳层启动接线缺失）",
    )
}

/// 出资账户视图委派：钩子在场即调用，缺席即码化错误。
fn lookup_funding_account(conn: &Connection, id: &str) -> Result<Option<FundingAccountView>> {
    let hook = FUNDING_ACCOUNT_LOOKUP
        .get()
        .ok_or_else(funding_account_lookup_missing_error)?;
    hook(conn, id)
}

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
