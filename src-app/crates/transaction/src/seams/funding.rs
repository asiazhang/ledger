//! 交易×账户接缝（出资账户视图，issue #1092 / ADR-0096 决策 6）。
//!
//! 职责：定义出资准入所需的账户类别投影与按 id 读在用账户的注册点；账户行的 SQL
//! 读取与「哪些类型属现金类」映射归账户域实现。不变量：未注册即码化错误（失败
//! 可见，不静默放行）。ADR 指针：ADR-0096 决策 6 / ADR-0112 决策 5。陷阱：准入
//! 判定留写路径 `crate::write::funding`，本模块只持契约与注册点。

use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

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
pub(crate) fn lookup_funding_account(
    conn: &Connection,
    id: &str,
) -> Result<Option<FundingAccountView>> {
    let hook = FUNDING_ACCOUNT_LOOKUP
        .get()
        .ok_or_else(funding_account_lookup_missing_error)?;
    hook(conn, id)
}
