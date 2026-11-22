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
#[schema(
    description = "基金查询响应（`GET /api/v1/funds/{code}`，issue #304 / ADR-0039 决策 2 / #1568 换源）：统一报价（行情接入载荷，ADR-0103）的场外通道投影——净值已是万分之一元价格刻度（换算在访问层，ADR-0038），AI 供校验「代码 → 名称」映射与查最新净值。已终止（清盘）基金同样可查（官方披露兜底，不再误报查无此码）。"
)]
pub struct FundLookup {
    #[doc = "基金代码（6 位数字）"]
    code: String,
    #[doc = "数据源权威名称（如「华夏成长混合」）"]
    name: String,
    #[doc = "已弃用，恒返回空串：基金分类在替代源无来源（ADR-0130 决策 8），字段保留为已发布契约的一部分，展示层可不渲染。"]
    fund_class: String,
    #[doc = "最新单位净值（万分之一元，元 × 10000，ADR-0038 价格刻度）；未公布为 null"]
    nav_cents: Option<i64>,
    #[doc = "净值日期（ISO 日期）；未公布为 null"]
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
