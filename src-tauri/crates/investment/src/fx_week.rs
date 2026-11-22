use std::collections::HashMap;

use chrono::NaiveDate;

use ledger_infra::error::Result;
use rusqlite::Connection;

pub(crate) const FX_NEAREST_WINDOW_WEEKS: i64 = 8;

pub(crate) type FxWeekHistory = HashMap<(String, String), HashMap<String, f64>>;

pub(crate) fn load_fx_week_history(conn: &Connection) -> Result<FxWeekHistory> {
    let mut fx: FxWeekHistory = HashMap::new();
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
    Ok(fx)
}

pub(crate) fn nearest_week_fx_rate(
    fx: &FxWeekHistory,
    base: &str,
    quote: &str,
    event_date: NaiveDate,
) -> Option<f64> {
    let event_week = crate::constant_price::week_monday(event_date);
    for distance in 0..=FX_NEAREST_WINDOW_WEEKS {
        let same_distance = if distance == 0 {
            vec![event_week]
        } else {
            vec![
                event_week - chrono::Duration::weeks(distance),
                event_week + chrono::Duration::weeks(distance),
            ]
        };
        for candidate in same_distance {
            let week_start = candidate.format("%Y-%m-%d").to_string();
            if let Some(rate) = fx_rate_at_week(fx, base, quote, &week_start) {
                return Some(rate);
            }
        }
    }
    None
}

pub(crate) fn fx_rate_at_week(
    fx: &FxWeekHistory,
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
