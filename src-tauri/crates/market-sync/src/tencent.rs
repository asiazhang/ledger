use ledger_infra::error::{AppError, Result};
use ledger_investment::prices::TENCENT_PRICE_SOURCE;
use ledger_investment::{InstrumentType, Market, Quote, StockRoute};

use super::channels::{QuoteItem, QuoteQuery};
use super::http::{Pacer, RetryConfig, request_bytes_from_hosts};
use super::source_tail;

pub(super) const TENCENT_QUOTE_HOSTS: &[&str] = &["https://qt.gtimg.cn"];

pub(super) const TENCENT_QUOTE_PATH_PREFIX: &str = "/q=";

pub(super) const TENCENT_QUOTE_BATCH_SIZE: usize = 800;

#[derive(Debug, Clone, PartialEq)]
pub struct TencentQuote {
    pub code: String,
    pub name: String,
    pub price_cents: Option<i64>,
    pub price_date: Option<String>,
    pub market: Market,
    pub security_type: String,
    pub kind_hint: InstrumentType,
    pub currency_code: String,
}

impl TencentQuote {
    pub(super) fn into_quote_item(self) -> QuoteItem {
        QuoteItem {
            code: self.code,
            name: self.name,
            price_cents: self.price_cents,
            price_date: self.price_date,
        }
    }

    pub fn into_quote(self) -> Quote {
        Quote {
            code: self.code,
            name: self.name,
            price_cents: self.price_cents,
            price_date: self.price_date,
            market: Some(self.market),
            kind_hint: Some(self.kind_hint),
            fund_class: None,
            nav_date: None,
            constant_unit_price_cents: None,
            price_source: TENCENT_PRICE_SOURCE,
        }
    }
}

fn route_key_prefix(route: StockRoute) -> &'static str {
    match route {
        StockRoute::Sh => "sh",
        StockRoute::Sz => "sz",
        StockRoute::Hk => "hk",
        StockRoute::Us => "us",
    }
}

pub(super) fn tencent_query_key(route: StockRoute, code: &str) -> String {
    format!("{}{code}", route_key_prefix(route))
}

pub(super) fn detect_kind_hint(security_type: &str) -> InstrumentType {
    match security_type {
        "ETF" | "LOF" | "GP-ETF" => InstrumentType::Etf,
        _ => InstrumentType::Stock,
    }
}

pub(super) fn price_cents_from_decimal(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let (int_part, frac_part) = match raw.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (raw, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.bytes().all(|b| b.is_ascii_digit())
        || !frac_part.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let int_value: i64 = if int_part.is_empty() {
        0
    } else {
        int_part.parse().ok()?
    };
    let mut frac: String = frac_part.chars().take(4).collect();
    while frac.len() < 4 {
        frac.push('0');
    }
    let frac_value: i64 = frac.parse().ok()?;
    let cents = int_value.checked_mul(10_000)?.checked_add(frac_value)?;
    (cents > 0).then_some(cents)
}

pub(super) fn price_date_from_timestamp(raw: &str) -> Option<String> {
    let raw = raw.trim();
    for format in [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d",
        "%Y%m%d%H%M%S",
        "%Y%m%d",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d",
    ] {
        if let Ok(date) = chrono::NaiveDate::parse_from_str(raw, format) {
            return Some(date.format("%Y-%m-%d").to_string());
        }
    }
    None
}

use super::http::decode_gbk;

struct QuoteLayout {
    kind_index: usize,
    currency_index: usize,
    min_fields: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseKeyMarket {
    Sh,
    Sz,
    Hk,
    Us,
}

impl ResponseKeyMarket {
    const fn as_str(self) -> &'static str {
        match self {
            ResponseKeyMarket::Sh => "sh",
            ResponseKeyMarket::Sz => "sz",
            ResponseKeyMarket::Hk => "hk",
            ResponseKeyMarket::Us => "us",
        }
    }

    const fn precise_market(self) -> Option<Market> {
        match self {
            ResponseKeyMarket::Sh => Some(Market::Sh),
            ResponseKeyMarket::Sz => Some(Market::Sz),
            ResponseKeyMarket::Hk => Some(Market::Hk),
            ResponseKeyMarket::Us => None,
        }
    }
}

fn quote_layout(key: &str) -> Option<(ResponseKeyMarket, QuoteLayout)> {
    let market = if key.len() > 2 && key.starts_with("sh") {
        ResponseKeyMarket::Sh
    } else if key.len() > 2 && key.starts_with("sz") {
        ResponseKeyMarket::Sz
    } else if key.len() > 2 && key.starts_with("hk") {
        ResponseKeyMarket::Hk
    } else if key.len() > 2 && key.starts_with("us") {
        ResponseKeyMarket::Us
    } else {
        return None;
    };
    let layout = match market {
        ResponseKeyMarket::Sh | ResponseKeyMarket::Sz => QuoteLayout {
            kind_index: 61,
            currency_index: 82,
            min_fields: 88,
        },
        ResponseKeyMarket::Hk => QuoteLayout {
            kind_index: 63,
            currency_index: 75,
            min_fields: 78,
        },
        ResponseKeyMarket::Us => QuoteLayout {
            kind_index: 56,
            currency_index: 35,
            min_fields: 73,
        },
    };
    Some((market, layout))
}

const SOURCE: &str = "tencent-batch-quote";

pub(super) fn parse_tencent_quotes(body: &str) -> std::result::Result<Vec<TencentQuote>, String> {
    let mut quotes = Vec::new();
    let mut saw_statement = false;
    for (key, value) in quote_statements(body) {
        saw_statement = true;
        if key == "pv_none_match" {
            continue;
        }
        let Some((key_market, layout)) = quote_layout(key) else {
            return Err(format!("响应含非预期的报价键 {key}"));
        };
        let fields: Vec<&str> = value.split('~').collect();
        if fields.len() < layout.min_fields {
            return Err(format!(
                "{} 报价字段数 {} 少于布局下界 {}（疑似布局漂移或被截断）",
                key_market.as_str(),
                fields.len(),
                layout.min_fields
            ));
        }
        if let Some(quote) = build_quote(key_market, &fields, &layout)? {
            quotes.push(quote);
        }
    }
    if !saw_statement {
        return Err("响应不含任何报价语句（疑似被风控拦截）".to_string());
    }
    Ok(quotes)
}

fn build_quote(
    key_market: ResponseKeyMarket,
    fields: &[&str],
    layout: &QuoteLayout,
) -> std::result::Result<Option<TencentQuote>, String> {
    let name = field(fields, 1).trim();
    let echo_code = field(fields, 2).trim();
    if name.is_empty() || echo_code.is_empty() {
        return Ok(None);
    }
    let (code, market) = match key_market.precise_market() {
        Some(market) => (echo_code.to_string(), market),
        None => {
            let (ticker, exchange) = split_us_code(echo_code)?;
            (ticker.to_string(), us_market_from_suffix(exchange)?)
        }
    };
    let security_type = field(fields, layout.kind_index).trim().to_string();
    Ok(Some(TencentQuote {
        code,
        name: name.to_string(),
        price_cents: price_cents_from_decimal(field(fields, 3)),
        price_date: price_date_from_timestamp(field(fields, 30)),
        market,
        kind_hint: detect_kind_hint(&security_type),
        security_type,
        currency_code: field(fields, layout.currency_index).trim().to_string(),
    }))
}

fn field<'a>(fields: &[&'a str], index: usize) -> &'a str {
    fields.get(index).copied().unwrap_or("")
}

fn split_us_code(echo_code: &str) -> std::result::Result<(&str, &str), String> {
    echo_code
        .rsplit_once('.')
        .ok_or_else(|| format!("美股报价缺少交易所后缀：{echo_code}"))
}

fn us_market_from_suffix(suffix: &str) -> std::result::Result<Market, String> {
    match suffix {
        "OQ" => Ok(Market::Nasdaq),
        "N" => Ok(Market::Nyse),
        "AM" => Ok(Market::Amex),
        _ => Err(format!("美股报价含未知交易所后缀 .{suffix}")),
    }
}

fn quote_statements(body: &str) -> Vec<(&str, &str)> {
    let mut statements = Vec::new();
    let mut cursor = 0usize;
    while let Some(offset) = body[cursor..].find("v_") {
        let start = cursor + offset;
        let rest = &body[start + 2..];
        let Some(eq) = rest.find('=') else { break };
        let key = rest[..eq].trim();
        let Some(value) = rest[eq + 1..].strip_prefix('"') else {
            cursor = start + 2;
            continue;
        };
        let Some(end) = value.find('"') else { break };
        statements.push((key, &value[..end]));
        cursor = start + 2 + eq + 2 + end + 1;
    }
    statements
}

pub(super) async fn fetch_tencent_quotes(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    queries: &[QuoteQuery],
) -> Result<Vec<TencentQuote>> {
    let mut quotes = Vec::new();
    for chunk in queries.chunks(TENCENT_QUOTE_BATCH_SIZE) {
        let keys: Vec<String> = chunk
            .iter()
            .map(|query| tencent_query_key(query.market.as_stock_route(), &query.code))
            .collect();
        quotes.extend(fetch_tencent_batch(client, pacer, hosts, &keys.join(",")).await?);
    }
    Ok(quotes)
}

pub(super) async fn fetch_tencent_batch(
    client: &reqwest::Client,
    pacer: &mut Pacer,
    hosts: &[&str],
    keys: &str,
) -> Result<Vec<TencentQuote>> {
    tracing::debug!(keys, "腾讯行情批量报价查询");
    let path = format!("{TENCENT_QUOTE_PATH_PREFIX}{keys}");
    let bytes = request_bytes_from_hosts(
        client,
        &[],
        &path,
        hosts,
        RetryConfig::production(),
        pacer,
        "fetch_tencent_quotes",
        None,
    )
    .await?;
    source_tail::finish(
        SOURCE,
        &bytes,
        pacer,
        |response| decode_gbk(response).map_err(|error| error.to_string()),
        parse_tencent_quotes,
        quote_source_malformed,
    )
}

fn quote_source_malformed() -> AppError {
    AppError::coded(
        "sync.quote-source-malformed",
        "腾讯行情报价数据源返回了无法解析的内容，请稍后重试同步",
    )
}
