use std::borrow::Cow;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use crate::InstrumentType;
use ledger_infra::error::Result;
use ledger_sync_protocol::command::SyncCommand;
use ledger_sync_protocol::op::record_local as record_op;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstrumentCommandRow {
    pub symbol: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub name: Option<String>,
    pub currency_code: String,
    pub market: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum InstrumentCommand {
    Create {
        id: String,
        row: InstrumentCommandRow,
    },
    Update {
        id: String,
        symbol: String,
        kind: InstrumentType,
        name: Option<String>,
        market: String,
    },
    Delete {
        id: String,
    },
}

impl InstrumentCommand {
    pub(crate) fn subject(&self) -> Option<Cow<'_, str>> {
        match self {
            InstrumentCommand::Create { id, .. }
            | InstrumentCommand::Update { id, .. }
            | InstrumentCommand::Delete { id } => Some(Cow::Borrowed(id.as_str())),
        }
    }
}

impl SyncCommand for InstrumentCommand {
    const ENTITY: &'static str = "instrument";

    fn subject(&self) -> Option<Cow<'_, str>> {
        InstrumentCommand::subject(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ExchangeRateCommand {
    Upsert {
        id: String,
        base_code: String,
        quote_code: String,
        rate: f64,
        priced_at: String,
        source: Option<String>,
    },
}

impl ExchangeRateCommand {
    pub(crate) fn subject(&self) -> Option<Cow<'_, str>> {
        match self {
            ExchangeRateCommand::Upsert {
                base_code,
                quote_code,
                ..
            } => Some(Cow::Owned(format!("{base_code}->{quote_code}"))),
        }
    }
}

impl SyncCommand for ExchangeRateCommand {
    const ENTITY: &'static str = "exchange_rate";

    fn subject(&self) -> Option<Cow<'_, str>> {
        ExchangeRateCommand::subject(self)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum PriceCommand {
    MarketPrice {
        instrument_id: String,
        price_cents: i64,
        currency_code: String,
        priced_at: String,
        source: Option<String>,
    },
    ManualPrice {
        instrument_id: String,
        date: String,
        price_cents: i64,
    },
}

impl PriceCommand {
    pub(crate) fn subject(&self) -> Option<Cow<'_, str>> {
        match self {
            PriceCommand::MarketPrice { instrument_id, .. } => {
                Some(Cow::Borrowed(instrument_id.as_str()))
            }
            PriceCommand::ManualPrice {
                instrument_id,
                date,
                ..
            } => Some(Cow::Owned(format!(
                "{instrument_id}|{}",
                week_start_of(date)
            ))),
        }
    }
}

impl SyncCommand for PriceCommand {
    const ENTITY: &'static str = "price";

    fn subject(&self) -> Option<Cow<'_, str>> {
        PriceCommand::subject(self)
    }
}

fn week_start_of(date: &str) -> String {
    use chrono::Datelike;
    chrono::NaiveDate::parse_from_str(date.trim(), "%Y-%m-%d")
        .map(|d| {
            (d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64))
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|_| date.to_string())
}

pub(crate) fn record_instrument(conn: &Connection, command: InstrumentCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub(crate) fn record_exchange_rate(conn: &Connection, command: ExchangeRateCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub(crate) fn record_price(conn: &Connection, command: PriceCommand) -> Result<()> {
    record_op(conn, &command)?;
    Ok(())
}

pub fn replay_instrument_command(conn: &Connection, command: &InstrumentCommand) -> Result<()> {
    match command {
        InstrumentCommand::Create { id, row } => {
            super::crud::write_instrument(conn, id, row)?;
            Ok(())
        }
        InstrumentCommand::Update {
            symbol,
            kind,
            name,
            market,
            ..
        } => super::crud::write_instrument_update(conn, symbol, *kind, name.as_deref(), market),
        InstrumentCommand::Delete { id } => super::crud::write_delete_instrument(conn, id),
    }
}

pub fn replay_exchange_rate_command(
    conn: &Connection,
    command: &ExchangeRateCommand,
) -> Result<()> {
    match command {
        ExchangeRateCommand::Upsert {
            id,
            base_code,
            quote_code,
            rate,
            priced_at,
            source,
        } => {
            super::crud::write_exchange_rate(
                conn,
                id,
                base_code,
                quote_code,
                *rate,
                priced_at,
                source.as_deref(),
            )?;
            Ok(())
        }
    }
}

pub fn replay_price_command(conn: &Connection, command: &PriceCommand) -> Result<()> {
    match command {
        PriceCommand::MarketPrice {
            instrument_id,
            price_cents,
            currency_code,
            priced_at,
            source,
        } => {
            super::prices::upsert_market_price(
                conn,
                &super::prices::MarketPriceWrite {
                    instrument_id,
                    price_cents: *price_cents,
                    currency_code,
                    priced_at,
                    nav_date: None,
                    source: source.as_deref(),
                },
            )?;
            Ok(())
        }
        PriceCommand::ManualPrice {
            instrument_id,
            date,
            price_cents,
        } => super::manual_price::write_manual_price(
            conn,
            &super::model::ManualPriceInput {
                instrument_id: instrument_id.clone(),
                date: date.clone(),
                price_cents: *price_cents,
            },
        )
        .map(|_| ()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn week_start_matches_sqlite_expression() {
        assert_eq!(week_start_of("2026-01-12"), "2026-01-12");
        assert_eq!(week_start_of("2026-01-18"), "2026-01-12");
        assert_eq!(week_start_of("2026-01-11"), "2026-01-05");
    }
}
