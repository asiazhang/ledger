use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

pub type BaseCurrencyReader = fn(&Connection) -> Result<String>;

static BASE_CURRENCY_READER: OnceLock<BaseCurrencyReader> = OnceLock::new();

pub fn register_base_currency_reader(reader: BaseCurrencyReader) {
    let _ = BASE_CURRENCY_READER.set(reader);
}

fn base_currency_reader_missing_error() -> AppError {
    AppError::coded(
        "transaction.base-currency-reader-unregistered",
        "本位币读取钩子未注册：本位币折算被拒绝（壳层启动接线缺失）",
    )
}

pub(crate) fn current_base_currency(conn: &Connection) -> Result<String> {
    let reader = BASE_CURRENCY_READER
        .get()
        .ok_or_else(base_currency_reader_missing_error)?;
    reader(conn)
}

#[cfg(test)]
mod tests;
