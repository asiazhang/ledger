use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

pub type BalanceRefreshHook =
    fn(&Connection, Option<RowAccounts<'_>>, Option<RowAccounts<'_>>) -> Result<()>;

#[derive(Debug, PartialEq, Eq)]
pub struct RowAccounts<'a> {
    pub account_id: Option<&'a str>,
    pub to_account_id: Option<&'a str>,
    pub funding_account_id: Option<&'a str>,
    pub funding_items: &'a [&'a str],
}

static BALANCE_REFRESH_HOOK: OnceLock<BalanceRefreshHook> = OnceLock::new();

pub fn register_balance_refresh_hook(hook: BalanceRefreshHook) {
    let _ = BALANCE_REFRESH_HOOK.set(hook);
}

fn balance_refresh_hook_missing_error() -> AppError {
    AppError::coded(
        "transaction.balance-refresh-hook-unregistered",
        "余额刷新钩子未注册：交易写入的余额缓存重算被跳过将产生缓存漂移（壳层启动接线缺失）",
    )
}

pub(crate) fn dispatch_balance_refresh(
    hook: Option<BalanceRefreshHook>,
    conn: &Connection,
    old: Option<RowAccounts<'_>>,
    new: Option<RowAccounts<'_>>,
) -> Result<()> {
    match hook {
        Some(hook) => hook(conn, old, new),
        None => Err(balance_refresh_hook_missing_error()),
    }
}

pub fn refresh_affected_balances(
    conn: &Connection,
    old: Option<RowAccounts<'_>>,
    new: Option<RowAccounts<'_>>,
) -> Result<()> {
    dispatch_balance_refresh(BALANCE_REFRESH_HOOK.get().copied(), conn, old, new)
}

#[cfg(test)]
mod tests;
