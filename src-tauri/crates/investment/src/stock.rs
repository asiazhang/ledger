use std::future::Future;

use rusqlite::params;

use super::crud;
use super::market::{Market, QuoteMarket, StockRoute, derive_quote_currency};
use super::model::{AddStockInstrumentResult, InstrumentInput, InstrumentType};
use super::quote::{Quote, QuoteAdoptionInput, adopt_quote};
use ledger_infra::db::now_iso;
use ledger_infra::error::{AppError, Result};

pub fn explicit_quote_market(market: &str) -> Result<QuoteMarket> {
    Market::parse(market)
        .ok()
        .and_then(|m| m.as_quote_market())
        .ok_or_else(|| {
            AppError::codedp(
                "stock.market-unsupported",
                format!(
                    "暂不支持查询 {market} 市场（当前支持沪 sh/深 sz/港 hk/美股 nasdaq/nyse/amex）"
                ),
                &[market],
            )
        })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStockCode {
    pub route: StockRoute,
    pub code: String,
}

enum CodeShape {
    BeijingExchange,
    Single(QuoteMarket),
    UsTicker,
    Ambiguous,
}

fn code_shape(code: &str) -> CodeShape {
    if code.is_empty() {
        return CodeShape::Ambiguous;
    }
    if code.bytes().all(|b| b.is_ascii_alphabetic()) {
        return CodeShape::UsTicker;
    }
    if !code.bytes().all(|b| b.is_ascii_digit()) {
        return CodeShape::Ambiguous;
    }
    match code.len() {
        6 => match code.as_bytes()[0] {
            b'4' | b'8' => CodeShape::BeijingExchange,
            b'6' | b'5' => CodeShape::Single(QuoteMarket::Sh),
            b'0' | b'3' | b'1' => CodeShape::Single(QuoteMarket::Sz),
            _ => CodeShape::Ambiguous,
        },
        1..=5 => CodeShape::Single(QuoteMarket::Hk),
        _ => CodeShape::Ambiguous,
    }
}

fn normalize_code(route: StockRoute, code: &str) -> String {
    match route {
        StockRoute::Hk => format!("{code:0>5}"),
        StockRoute::Us => code.to_ascii_uppercase(),
        StockRoute::Sh | StockRoute::Sz => code.to_string(),
    }
}

fn market_conflict(market: QuoteMarket, code: &str) -> AppError {
    AppError::codedp(
        "stock.market-conflict",
        format!("market 参数 {market} 与股票代码 {code} 的市场形态矛盾，请核对后重试"),
        &[market.as_str(), code],
    )
}

fn bse_unsupported(code: &str) -> AppError {
    AppError::codedp(
        "stock.bse-unsupported",
        format!("股票代码 {code} 为北交所代码（4/8 开头），暂不支持"),
        &[code],
    )
}

pub fn resolve_stock_code(market: Option<QuoteMarket>, code: &str) -> Result<ResolvedStockCode> {
    let resolved = |route| {
        Ok(ResolvedStockCode {
            route,
            code: normalize_code(route, code),
        })
    };
    match code_shape(code) {
        CodeShape::BeijingExchange => Err(bse_unsupported(code)),
        CodeShape::Single(inferred) => match market {
            None => resolved(inferred.as_stock_route()),
            Some(m) if m == inferred => resolved(inferred.as_stock_route()),
            Some(m) => Err(market_conflict(m, code)),
        },
        CodeShape::UsTicker => match market {
            None => resolved(StockRoute::Us),
            Some(m) if m.as_stock_route() == StockRoute::Us => resolved(StockRoute::Us),
            Some(m) => Err(market_conflict(m, code)),
        },
        CodeShape::Ambiguous => match market {
            Some(m) => Err(market_conflict(m, code)),
            None => Err(AppError::codedp(
                "stock.code-unresolvable",
                format!("无法根据股票代码 {code} 的形态推断市场，请显式传入 market 参数"),
                &[code],
            )),
        },
    }
}

pub fn resolve_add_stock_channel(channel: &str) -> Result<Option<QuoteMarket>> {
    match channel {
        "sh" => Ok(Some(QuoteMarket::Sh)),
        "sz" => Ok(Some(QuoteMarket::Sz)),
        "hk" => Ok(Some(QuoteMarket::Hk)),
        "us" => Ok(None),
        other => Err(AppError::codedp(
            "stock.channel-unsupported",
            format!("暂不支持的投资标的录入通道 {other}（当前支持沪 sh/深 sz/港 hk/美股 us）"),
            &[other],
        )),
    }
}

pub async fn fetch_stock_quote_for_add<F, Fut>(
    channel: &str,
    code: &str,
    fetch: &mut F,
) -> Result<Quote>
where
    F: FnMut(&str, StockRoute) -> Fut,
    Fut: Future<Output = Result<Quote>>,
{
    let market = resolve_add_stock_channel(channel)?;
    let candidate = resolve_stock_code(market, code)?;
    fetch(&candidate.code, candidate.route).await
}

pub fn add_stock_instrument_with_quote(
    conn: &rusqlite::Connection,
    quote: &Quote,
) -> Result<AddStockInstrumentResult> {
    let kind = quote.stock_kind_hint();
    let market = quote.stock_market()?;
    let outcome = adopt_stock_quote(conn, kind, quote)?;
    Ok(AddStockInstrumentResult {
        instrument_id: outcome.instrument_id,
        symbol: quote.code.clone(),
        name: quote.name.clone(),
        kind,
        market: market.as_str().to_string(),
        currency_code: derive_quote_currency(market).to_string(),
        price_cents: quote.price_cents,
        price_date: quote.price_date.clone(),
        price_written: outcome.price_written,
    })
}

pub struct StockCreateOutcome {
    pub instrument_id: String,
    pub price_written: bool,
}

#[derive(Debug)]
pub enum StockCreateRoute {
    Enhance(StockEnhancePlan),
    Reject(AppError),
    Generic,
}

#[derive(Debug)]
pub struct StockEnhancePlan {
    pub candidate: ResolvedStockCode,
    pub degrade_market: Market,
}

pub fn route_stock_creation(market: Option<&str>, symbol: &str) -> StockCreateRoute {
    let shape = code_shape(symbol);
    match shape {
        CodeShape::BeijingExchange => StockCreateRoute::Reject(bse_unsupported(symbol)),
        CodeShape::Single(_) | CodeShape::UsTicker => {
            let explicit = match market.map(explicit_quote_market) {
                Some(Ok(m)) => Some(m),
                Some(Err(e)) => return StockCreateRoute::Reject(e),
                None => None,
            };
            match resolve_stock_code(explicit, symbol) {
                Ok(candidate) => StockCreateRoute::Enhance(StockEnhancePlan {
                    candidate,
                    degrade_market: match explicit {
                        Some(m) => m.as_market(),
                        None => match shape {
                            CodeShape::Single(qm) => qm.as_market(),
                            _ => Market::Unknown,
                        },
                    },
                }),
                Err(e) => StockCreateRoute::Reject(e),
            }
        }
        CodeShape::Ambiguous => StockCreateRoute::Generic,
    }
}

pub fn adopt_stock_quote(
    conn: &rusqlite::Connection,
    kind: InstrumentType,
    quote: &Quote,
) -> Result<StockCreateOutcome> {
    let market = quote.stock_market()?;
    let outcome = adopt_quote(
        conn,
        &QuoteAdoptionInput {
            kind,
            market,
            currency_code: derive_quote_currency(market),
            priced_at: &now_iso(),
            nav_date: None,
        },
        quote,
    )?;
    Ok(StockCreateOutcome {
        instrument_id: outcome.instrument_id,
        price_written: outcome.price_written,
    })
}

pub fn create_stock_degraded(
    conn: &rusqlite::Connection,
    kind: InstrumentType,
    market: Market,
    code: &str,
    ai_name: Option<String>,
) -> Result<StockCreateOutcome> {
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            params![code, kind],
            |r| r.get(0),
        )
        .ok();
    if let Some(instrument_id) = existing_id {
        return Ok(StockCreateOutcome {
            instrument_id,
            price_written: false,
        });
    }
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: code.to_string(),
            kind,
            name: ai_name,
            currency_code: derive_quote_currency(market).to_string(),
            market: Some(market.as_str().to_string()),
        },
    )?;
    Ok(StockCreateOutcome {
        instrument_id,
        price_written: false,
    })
}
