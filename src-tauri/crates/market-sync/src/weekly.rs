use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, params};

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_investment::prices::{price_value_to_cents, upsert_price_history};

use super::persist::upsert_fx_rate_history;

pub(crate) fn week_monday(d: NaiveDate) -> NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}

pub(crate) fn downsample_weekly_points<I>(points: I) -> Vec<(NaiveDate, f64)>
where
    I: IntoIterator<Item = (NaiveDate, f64)>,
{
    let mut sorted: Vec<(NaiveDate, f64)> = points
        .into_iter()
        .filter(|(_, value)| *value > 0.0)
        .collect();
    sorted.sort_by_key(|(date, _)| *date);
    let mut by_week: std::collections::BTreeMap<NaiveDate, (NaiveDate, f64)> =
        std::collections::BTreeMap::new();
    for (d, value) in sorted {
        by_week.insert(week_monday(d), (d, value));
    }
    by_week.into_values().collect()
}

pub(crate) fn commit_price_history_weekly(
    conn: &Connection,
    instrument_id: &str,
    currency: &str,
    source: &str,
    daily_points: &[(NaiveDate, f64)],
) -> Result<bool> {
    let instrument_id = instrument_id.to_string();
    let currency = currency.to_string();
    let source = source.to_string();
    commit_weekly(
        conn,
        daily_points,
        price_value_to_cents,
        |conn| {
            let mut stmt = conn.prepare(
                "SELECT trade_date, price_cents FROM price_history WHERE instrument_id = ?1",
            )?;
            let rows = stmt.query_map(params![instrument_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?;
            let mut by_week = HashMap::new();
            for row in rows {
                let (trade_date, cents) = row?;
                if let Ok(day) = NaiveDate::parse_from_str(trade_date.trim(), "%Y-%m-%d") {
                    by_week.insert(week_monday(day), cents);
                }
            }
            Ok(by_week)
        },
        |conn, trade_date, value| {
            upsert_price_history(
                conn,
                &instrument_id,
                trade_date,
                price_value_to_cents(value),
                &currency,
                &source,
            )
        },
    )
}

pub(crate) fn commit_fx_rate_history_weekly(
    conn: &Connection,
    base_code: &str,
    quote_code: &str,
    source: &str,
    daily_points: &[(NaiveDate, f64)],
) -> Result<bool> {
    let base_code = base_code.to_string();
    let quote_code = quote_code.to_string();
    let source = source.to_string();
    commit_weekly(
        conn,
        daily_points,
        |rate| rate,
        |conn| {
            let mut stmt = conn.prepare(
                "SELECT week_start, rate FROM fx_rate_history \
                 WHERE base_code = ?1 AND quote_code = ?2",
            )?;
            let rows = stmt.query_map(params![base_code, quote_code], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })?;
            let mut by_week = HashMap::new();
            for row in rows {
                let (week_start, rate) = row?;
                if let Ok(monday) = NaiveDate::parse_from_str(week_start.trim(), "%Y-%m-%d") {
                    by_week.insert(monday, rate);
                }
            }
            Ok(by_week)
        },
        |conn, trade_date, rate| {
            upsert_fx_rate_history(conn, &base_code, &quote_code, trade_date, rate, &source)
        },
    )
}

fn commit_weekly<V: PartialEq>(
    conn: &Connection,
    daily_points: &[(NaiveDate, f64)],
    comparable: impl Fn(f64) -> V,
    existing_by_week: impl FnOnce(&Connection) -> Result<HashMap<NaiveDate, V>>,
    mut write_point: impl FnMut(&Connection, &str, f64) -> Result<()>,
) -> Result<bool> {
    ensure_transaction(conn, || {
        let weekly = downsample_weekly_points(daily_points.iter().copied());
        let existing = existing_by_week(conn)?;
        let new_points: Vec<(NaiveDate, f64)> = weekly
            .into_iter()
            .filter(|(date, value)| existing.get(&week_monday(*date)) != Some(&comparable(*value)))
            .collect();
        for (date, value) in &new_points {
            write_point(conn, &date.format("%Y-%m-%d").to_string(), *value)?;
        }
        Ok(!new_points.is_empty())
    })
}
