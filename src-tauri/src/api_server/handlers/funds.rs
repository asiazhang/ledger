use axum::Json;
use axum::extract::{Path, State};
use utoipa::ToSchema;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ApiState;
use ledger_infra::error::AppError;
use ledger_investment::Quote;
use ledger_investment::validate_fund_code;

pub async fn fetch_fund_quote_for_api(state: &ApiState, code: &str) -> Result<Quote, AppError> {
    match &state.fund_fetch {
        Some(fetch) => fetch(code).await,
        None => ledger_market_sync::fetch_fund_quote_production(code).await,
    }
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct FundLookup {
    code: String,
    name: String,
    fund_class: String,
    nav_cents: Option<i64>,
    nav_date: Option<String>,
}

impl From<Quote> for FundLookup {
    fn from(q: Quote) -> Self {
        Self {
            code: q.code,
            name: q.name,
            fund_class: q.fund_class.unwrap_or_default(),
            nav_cents: q.price_cents,
            nav_date: q.nav_date,
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/funds/{code}",
    tag = "funds",
    summary = "按 6 位代码查询场外基金（只读，实时）",
    description = "按 6 位代码查询场外基金：返回名称/最新净值/净值日期（万分之一元刻度；\
                  fund_class 已弃用恒空串）；已终止（清盘）基金同样可查；查无此码 400。\
                  基金申赎行拆解见导入知识「基金申赎」节。",
    params(
        ("code" = String, Path, description = "基金代码（6 位数字）")
    ),
    responses(
        (status = 200, description = "基金详情（名称/最新净值/净值日期；fund_class 弃用恒空串）", body = FundLookup),
        (status = 400, description = "代码格式非法（非 6 位数字）或查无此码", body = ErrorResponse),
        (status = 500, description = "行情源网络不可达等临时故障", body = ErrorResponse)
    )
)]
pub async fn lookup_fund_handler(
    State(state): State<ApiState>,
    Path(code): Path<String>,
) -> Result<Json<FundLookup>, AppError> {
    validate_fund_code(&code)?;
    let quote = fetch_fund_quote_for_api(&state, &code).await?;
    Ok(Json(FundLookup::from(quote)))
}
