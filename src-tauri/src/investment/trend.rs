//! 走势查询（issue #138 / spec #135 / ADR-0019）：PortfolioValueTrend 的取数与推算。
//!
//! - 单标的走势：`price_history` 直出，按区间裁剪，从首个有效采样点开始。
//! - 组合市值走势：当期持有数量逐价格行委托时点持仓接缝推算（`holdings_as_of`，
//!   不物化快照），各标的市值 = 数量 × 当期周线价格；非本位币经同期
//!   `fx_rate_history` 正反向兜底折算到 DefaultCurrency 后汇总为一条曲线。
//! - 某周缺价格或缺汇率则该贡献被跳过（不伪造数据）；全部贡献缺失的周无点，
//!   曲线从区间内首个有效采样点开始。

use std::collections::{BTreeMap, HashMap};

use chrono::NaiveDate;
use rusqlite::Connection;

use super::holdings::holdings_as_of;

use super::prices::PRICE_UNITS_PER_FEN;
use crate::error::{AppError, Result};
use crate::models::{
    InstrumentPriceTrend, PortfolioTrendPoint, PortfolioValueTrend, PriceTrendPoint, TrendRange,
};
use crate::transaction::amount::default_currency_code;

/// 校验区间：日期格式合法且起点不晚于终点。返回原样起止字符串（SQL 字符串比较
/// 对 ISO 8601 日期即时间序）。
fn validate_range(range: &TrendRange) -> Result<()> {
    let parse = |label: &str, code: &str, raw: &Option<String>| -> Result<Option<NaiveDate>> {
        raw.as_deref()
            .map(|s| {
                NaiveDate::parse_from_str(s, "%Y-%m-%d")
                    .map_err(|_| AppError::codedp(code, format!("{label}日期格式无效: {s}"), &[s]))
            })
            .transpose()
    };
    let start = parse(
        "起始",
        "instrument.trend-start-date-invalid",
        &range.start_date,
    )?;
    let end = parse("截止", "instrument.trend-end-date-invalid", &range.end_date)?;
    if let (Some(s), Some(e)) = (start, end)
        && s > e
    {
        return Err(AppError::coded(
            "instrument.trend-range-invalid",
            "起始日期不能晚于截止日期",
        ));
    }
    Ok(())
}

/// 单标的走势：PriceHistory 直出，区间裁剪（含端点），按采样日升序。
pub fn query_instrument_price_trend(
    conn: &Connection,
    instrument_id: &str,
    range: &TrendRange,
) -> Result<InstrumentPriceTrend> {
    validate_range(range)?;

    let mut conditions = vec!["instrument_id=?1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(instrument_id.to_string())];
    if let Some(start) = &range.start_date {
        params.push(Box::new(start.clone()));
        conditions.push(format!("trade_date>=?{}", params.len()));
    }
    if let Some(end) = &range.end_date {
        params.push(Box::new(end.clone()));
        conditions.push(format!("trade_date<=?{}", params.len()));
    }
    let sql = format!(
        "SELECT trade_date, price_cents, currency_code FROM price_history \
         WHERE {} ORDER BY trade_date",
        conditions.join(" AND ")
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
    let mut points = Vec::new();
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |r| {
        Ok(PriceTrendPoint {
            date: r.get(0)?,
            price_cents: r.get(1)?,
            currency_code: r.get(2)?,
        })
    })?;
    for row in rows {
        points.push(row?);
    }

    Ok(InstrumentPriceTrend {
        instrument_id: instrument_id.to_string(),
        points,
    })
}

/// 区间内的一条价格历史周点行。
struct PriceRow {
    instrument_id: String,
    trade_date: String,
    week_start: String,
    price_cents: i64,
    currency_code: String,
}

/// 组合市值走势：逐周汇总「当期持有数量 × 周线价格」（折算到本位币）。
///
/// 数量推算：对每条价格行，取该标的截至其采样交易日（含当日）的持有数量，
/// 委托时点持仓接缝 [`holdings_as_of`]——buy/sell 口径单点在推算模块，本函数
/// 只负责取数、折算与组装（数量按交易日取、汇率按周键取，双时间键契约
/// 显式分界）；缺价格或缺同期汇率的标的该周跳过，全周无有效贡献则该周无点。
pub fn query_portfolio_value_trend(
    conn: &Connection,
    range: &TrendRange,
) -> Result<PortfolioValueTrend> {
    validate_range(range)?;
    let native = default_currency_code();

    // 1. 区间内价格历史周点（week_start 为 STORED 生成列，直读即为周键）。
    let mut conditions: Vec<String> = vec!["1=1".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(start) = &range.start_date {
        params.push(Box::new(start.clone()));
        conditions.push(format!("trade_date>=?{}", params.len()));
    }
    if let Some(end) = &range.end_date {
        params.push(Box::new(end.clone()));
        conditions.push(format!("trade_date<=?{}", params.len()));
    }
    let price_sql = format!(
        "SELECT instrument_id, trade_date, week_start, price_cents, currency_code \
         FROM price_history WHERE {} ORDER BY trade_date",
        conditions.join(" AND ")
    );
    let mut price_rows: Vec<PriceRow> = Vec::new();
    {
        let mut stmt = conn.prepare(&price_sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            Ok(PriceRow {
                instrument_id: r.get(0)?,
                trade_date: r.get(1)?,
                week_start: r.get(2)?,
                price_cents: r.get(3)?,
                currency_code: r.get(4)?,
            })
        })?;
        for row in rows {
            price_rows.push(row?);
        }
    }

    // 2. 同期汇率历史：数据量小（周粒度、币种对个位数），全量载入建周键索引。
    let mut fx: HashMap<(String, String), HashMap<String, f64>> = HashMap::new();
    {
        let mut stmt =
            conn.prepare("SELECT base_code, quote_code, week_start, rate FROM fx_rate_history")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (base, quote, week, rate) = row?;
            fx.entry((base, quote)).or_default().insert(week, rate);
        }
    }

    // 3. 按周聚合：逐价格行委托时点持仓接缝推算当期数量（数量按交易日取、
    //    汇率按周键取）；某标的缺汇率则跳过该贡献；全周无有效贡献则该周无点。
    let mut by_week: BTreeMap<String, Vec<PriceRow>> = BTreeMap::new();
    for row in price_rows {
        by_week.entry(row.week_start.clone()).or_default().push(row);
    }

    let mut points = Vec::new();
    for (week, rows) in by_week {
        let mut total = 0i64;
        let mut contributed = false;
        for row in rows {
            let rate = if row.currency_code == native {
                Some(1.0)
            } else {
                historical_fx_rate(&fx, &row.currency_code, native, &week)
            };
            let Some(rate) = rate else { continue };
            let quantity = holdings_as_of(conn, Some(&row.instrument_id), &row.trade_date)?;
            // 金额分 = 数量 × 单价（万分之一元）÷ 换算因子，再折算到本位币（ADR-0038）。
            let value = (quantity * row.price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64;
            total += (value as f64 * rate).round() as i64;
            contributed = true;
        }
        if contributed {
            points.push(PortfolioTrendPoint {
                date: week,
                market_value_cents: total,
            });
        }
    }

    Ok(PortfolioValueTrend {
        currency_code: native.to_string(),
        points,
    })
}

/// 查询同期历史汇率（正查失败则反查取倒数），与 Amount 接缝的正反向兜底同思路；
/// 同期缺失返回 `None`（调用方跳过该贡献，不用当期汇率近似历史）。
fn historical_fx_rate(
    fx: &HashMap<(String, String), HashMap<String, f64>>,
    base: &str,
    quote: &str,
    week_start: &str,
) -> Option<f64> {
    if base == quote {
        return Some(1.0);
    }
    if let Some(rate) = fx
        .get(&(base.to_string(), quote.to_string()))
        .and_then(|w| w.get(week_start))
    {
        return Some(*rate);
    }
    fx.get(&(quote.to_string(), base.to_string()))
        .and_then(|w| w.get(week_start))
        .map(|rev| 1.0 / rev)
}
