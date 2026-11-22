use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundingAccountClass {
    CashLike,
    Ineligible,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FundingAccountView {
    pub class: FundingAccountClass,
    pub type_display: String,
    pub currency_code: String,
}

pub type FundingAccountLookup = fn(&Connection, &str) -> Result<Option<FundingAccountView>>;

static FUNDING_ACCOUNT_LOOKUP: OnceLock<FundingAccountLookup> = OnceLock::new();

pub fn register_funding_account_lookup_hook(hook: FundingAccountLookup) {
    let _ = FUNDING_ACCOUNT_LOOKUP.set(hook);
}

fn funding_account_lookup_missing_error() -> AppError {
    AppError::coded(
        "transaction.funding-account-lookup-unregistered",
        "出资账户视图钩子未注册：出资准入校验被拒绝（壳层启动接线缺失）",
    )
}

pub(crate) fn lookup_funding_account(
    conn: &Connection,
    id: &str,
) -> Result<Option<FundingAccountView>> {
    let hook = FUNDING_ACCOUNT_LOOKUP
        .get()
        .ok_or_else(funding_account_lookup_missing_error)?;
    hook(conn, id)
}
