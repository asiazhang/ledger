//! 场外基金端点：按 6 位代码查询（东财实时）与两端点共用的东财报价获取接缝。

use axum::Json;
use axum::extract::{Path, State};
use utoipa::ToSchema;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ApiState;
use crate::error::AppError;
use crate::investment::Quote;
use crate::investment::validate_fund_code;

/// 东财基金报价获取（查询与创建两端点共用，issue #304）：测试注入桩直接同步
/// 调用（离线驱动）；生产路径经 `spawn_blocking` 在连接锁外完成阻塞网络往返
/// （单请求叠加限流冷却重试最长可达分钟级，先例：`add_fund_by_code` 命令的
/// 网络拉取在锁外完成，不阻塞其它命令）。
pub async fn fetch_fund_quote_for_api(state: &ApiState, code: &str) -> Result<Quote, AppError> {
    match &state.fund_fetch {
        Some(fetch) => fetch(code),
        None => {
            let code = code.to_string();
            tauri::async_runtime::spawn_blocking(move || {
                crate::sync::fetch_fund_quote_production(&code)
            })
            .await
            .map_err(|e| AppError::Io(format!("基金详情查询任务执行失败: {e}")))?
        }
    }
}

/// 基金查询响应（`GET /api/v1/funds/{code}`，issue #304 / ADR-0039 决策 2）：
/// 统一报价（行情接入载荷，ADR-0103）的场外通道投影——净值已是万分之一元价格
/// 刻度（换算在访问层，ADR-0038），AI 供校验「代码 → 名称」映射与查最新净值。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct FundLookup {
    /// 基金代码（6 位数字）
    code: String,
    /// 东财权威名称（如「华夏成长混合」）
    name: String,
    /// 东财基金分类（如「混合型-灵活」）
    fund_class: String,
    /// 最新单位净值（万分之一元，元 × 10000，ADR-0038 价格刻度）；未公布为 null
    nav_cents: Option<i64>,
    /// 净值日期（ISO 日期）；未公布为 null
    nav_date: Option<String>,
}

impl From<Quote> for FundLookup {
    fn from(q: Quote) -> Self {
        // 净值对（值 + 日期）在东财访问层已保证成对出现（任一缺省即价格 = None）。
        Self {
            code: q.code,
            name: q.name,
            // 场外通道成员：基金分类缺省为空串（源数据分类缺省时同为空白，行为不变）。
            fund_class: q.fund_class.unwrap_or_default(),
            nav_cents: q.price_cents,
            nav_date: q.nav_date,
        }
    }
}

/// 按代码查询场外基金（AI 导入契约，issue #304 / ADR-0039 决策 2）：只读，
/// 实时从东方财富取名称、基金类型、最新单位净值与净值日期，供 AI 校验「代码 →
/// 名称」映射与查净值。代码格式非法即刻拒绝不发起网络；查无此码返回中文错误，
/// AI 可提示用户或跳过该行。
#[utoipa::path(
    get,
    path = "/api/v1/funds/{code}",
    tag = "funds",
    summary = "按 6 位代码查询场外基金（只读，东财实时）",
    description = "按 6 位代码查询场外基金：返回名称/东财分类/最新净值/净值日期（\
                  万分之一元刻度）；查无此码 400。基金申赎行拆解见导入知识「基金申赎」节。",
    params(
        ("code" = String, Path, description = "基金代码（6 位数字）")
    ),
    responses(
        (status = 200, description = "基金详情（名称/分类/最新净值/净值日期）", body = FundLookup),
        (status = 400, description = "代码格式非法（非 6 位数字）或查无此码", body = ErrorResponse),
        (status = 500, description = "东财网络不可达等临时故障", body = ErrorResponse)
    )
)]
pub async fn lookup_fund_handler(
    State(state): State<ApiState>,
    Path(code): Path<String>,
) -> Result<Json<FundLookup>, AppError> {
    // 格式非法即刻拒绝，不发起网络请求（与按代码即拉同一校验、同一中文错误）。
    validate_fund_code(&code)?;
    let quote = fetch_fund_quote_for_api(&state, &code).await?;
    Ok(Json(FundLookup::from(quote)))
}
