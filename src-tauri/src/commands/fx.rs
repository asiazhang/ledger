use rusqlite::Connection;

use crate::error::{AppError, Result};

/// 查询账户本位币代码。
pub(crate) fn account_currency_code(conn: &Connection, account_id: &str) -> Result<String> {
    conn.query_row(
        "SELECT currency_code FROM accounts WHERE id=?1",
        rusqlite::params![account_id],
        |r| r.get(0),
    )
    .map_err(Into::into)
}

/// 查询货币对的当前汇率。exchange_rates 每货币对仅保留一行最新，无需日期参数。
/// 查不到正向 (base→quote) 时，兜底查反向 (quote→base) 并取倒数。
pub(crate) fn exchange_rate(conn: &Connection, base_code: &str, quote_code: &str) -> Result<f64> {
    if base_code == quote_code {
        return Ok(1.0);
    }
    if let Ok(rate) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![base_code, quote_code],
        |r| r.get(0),
    ) {
        return Ok(rate);
    }
    if let Ok(rev) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![quote_code, base_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rev <= 0.0 {
            return Err(AppError::Invalid(format!(
                "反向汇率 {quote_code}->{base_code} 非正: {rev}"
            )));
        }
        return Ok(1.0 / rev);
    }
    Err(AppError::Invalid(format!(
        "未找到 {base_code} -> {quote_code} 的汇率（正反向均无）"
    )))
}

/// 将交易金额折算为账户本位币金额。汇率取当前最新，无需交易日期。
pub(crate) fn convert_to_native(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
    account_id: &str,
) -> Result<i64> {
    let account_currency = account_currency_code(conn, account_id)?;
    if currency_code == account_currency {
        Ok(amount_cents)
    } else {
        let rate = exchange_rate(conn, currency_code, &account_currency)?;
        Ok((amount_cents as f64 * rate).round() as i64)
    }
}
