use std::collections::HashMap;

use chrono::NaiveDate;
use rusqlite::Connection;

use super::fx_week::{FxWeekHistory, fx_rate_at_week, load_fx_week_history};

use ledger_infra::error::Result;

pub(crate) struct AsOfValues {
    latest_price: HashMap<String, (String, String, i64, String)>,
    fx: FxWeekHistory,
}

impl AsOfValues {
    pub(crate) fn load(conn: &Connection, as_of: NaiveDate) -> Result<Self> {
        let mut latest_price: HashMap<String, (String, String, i64, String)> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT ph.instrument_id, ph.trade_date, ph.week_start, ph.price_cents, ph.currency_code \
                 FROM price_history ph \
                 JOIN (SELECT instrument_id, MAX(trade_date) AS trade_date \
                       FROM price_history WHERE trade_date <= ?1 \
                       GROUP BY instrument_id) latest \
                   ON latest.instrument_id = ph.instrument_id \
                  AND latest.trade_date = ph.trade_date",
            )?;
            let rows = stmt.query_map([as_of.to_string()], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?;
            for row in rows {
                let (instrument_id, trade_date, week_start, price_cents, currency) = row?;
                latest_price.insert(
                    instrument_id,
                    (trade_date, week_start, price_cents, currency),
                );
            }
        }
        for constant in crate::constant_price::load_constant_prices(conn)? {
            latest_price.insert(
                constant.instrument_id,
                (
                    as_of.to_string(),
                    crate::constant_price::week_monday(as_of).to_string(),
                    constant.price_cents,
                    constant.currency_code,
                ),
            );
        }
        let fx = load_fx_week_history(conn)?;
        Ok(AsOfValues { latest_price, fx })
    }

    pub(crate) fn market_value(
        &self,
        quantity: f64,
        instrument_id: &str,
        account_currency: &str,
    ) -> Option<i64> {
        let (_, week_start, price_cents, price_currency) = self.latest_price.get(instrument_id)?;
        let rate = fx_rate_at_week(&self.fx, price_currency, account_currency, week_start)?;
        let value = (quantity * *price_cents as f64 / crate::prices::PRICE_UNITS_PER_FEN).round();
        Some((value * rate).round() as i64)
    }
}
