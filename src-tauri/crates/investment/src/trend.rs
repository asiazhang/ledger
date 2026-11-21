use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::NaiveDate;
use rusqlite::Connection;

use super::constant_price::{ConstantPriceValue, load_constant_prices, weekly_samples};
use super::fx_week::{fx_rate_at_week, load_fx_week_history};
use super::holdings::holdings_legs_by_instrument;
use super::model::{PortfolioTrendPoint, PortfolioValueTrend, TrendRange};
use super::prices::PRICE_UNITS_PER_FEN;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::{AppError, Result};
use ledger_transaction::amount::default_currency_code;

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

fn constant_sample_window(
    constant: &ConstantPriceValue,
    range: &TrendRange,
    today: chrono::NaiveDate,
) -> (chrono::NaiveDate, chrono::NaiveDate) {
    let parse = |raw: &Option<String>| {
        raw.as_deref()
            .and_then(|s| chrono::NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").ok())
    };
    let start =
        parse(&range.start_date).map_or(constant.anchor_date, |s| s.max(constant.anchor_date));
    let end = parse(&range.end_date).unwrap_or(today);
    (start, end)
}

struct PriceRow {
    instrument_id: String,
    trade_date: String,
    week_start: String,
    price_cents: i64,
    currency_code: String,
}

pub fn query_portfolio_value_trend(
    conn: &Connection,
    range: &TrendRange,
) -> Result<PortfolioValueTrend> {
    query_portfolio_value_trend_on(conn, range, super::staleness::beijing_today())
}

pub fn query_portfolio_value_trend_on(
    conn: &Connection,
    range: &TrendRange,
    today: chrono::NaiveDate,
) -> Result<PortfolioValueTrend> {
    ensure_transaction(conn, || portfolio_value_trend_within_tx(conn, range, today))
}

fn portfolio_value_trend_within_tx(
    conn: &Connection,
    range: &TrendRange,
    today: chrono::NaiveDate,
) -> Result<PortfolioValueTrend> {
    validate_range(range)?;
    let native = default_currency_code(conn)?;

    let mut conditions: Vec<String> = vec![
        "1=1".to_string(),
        "instrument_id NOT IN \
         (SELECT id FROM instruments WHERE constant_unit_price IS NOT NULL)"
            .to_string(),
    ];
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

    {
        for constant in load_constant_prices(conn)? {
            let (start, end) = constant_sample_window(&constant, range, today);
            for (week_start, trade_date) in weekly_samples(start, end, today) {
                price_rows.push(PriceRow {
                    instrument_id: constant.instrument_id.clone(),
                    trade_date,
                    week_start,
                    price_cents: constant.price_cents,
                    currency_code: constant.currency_code.clone(),
                });
            }
        }
    }

    let fx = load_fx_week_history(conn)?;

    let legs_by_instrument = holdings_legs_by_instrument(conn)?;
    let mut rows_by_instrument: HashMap<&str, Vec<&PriceRow>> = HashMap::new();
    for row in &price_rows {
        rows_by_instrument
            .entry(row.instrument_id.as_str())
            .or_default()
            .push(row);
    }

    let mut week_cut: BTreeMap<&str, &str> = BTreeMap::new();
    for row in &price_rows {
        week_cut
            .entry(row.week_start.as_str())
            .and_modify(|cut| {
                if row.trade_date.as_str() > *cut {
                    *cut = row.trade_date.as_str();
                }
            })
            .or_insert(row.trade_date.as_str());
    }
    let weeks: Vec<&str> = week_cut.keys().copied().collect();
    let currencies: BTreeSet<&str> = price_rows
        .iter()
        .map(|row| row.currency_code.as_str())
        .collect();
    let mut fx_carried: HashMap<&str, HashMap<&str, f64>> = HashMap::new();
    for currency in currencies {
        let mut carried_rates: HashMap<&str, f64> = HashMap::new();
        let mut last: Option<f64> = None;
        for &week in &weeks {
            if let Some(rate) = fx_rate_at_week(&fx, currency, &native, week).or(last) {
                carried_rates.insert(week, rate);
                last = Some(rate);
            }
        }
        fx_carried.insert(currency, carried_rates);
    }

    let mut by_week: BTreeMap<String, (i64, bool)> = BTreeMap::new();
    for (instrument_id, rows) in rows_by_instrument {
        let mut rows = rows;
        rows.sort_by(|a, b| a.trade_date.cmp(&b.trade_date));
        let mut weekly: HashMap<&str, &PriceRow> = HashMap::new();
        for row in rows {
            weekly.insert(row.week_start.as_str(), row);
        }
        let legs = legs_by_instrument
            .get(instrument_id)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut cursor = 0usize;
        let mut quantity = 0.0f64;
        let mut carried: Option<&PriceRow> = None;
        for &week in &weeks {
            if let Some(&row) = weekly.get(week) {
                carried = Some(row);
            }
            let Some(price) = carried else { continue };
            let sample = match weekly.get(week) {
                Some(row) => row.trade_date.as_str(),
                None => week_cut[week],
            };
            while cursor < legs.len() && legs[cursor].0.as_str() <= sample {
                quantity += legs[cursor].1;
                cursor += 1;
            }
            let Some(&rate) = fx_carried
                .get(price.currency_code.as_str())
                .and_then(|rates| rates.get(week))
            else {
                continue;
            };
            let value = (quantity * price.price_cents as f64 / PRICE_UNITS_PER_FEN).round() as i64;
            let entry = by_week.entry(week.to_string()).or_insert((0, false));
            entry.0 += (value as f64 * rate).round() as i64;
            entry.1 = true;
        }
    }

    let points: Vec<PortfolioTrendPoint> = by_week
        .into_iter()
        .filter(|(_, (_, contributed))| *contributed)
        .map(|(date, (market_value_cents, _))| PortfolioTrendPoint {
            date,
            market_value_cents,
        })
        .collect();

    let backfill = if points.is_empty() {
        super::backfill::portfolio_trend_backfill_status(conn)?
    } else {
        None
    };

    Ok(PortfolioValueTrend {
        currency_code: native.to_string(),
        points,
        backfill,
    })
}
