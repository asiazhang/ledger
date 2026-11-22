use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use serde::Deserialize;

use crate::api_server::error::ErrorResponse;
use crate::api_server::handlers::funds::fetch_fund_quote_for_api;
use crate::api_server::handlers::stocks::fetch_stock_quote_for_api;
use crate::api_server::state::{ApiState, ReadConn};
use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::error::AppError;
use ledger_infra::signals::{WriteEvidence, WriteOp};
use ledger_investment::{
    InstrumentInput, InstrumentListFilter, InstrumentListResult, InstrumentType, Market, Quote,
    StockCreateRoute, adopt_fund_quote, adopt_stock_quote, create_fund_degraded,
    create_stock_degraded, derive_quote_currency, is_six_digit_code, reject_carried_fund_market,
    route_stock_creation,
};

#[derive(Debug, Deserialize)]
pub struct InstrumentSearchQuery {
    query: Option<String>,
    limit: Option<i64>,
    market: Option<String>,
    #[serde(rename = "type")]
    kind: Option<InstrumentType>,
}

#[utoipa::path(
    get,
    path = "/api/v1/instruments",
    tag = "instruments",
    summary = "按关键词搜索标的（统一模糊搜索、封顶返回）",
    description = "按关键词统一模糊搜索标的（`query` 必填、按 symbol 排序封顶返回，`limit` \
                  缺省 20、上限 100）：作名称消歧的辅助，非带代码行的解析前提，见导入知识「\
                  投资交易」节。",
    params(
        ("query" = String, Query, description = "搜索关键词（必填，空即 400）"),
        ("limit" = Option<i64>, Query, description = "返回条数上限，缺省 20，最大 100（小于 1 视为 1）"),
        ("market" = Option<String>, Query, description = "交易市场精确过滤（sh / sz / hk / nasdaq / nyse / amex / unknown）"),
        ("type" = InstrumentType, Query, description = "标的类型过滤（stock/fund/bond/etf/other），同码异类型消歧用")
    ),
    responses(
        (status = 200, description = "命中标的 {items, total}", body = InstrumentListResult),
        (status = 400, description = "缺 query 或 query 为纯空白；或参数非法", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn search_instruments_handler(
    State(read): State<ReadConn>,
    Query(params): Query<InstrumentSearchQuery>,
) -> Result<Json<InstrumentListResult>, AppError> {
    let query = params
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or_else(|| {
            AppError::coded(
                "instrument.query-required",
                "query 不能为空：标的搜索为搜索式端点，请携带关键词（不做全量列表）",
            )
        })?;
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let filter = InstrumentListFilter {
        search: Some(query.to_string()),
        market: params.market.filter(|m| !m.is_empty()),
        kind: params.kind,
        only_invested: None,
        page: Some(1),
        page_size: Some(limit as usize),
        anchor_id: None,
    };
    read_entry("GET /api/v1/instruments", read.0, move |conn| {
        Ok(Json(ledger_investment::list_instruments(conn, &filter)?))
    })
    .await
}

#[derive(Debug, Deserialize, utoipa::ToSchema)]
#[schema(
    description = "标的创建请求体（`POST /api/v1/instruments`，issue #296 / ADR-0037）。\n\n与 IPC 侧 `InstrumentInput` 的差异仅在报价币种可省：缺省按市场推导（沪深→CNY、港→HKD、美股三市场→USD、未知→CNY，ADR-0081），显式传参可覆盖。"
)]
pub struct InstrumentCreateInput {
    #[doc = "标的代码（必填；源数据只有名称时以名称充当代码，ADR-0037 决策 3）"]
    symbol: String,
    #[serde(rename = "type")]
    #[doc = "标的类型（闭集：stock/fund/bond/etf/other；五类全开，不经自建标的的 UI 白名单）"]
    kind: InstrumentType,
    #[doc = "标的名称（可选）"]
    name: Option<String>,
    #[doc = "交易市场（可选，缺省 unknown；sh / sz / hk / nasdaq / nyse / amex）。fund 类型市场恒 unknown（场外基金无交易所市场概念，ADR-0038）：携带非 unknown 市场即 400（`instrument.fund-market-forbidden`）。"]
    market: Option<String>,
    #[doc = "报价币种（可选；缺省按市场推导：沪深→CNY、港→HKD、美股三市场→USD、未知→CNY）"]
    currency_code: Option<String>,
}

#[utoipa::path(
    post,
    path = "/api/v1/instruments",
    tag = "instruments",
    summary = "创建标的（按（代码，类型）幂等 find-or-create，fund/stock 类型经行情源增强）",
    description = "按（symbol + type）幂等 find-or-create 标的：同码同类型静默复用返回既有 id（\
                  201 + 裸 id 字符串）；fund/stock 类型经行情源校验回填权威名称与最新价，\
                  查无此码 400、临时不可达降级建行。标的解析三步法与降级细节见导入知识「投资交易」「\
                  基金申赎」节。",
    request_body = InstrumentCreateInput,
    responses(
        (status = 201, description = "创建或命中复用，返回标的 ID", body = String),
        (status = 400, description = "参数错误（如标的代码为空）", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn create_instrument_handler(
    State(state): State<ApiState>,
    Json(input): Json<InstrumentCreateInput>,
) -> Result<(StatusCode, Json<String>), AppError> {
    enum Enrichment {
        FundAuthoritative(Quote),
        FundDegrade,
        StockAuthoritative {
            kind: InstrumentType,
            quote: Quote,
        },
        StockDegrade {
            kind: InstrumentType,
            market: Market,
            code: String,
        },
    }
    if input.kind == InstrumentType::Fund {
        reject_carried_fund_market(input.market.as_deref())?;
    }
    let enrichment: Option<Enrichment> = match input.kind {
        InstrumentType::Fund if is_six_digit_code(&input.symbol) => Some(
            match fetch_fund_quote_for_api(&state, &input.symbol).await {
                Ok(quote) => Enrichment::FundAuthoritative(quote),
                Err(e) if e.is_code("sync.fund-not-found") => return Err(e),
                Err(_) => Enrichment::FundDegrade,
            },
        ),
        InstrumentType::Stock | InstrumentType::Etf => {
            match route_stock_creation(input.market.as_deref(), &input.symbol) {
                StockCreateRoute::Enhance(plan) => Some(
                    match fetch_stock_quote_for_api(
                        &state,
                        plan.candidate.route,
                        &plan.candidate.code,
                    )
                    .await
                    {
                        Ok(quote) => Enrichment::StockAuthoritative {
                            kind: input.kind,
                            quote,
                        },
                        Err(e) if e.is_code("sync.stock-not-found") => return Err(e),
                        Err(_) => Enrichment::StockDegrade {
                            kind: input.kind,
                            market: plan.degrade_market,
                            code: plan.candidate.code,
                        },
                    },
                ),
                StockCreateRoute::Reject(e) => return Err(e),
                StockCreateRoute::Generic => None,
            }
        }
        _ => None,
    };
    let currency_code = match input.currency_code {
        Some(currency_code) => currency_code,
        None => derive_quote_currency(match input.market.as_deref() {
            Some(m) => Market::parse(m)?,
            None => Market::Unknown,
        })
        .to_string(),
    };
    let instrument_id = write_entry(
        "POST /api/v1/instruments",
        state.write_handle(),
        state.emitter.as_deref(),
        WriteOp::CreateInstrument,
        move |conn| {
            let (instrument_id, price_written) = match &enrichment {
                Some(Enrichment::FundAuthoritative(quote)) => {
                    let r = adopt_fund_quote(conn, quote)?;
                    (r.instrument_id, r.price_written)
                }
                Some(Enrichment::FundDegrade) => {
                    let r = create_fund_degraded(conn, &input.symbol, input.name.clone())?;
                    (r.instrument_id, r.price_written)
                }
                Some(Enrichment::StockAuthoritative { kind, quote }) => {
                    let r = adopt_stock_quote(conn, *kind, quote)?;
                    (r.instrument_id, r.price_written)
                }
                Some(Enrichment::StockDegrade { kind, market, code }) => {
                    let r = create_stock_degraded(conn, *kind, *market, code, input.name.clone())?;
                    (r.instrument_id, r.price_written)
                }
                None => {
                    let generic_input = InstrumentInput {
                        symbol: input.symbol.clone(),
                        kind: input.kind,
                        name: input.name.clone(),
                        currency_code,
                        market: input.market.clone(),
                    };
                    (
                        ledger_investment::create_instrument(conn, generic_input)?,
                        false,
                    )
                }
            };
            let evidence = WriteEvidence::PriceWritten(price_written);
            Ok(Outcome::Evidenced(instrument_id, evidence))
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(instrument_id)))
}
