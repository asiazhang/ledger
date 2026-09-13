//! 本位币折算（共享语义区）：raw 币种金额 → 全局默认币种。
//!
//! 职责：[`default_currency_code`]（经接缝读本位币基准）、[`convert_to_native`]
//! （四舍五入到分）。不变量：基准为全局默认币种、与账户币种无关（避免跨账户漂移）；
//! 正反向汇率均无即报错，不静默混币种。ADR 指针：ADR-0091 决策 3 / ADR-0113 决策
//! 3.1。陷阱：本位币读取经 `super::base_currency` 接缝，未注册即码化错误。

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

// ---------------------------------------------------------------------------
// 本位币折算
// ---------------------------------------------------------------------------

/// 全局默认（本位）币种基准：`amount_native_cents` 的折算基准，读账本级设置
/// （LedgerLevelSetting 首个成员，issue #858 / ADR-0091 决策 3）：存储落
/// `app_settings`（ADR-0017，读写协议归币种域），缺 key / 缺表回默认 CNY（行为
/// 免费正确），随多端同步分发、全设备强制一致。读取自 #1092 起经交易×币种接缝
/// （[`base_currency`]）：注册点在本共享语义区，实现由币种域启动时装入，
/// 本域对币种域零直接依赖。模块内其余口径不变。
pub fn default_currency_code(conn: &Connection) -> Result<String> {
    super::base_currency::current_base_currency(conn)
}

/// 查询货币对当前汇率（正查失败则反查取倒数）。
///
/// 私有依赖（spec #52）：旧壳层同名查询已随 issue #60 接线删除，
/// 本函数即其收口后的单一实现（Writer 接缝落地时已统一）。
fn lookup_exchange_rate(conn: &Connection, base_code: &str, quote_code: &str) -> Result<f64> {
    if base_code == quote_code {
        return Ok(1.0);
    }
    if let Ok(rate) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![base_code, quote_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rate <= 0.0 {
            return Err(AppError::codedp(
                "fx.rate-non-positive",
                format!("汇率 {base_code}->{quote_code} 非正: {rate}"),
                &[base_code, quote_code, &rate.to_string()],
            ));
        }
        return Ok(rate);
    }
    if let Ok(rev) = conn.query_row(
        "SELECT rate FROM exchange_rates WHERE base_code=?1 AND quote_code=?2",
        rusqlite::params![quote_code, base_code],
        |r| r.get::<_, f64>(0),
    ) {
        if rev <= 0.0 {
            return Err(AppError::codedp(
                "fx.reverse-rate-non-positive",
                format!("反向汇率 {quote_code}->{base_code} 非正: {rev}"),
                &[quote_code, base_code, &rev.to_string()],
            ));
        }
        return Ok(1.0 / rev);
    }
    Err(AppError::codedp(
        "fx.rate-missing",
        format!("未找到 {base_code} -> {quote_code} 的汇率（正反向均无）"),
        &[base_code, quote_code],
    ))
}

/// 将原始币种金额折算为**全局默认币种**金额（四舍五入到分）。
///
/// - 币种与默认币种相同 → 1:1 原样返回。
/// - 基准为 [`default_currency_code`]，**与账户币种无关**：
///   各账户的交易统一折算到同一本位币，避免跨账户漂移。
/// - 正反向汇率均无 → 报错，不静默混币种。
pub fn convert_to_native(conn: &Connection, amount_cents: i64, currency_code: &str) -> Result<i64> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        return Ok(amount_cents);
    }
    let rate = lookup_exchange_rate(conn, currency_code, &target)?;
    Ok((amount_cents as f64 * rate).round() as i64)
}
