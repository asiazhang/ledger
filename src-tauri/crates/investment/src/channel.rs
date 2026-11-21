use super::fund::is_six_digit_code;
use super::market::{Market, QuoteMarket};
use super::model::InstrumentType;
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceChannel {
    Quote,
    FundNav,
    Constant,
    Manual,
    None,
}

impl PriceChannel {
    pub const ALL: [PriceChannel; 5] = [
        PriceChannel::Quote,
        PriceChannel::FundNav,
        PriceChannel::Constant,
        PriceChannel::Manual,
        PriceChannel::None,
    ];
}

impl std::fmt::Display for PriceChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PriceChannel::Quote => "quote",
            PriceChannel::FundNav => "fund_nav",
            PriceChannel::Constant => "constant",
            PriceChannel::Manual => "manual",
            PriceChannel::None => "none",
        };
        write!(f, "{s}")
    }
}

impl PartialSchema for PriceChannel {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(PriceChannel::ALL.map(|c| c.to_string())))
                .description(Some(
                    "价格写入通道（派生事实：quote 行情 / fund_nav 净值 / constant 恒定价格 / manual 手动报价 / none 无来源）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for PriceChannel {}

pub fn derive_quote_market(
    kind: InstrumentType,
    market: Market,
    symbol: &str,
    constant_unit_price: Option<i64>,
) -> Option<QuoteMarket> {
    match derive_price_channel(kind, market, symbol, constant_unit_price) {
        PriceChannel::Quote => market.as_quote_market(),
        _ => None,
    }
}

pub fn derive_price_channel(
    kind: InstrumentType,
    market: Market,
    symbol: &str,
    constant_unit_price: Option<i64>,
) -> PriceChannel {
    if constant_unit_price.is_some() {
        return PriceChannel::Constant;
    }
    match kind {
        InstrumentType::Stock | InstrumentType::Etf if market.as_quote_market().is_some() => {
            PriceChannel::Quote
        }
        InstrumentType::Stock => PriceChannel::None,
        InstrumentType::Fund if is_six_digit_code(symbol) => PriceChannel::FundNav,
        _ => PriceChannel::Manual,
    }
}
