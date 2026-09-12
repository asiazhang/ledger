//! 交易×币种接缝（共享语义区，spec #1086 / issue #1092）：本位币基准读取的注册点。
//!
//! 职责：为金额口径折算（[`super::convert_to_native`]）提供本位币读取单点
//! （`app_settings` 的 `ledger.base_currency` 键）。不变量：未注册即码化错误
//! （折算显式失败，不静默回退币种）。ADR 指针：ADR-0112 决策 5 / ADR-0113 决策
//! 3.1（接缝契约随消费概念归共享语义区）。陷阱：读实现由币种域装入、壳层接线。

use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

/// 本位币基准读取钩子：返回当前本位币代码（缺 key / 缺表时实现侧回默认值，
/// 行为免费正确）。
pub type BaseCurrencyReader = fn(&Connection) -> Result<String>;

static BASE_CURRENCY_READER: OnceLock<BaseCurrencyReader> = OnceLock::new();

/// 注册本位币基准读取实现（幂等：进程级一次，重复注册保留首次）。调用点在
/// 币种域 `install_base_currency_hook`，壳层启动接线，业务代码不直接调用。
pub fn register_base_currency_reader(reader: BaseCurrencyReader) {
    let _ = BASE_CURRENCY_READER.set(reader);
}

/// 未注册错误的单一构造（纯函数，可测）：码化 Invalid——接线缺失是程序缺陷。
fn base_currency_reader_missing_error() -> AppError {
    AppError::coded(
        "transaction.base-currency-reader-unregistered",
        "本位币读取钩子未注册：本位币折算被拒绝（壳层启动接线缺失）",
    )
}

/// 本位币基准委派（折算与默认币种查询共用）：未注册即码化错误。
pub(crate) fn current_base_currency(conn: &Connection) -> Result<String> {
    let reader = BASE_CURRENCY_READER
        .get()
        .ok_or_else(base_currency_reader_missing_error)?;
    reader(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 钩子未注册即码化错误_错误码可判读() {
        let err = base_currency_reader_missing_error();
        assert_eq!(
            err.code(),
            Some("transaction.base-currency-reader-unregistered")
        );
    }
}
