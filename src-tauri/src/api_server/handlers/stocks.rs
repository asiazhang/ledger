use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ApiState;
use ledger_infra::error::AppError;
use ledger_investment::{
    InstrumentType, Quote, StockRoute, derive_quote_currency, explicit_quote_market,
    resolve_stock_code,
};

pub async fn fetch_stock_quote_for_api(
    state: &ApiState,
    route: StockRoute,
    code: &str,
) -> Result<Quote, AppError> {
    match &state.stock_fetch {
        Some(fetch) => fetch(route, code).await,
        None => ledger_market_sync::fetch_stock_quote_production(route, code).await,
    }
}

#[derive(Debug, Deserialize)]
pub struct StockLookupQuery {
    market: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct StockLookup {
    code: String,
    name: String,
    market: String,
    currency_code: String,
    price_cents: Option<i64>,
    price_date: Option<String>,
    kind_hint: InstrumentType,
}

impl TryFrom<Quote> for StockLookup {
    type Error = AppError;

    fn try_from(q: Quote) -> Result<Self, AppError> {
        let market = q.stock_market()?;
        let kind_hint = q.stock_kind_hint();
        Ok(Self {
            currency_code: derive_quote_currency(market).to_string(),
            code: q.code,
            name: q.name,
            market: market.as_str().to_string(),
            price_cents: q.price_cents,
            price_date: q.price_date,
            kind_hint,
        })
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/stocks/{code}",
    tag = "stocks",
    summary = "按代码查询股票实时行情（只读，实时，沪深港美）",
    description = "按代码实时查询股票（沪深港美）：返回权威名称、精确市场、币种、最新价（\
                  万分之一元）与类型提示（stock/etf）；`market` 可选、缺省按代码形态推断（美股 \
                  单次查询，精确交易所由行情源自报）；查无此码与北交所代码均显式 400。三步法见\
                  导入知识「投资交易」节。",
    params(
        ("code" = String, Path, description = "股票代码（沪深 6 位数字 / 港股 5 位及以下数字 / 美股字母 ticker，大小写不敏感）"),
        ("market" = Option<String>, Query, description = "交易市场（可选：sh/sz/hk/nasdaq/nyse/amex；缺省按代码形态解析；美股三值同解，精确交易所由行情源自报）")
    ),
    responses(
        (status = 200, description = "股票行情（名称/精确市场/币种/最新价/价格日期/类型提示）", body = StockLookup),
        (status = 400, description = "北交所代码暂不支持；代码形态无法推断；market 与代码形态矛盾或不在支持闭集；查无此码", body = ErrorResponse),
        (status = 500, description = "行情网络不可达等临时故障", body = ErrorResponse)
    )
)]
pub async fn lookup_stock_handler(
    State(state): State<ApiState>,
    Path(code): Path<String>,
    Query(query): Query<StockLookupQuery>,
) -> Result<Json<StockLookup>, AppError> {
    let explicit = query
        .market
        .as_deref()
        .map(explicit_quote_market)
        .transpose()?;
    let candidate = resolve_stock_code(explicit, &code)?;
    let quote = fetch_stock_quote_for_api(&state, candidate.route, &candidate.code).await?;
    Ok(Json(StockLookup::try_from(quote)?))
}
