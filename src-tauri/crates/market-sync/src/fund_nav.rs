use chrono::NaiveDate;
use rusqlite::params;

use ledger_investment::PriceChannel;
use ledger_investment::constant_price::{ensure_constant_base_price, mark_constant_unit_price};
use ledger_investment::prices::{CSRC_PRICE_SOURCE, price_value_to_cents};

use ledger_infra::error::Result;

use super::session::ScopedSession;

#[derive(Debug, Clone, PartialEq)]
pub struct NavPoint {
    pub(super) date: String,
    pub(super) nav: f64,
}

pub(super) struct FundIdentity<'a> {
    pub(super) instrument_id: &'a str,
    pub(super) code: &'a str,
    pub(super) currency: &'a str,
    pub(super) channel: PriceChannel,
}

impl FundIdentity<'_> {
    pub(super) fn from_instrument(
        instrument: &super::incremental::SyncInstrument,
    ) -> FundIdentity<'_> {
        FundIdentity {
            instrument_id: &instrument.instrument_id,
            code: &instrument.symbol,
            currency: &instrument.currency,
            channel: instrument.channel,
        }
    }
}

pub(super) const MONEY_FUND_UNIT_NAV: f64 = 1.0;

pub(super) fn nav_window(watermark: Option<&str>, today: NaiveDate) -> (String, String) {
    let start = match watermark.map(str::trim) {
        Some(w) if !w.is_empty() => match NaiveDate::parse_from_str(w, "%Y-%m-%d") {
            Ok(d) => d.succ_opt().unwrap_or(d),
            Err(_) => {
                tracing::warn!(watermark = %w, "净值水位非法，按首刷回填近两年");
                super::incremental::two_years_ago(today)
            }
        },
        _ => super::incremental::two_years_ago(today),
    };
    (
        start.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    )
}

pub(super) fn trim_to_window(points: Vec<NavPoint>, start: &str, end: &str) -> Vec<NavPoint> {
    points
        .into_iter()
        .filter(|p| p.date.as_str() >= start && p.date.as_str() <= end)
        .collect()
}

pub(super) fn coverage_short_of_first_position(
    earliest_history: Option<&str>,
    first_position_date: Option<&str>,
) -> bool {
    let Some(first) = first_position_date.map(str::trim) else {
        return false;
    };
    let Ok(first) = NaiveDate::parse_from_str(first, "%Y-%m-%d") else {
        return false;
    };
    let Some(earliest) =
        earliest_history.and_then(|d| NaiveDate::parse_from_str(d.trim(), "%Y-%m-%d").ok())
    else {
        return true;
    };
    super::weekly::week_monday(first) < super::weekly::week_monday(earliest)
}

pub(super) fn deep_backfill_window(
    first_position_date: Option<&str>,
    today: NaiveDate,
) -> (String, String) {
    let mut start = super::incremental::two_years_ago(today);
    if let Some(d) = first_position_date
        .map(str::trim)
        .and_then(|s| NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
    {
        start = start.min(super::weekly::week_monday(d));
    }
    (
        start.format("%Y-%m-%d").to_string(),
        today.format("%Y-%m-%d").to_string(),
    )
}

pub(super) async fn mark_constant_price_on_confirm<Q: ScopedSession>(
    session: &Q,
    instrument_id: &str,
    currency_code: &str,
    priced_at: String,
) -> Result<bool> {
    let cents = price_value_to_cents(MONEY_FUND_UNIT_NAV);
    let instrument_id = instrument_id.to_string();
    let currency_code = currency_code.to_string();
    session
        .with_connection(move |conn| {
            mark_constant_unit_price(conn, &instrument_id, cents)?;
            ensure_constant_base_price(
                conn,
                &instrument_id,
                cents,
                &currency_code,
                &priced_at,
                CSRC_PRICE_SOURCE,
            )
        })
        .await
}

pub(super) async fn read_fund_watermark<Q: ScopedSession>(
    session: &Q,
    instrument_id: &str,
) -> Result<(Option<String>, bool)> {
    let instrument_id = instrument_id.to_string();
    session
        .with_connection(move |conn| {
            let watermark: Option<String> = conn
                .query_row(
                    "SELECT nav_date FROM market_prices WHERE instrument_id=?1",
                    params![instrument_id],
                    |r| r.get(0),
                )
                .ok();
            let has_history = ledger_investment::backfill::has_any_history(conn, &instrument_id)?;
            Ok((watermark, has_history))
        })
        .await
}

pub(super) async fn read_fund_coverage<Q: ScopedSession>(
    session: &Q,
    instrument_id: &str,
) -> Result<(Option<String>, Option<String>)> {
    let instrument_id = instrument_id.to_string();
    session
        .with_connection(move |conn| {
            let earliest_history: Option<String> = conn
                .query_row(
                    "SELECT MIN(trade_date) FROM price_history WHERE instrument_id=?1",
                    params![instrument_id],
                    |r| r.get(0),
                )
                .map_err(ledger_infra::error::AppError::from)?;
            let first_position =
                ledger_investment::holdings::first_position_date(conn, &instrument_id)?;
            Ok((earliest_history, first_position))
        })
        .await
}
