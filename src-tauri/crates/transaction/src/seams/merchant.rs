use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

pub struct MerchantNameHooks {
    pub find_by_name: fn(&Connection, &str) -> Result<Option<String>>,
    pub create_by_name: fn(&Connection, &str) -> Result<String>,
}

static MERCHANT_HOOKS: OnceLock<MerchantNameHooks> = OnceLock::new();

pub fn register_merchant_hooks(hooks: MerchantNameHooks) {
    let _ = MERCHANT_HOOKS.set(hooks);
}

fn merchant_hooks_missing_error() -> AppError {
    AppError::coded(
        "transaction.merchant-hooks-unregistered",
        "商户名钩子未注册：商户名解析与即建被跳过（壳层启动接线缺失）",
    )
}

pub(crate) fn find_merchant_by_name(conn: &Connection, name: &str) -> Result<Option<String>> {
    let hooks = MERCHANT_HOOKS
        .get()
        .ok_or_else(merchant_hooks_missing_error)?;
    (hooks.find_by_name)(conn, name)
}

pub(crate) fn create_merchant_by_name(conn: &Connection, name: &str) -> Result<String> {
    let hooks = MERCHANT_HOOKS
        .get()
        .ok_or_else(merchant_hooks_missing_error)?;
    (hooks.create_by_name)(conn, name)
}

#[cfg(test)]
mod tests;
