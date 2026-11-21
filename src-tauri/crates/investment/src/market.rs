use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use ledger_infra::closed_set::closed_set;

closed_set! {
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Market {
    Sh => "sh",
    Sz => "sz",
    Hk => "hk",
    Nasdaq => "nasdaq",
    Nyse => "nyse",
    Amex => "amex",
    Unknown => "unknown",
}
err_label = "市场",
err_code = "instrument.market-unknown",
}

impl Serialize for Market {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Market {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Market::parse(&s).map_err(serde::de::Error::custom)
    }
}

impl PartialSchema for Market {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(Market::ALL.map(|m| m.to_string())))
                .description(Some(
                    "标的挂牌市场（闭集，小写字符串，与 instruments.market 一致）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for Market {}

impl ToSql for Market {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for Market {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        Market::parse(value.as_str()?).map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

impl Market {
    pub fn as_quote_market(self) -> Option<QuoteMarket> {
        match self {
            Market::Sh => Some(QuoteMarket::Sh),
            Market::Sz => Some(QuoteMarket::Sz),
            Market::Hk => Some(QuoteMarket::Hk),
            Market::Nasdaq => Some(QuoteMarket::Nasdaq),
            Market::Nyse => Some(QuoteMarket::Nyse),
            Market::Amex => Some(QuoteMarket::Amex),
            Market::Unknown => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuoteMarket {
    Sh,
    Sz,
    Hk,
    Nasdaq,
    Nyse,
    Amex,
}

impl QuoteMarket {
    pub const ALL: [QuoteMarket; 6] = [
        QuoteMarket::Sh,
        QuoteMarket::Sz,
        QuoteMarket::Hk,
        QuoteMarket::Nasdaq,
        QuoteMarket::Nyse,
        QuoteMarket::Amex,
    ];

    pub const fn as_market(self) -> Market {
        match self {
            QuoteMarket::Sh => Market::Sh,
            QuoteMarket::Sz => Market::Sz,
            QuoteMarket::Hk => Market::Hk,
            QuoteMarket::Nasdaq => Market::Nasdaq,
            QuoteMarket::Nyse => Market::Nyse,
            QuoteMarket::Amex => Market::Amex,
        }
    }

    pub const fn as_str(self) -> &'static str {
        self.as_market().as_str()
    }

    pub const fn as_stock_route(self) -> StockRoute {
        match self {
            QuoteMarket::Sh => StockRoute::Sh,
            QuoteMarket::Sz => StockRoute::Sz,
            QuoteMarket::Hk => StockRoute::Hk,
            QuoteMarket::Nasdaq | QuoteMarket::Nyse | QuoteMarket::Amex => StockRoute::Us,
        }
    }
}

impl std::fmt::Display for QuoteMarket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StockRoute {
    Sh,
    Sz,
    Hk,
    Us,
}

impl StockRoute {
    pub const fn as_str(self) -> &'static str {
        match self {
            StockRoute::Sh => "sh",
            StockRoute::Sz => "sz",
            StockRoute::Hk => "hk",
            StockRoute::Us => "us",
        }
    }
}

pub fn derive_quote_currency(market: Market) -> &'static str {
    match market {
        Market::Hk => "HKD",
        Market::Nasdaq | Market::Nyse | Market::Amex => "USD",
        Market::Sh | Market::Sz | Market::Unknown => "CNY",
    }
}
