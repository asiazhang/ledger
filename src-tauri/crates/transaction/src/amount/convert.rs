//! 本位币折算（共享语义区）：raw 币种金额 → 全局默认币种，两个具名入口。
//!
//! 职责：[`default_currency_code`](本位币基准读取)、[`convert_to_native_current`]
//! （**当期折算**，读路径入口：持仓市值、净资产、财务自由度、实物资产估值、跨账本
//! 汇总、定时花费）、[`convert_to_native_on_trade_date`]（**按交易日折算**，写路径
//! 入口，#1547 接入：按交易所属 ISO 周命中汇率历史）。两入口不设隐式默认，调用方
//! 必须显式选择（#1540 spec）。
//! 共同不变量：基准为全局默认币种、与账户币种无关（避免跨账户漂移）；与本位币同
//! 币种原样返回；正反向汇率均无即报错，不静默混币种。ADR 指针：ADR-0011 / ADR-0091
//! 决策 3 / ADR-0113 决策 3.1。陷阱：本位币读取经 `super::base_currency` 接缝，
//! 未注册即码化错误。

use rusqlite::{Connection, OptionalExtension};

use ledger_infra::error::{AppError, Result};

/// 周键派生表达式（与 `fx_rate_history.week_start` 生成列同式，V010）：周一为
/// 周键。命中查询与文案推导共用同一片段，保证两侧对同一交易日恒得同周键。
const WEEK_START_EXPR: &str = "date(?,'-6 days','weekday 1')";

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

/// **当期折算**（读路径入口）：将原始币种金额按**当期汇率**折算为全局默认币种
/// 金额（四舍五入到分）。
///
/// - 取数读当期汇率表 `exchange_rates`（每币种对一行当期值）。
/// - 币种与默认币种相同 → 1:1 原样返回。
/// - 基准为 [`default_currency_code`]，**与账户币种无关**：
///   各账户的交易统一折算到同一本位币，避免跨账户漂移。
/// - 正反向汇率均无 → 报错，不静默混币种。
///
/// 消费面（#1541 起）：持仓市值、净资产、财务自由度、实物资产估值、跨账本汇总、
/// 定时花费等读路径（写路径自 #1547 起全部改接
/// [`convert_to_native_on_trade_date`]，当期表不再是写路径取数源）。
pub fn convert_to_native_current(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
) -> Result<i64> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        return Ok(amount_cents);
    }
    let rate = lookup_exchange_rate(conn, currency_code, &target)?;
    Ok((amount_cents as f64 * rate).round() as i64)
}

/// **按交易日折算**（写路径入口，#1547 接入）：raw 币种金额 → 全局默认币种，
/// 取数时点为交易所属日期（#1540 spec：折算时点取交易日期，不用当期值折算历史交易）。
///
/// - 取数读汇率历史序列 `fx_rate_history`，按交易所属 ISO 周精确命中周键
///   （周一为周键，与 `week_start` 生成列同式，SQL 内派生）；正反向兜底；
///   整周无点即报错——不滑到相邻周、不回落当期表（`exchange_rates` 只服务
///   读路径当期折算 [`convert_to_native_current`]）。
/// - 整周无点的文案区分「该周尚未发布（重试即可）」与「该周历史空缺」。
/// - 币种与默认币种相同 → 1:1 原样返回；基准为 [`default_currency_code`]。
/// - 显式汇率优先由 #1549 接入。
pub fn convert_to_native_on_trade_date(
    conn: &Connection,
    amount_cents: i64,
    currency_code: &str,
    trade_date: &str,
) -> Result<i64> {
    let target = default_currency_code(conn)?;
    if currency_code == target {
        return Ok(amount_cents);
    }
    let rate = lookup_fx_history_rate(conn, currency_code, &target, trade_date)?;
    Ok((amount_cents as f64 * rate).round() as i64)
}

/// 按交易日所属 ISO 周在汇率历史序列查汇率（正查失败则反查取倒数）。
///
/// 周键派生在 SQL 内完成（`date(?,'-6 days','weekday 1')`，与 `week_start`
/// 生成列同式），畸形日期派生 NULL → 不命中，落到整周无点报错路径，不静默。
fn lookup_fx_history_rate(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
) -> Result<f64> {
    let week_of = |base: &str, quote: &str| {
        conn.query_row(
            &format!(
                "SELECT rate FROM fx_rate_history WHERE base_code=?1 AND quote_code=?2 \
                 AND week_start = {WEEK_START_EXPR}"
            ),
            rusqlite::params![base, quote, trade_date],
            |r| r.get::<_, f64>(0),
        )
        .optional()
    };
    // 只认「该周无行」为缺数据；其余数据库错误原样上抛，不静默并入缺汇率。
    if let Some(rate) = week_of(base_code, quote_code)? {
        if rate <= 0.0 {
            return Err(AppError::codedp(
                "fx.rate-non-positive",
                format!("汇率 {base_code}->{quote_code} 非正: {rate}"),
                &[base_code, quote_code, &rate.to_string()],
            ));
        }
        return Ok(rate);
    }
    if let Some(rev) = week_of(quote_code, base_code)? {
        if rev <= 0.0 {
            return Err(AppError::codedp(
                "fx.reverse-rate-non-positive",
                format!("反向汇率 {quote_code}->{base_code} 非正: {rev}"),
                &[quote_code, base_code, &rev.to_string()],
            ));
        }
        return Ok(1.0 / rev);
    }
    Err(missing_week_error(conn, base_code, quote_code, trade_date))
}

/// 整周无点的 `fx.rate-missing`：文案区分「该周尚未发布（重试即可）」与
/// 「该周历史空缺」。交易周不早于本周（含未来周）→ 尚未发布，同步后重试即可；
/// 更早的历史周缺点 → 历史空缺，重试无济于事。畸形日期派生不出周键时按
/// 历史空缺处理（写入路径对畸形日期本就应先修正，不静默）。沿用既有码与
/// 参数（base/quote，#1540 spec 错误面），周键只进中文 message。
fn missing_week_error(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    trade_date: &str,
) -> AppError {
    let trade_week: Option<String> = conn
        .query_row(
            &format!("SELECT {WEEK_START_EXPR}"),
            rusqlite::params![trade_date],
            |r| r.get(0),
        )
        .unwrap_or(None);
    let current_week: Option<String> = conn
        .query_row(
            "SELECT date('now','localtime','-6 days','weekday 1')",
            [],
            |r| r.get(0),
        )
        .ok();
    // 交易周不早于本周（含未来周）→ 尚未发布；本周判定不可得（查询失败）或
    // 交易周更早（含畸形日期派生不出周键）→ 保守按历史空缺，不误导用户重试。
    let not_yet_published = matches!((&trade_week, &current_week), (Some(w), Some(c)) if w >= c);
    let week = trade_week.unwrap_or_else(|| trade_date.to_string());
    if not_yet_published {
        AppError::codedp(
            "fx.rate-missing",
            format!(
                "未找到 {base_code} -> {quote_code} 在 {week} 当周的汇率：该周尚未发布，待汇率同步后重试即可"
            ),
            &[base_code, quote_code],
        )
    } else {
        AppError::codedp(
            "fx.rate-missing",
            format!("未找到 {base_code} -> {quote_code} 在 {week} 当周的汇率：该周历史空缺"),
            &[base_code, quote_code],
        )
    }
}
