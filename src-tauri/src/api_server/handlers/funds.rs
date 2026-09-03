//! 场外基金端点：按 6 位代码查询（东财实时）与两端点共用的东财详情获取接缝。

use axum::Json;
use axum::extract::{Path, State};
use utoipa::ToSchema;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ApiState;
use crate::error::AppError;
use crate::investment::prices::price_value_to_cents;
use crate::investment::validate_fund_code;
use crate::sync::FundDetail;

/// 东财基金详情获取（查询与创建两端点共用，issue #304）：测试注入桩直接同步
/// 调用（离线驱动）；生产路径经 `spawn_blocking` 在连接锁外完成阻塞网络往返
/// （单请求叠加限流冷却重试最长可达分钟级，先例：`add_fund_by_code` 命令的
/// 网络拉取在锁外完成，不阻塞其它命令）。
pub async fn fetch_fund_detail_for_api(
    state: &ApiState,
    code: &str,
) -> Result<FundDetail, AppError> {
    match &state.fund_fetch {
        Some(fetch) => fetch(code),
        None => {
            let code = code.to_string();
            tauri::async_runtime::spawn_blocking(move || {
                crate::sync::fetch_fund_detail_production(&code)
            })
            .await
            .map_err(|e| AppError::Io(format!("基金详情查询任务执行失败: {e}")))?
        }
    }
}

/// 基金查询响应（`GET /api/v1/funds/{code}`，issue #304 / ADR-0039 决策 2）：
/// 东财详情投影为 API 价格刻度（净值 万分之一元），AI 供校验「代码 → 名称」
/// 映射与查最新净值。
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

impl From<FundDetail> for FundLookup {
    fn from(d: FundDetail) -> Self {
        // 净值对（值 + 日期）在东财访问层已保证成对出现（任一缺省即 nav = None）。
        Self {
            code: d.code,
            name: d.name,
            fund_class: d.fund_class,
            nav_cents: d.nav.as_ref().map(|n| price_value_to_cents(n.nav)),
            nav_date: d.nav.map(|n| n.nav_date),
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
    description = "返回东财实时详情：`code` / `name`（权威名称）/ `fund_class`（东财基金分类，\
                  如「混合型-灵活」）/ `nav_cents`（最新单位净值，万分之一元，元 × 10000）/\
                  `nav_date`（净值日期，ISO 日期）；基金未公布净值时后两字段为 null。\
                  `code` 必须为 6 位数字（非 6 位返回 400，不发起网络请求）；查无此码返回 400 中文错误。\
                  本端点实时访问东方财富，网络故障返回 500。\
                  基金申赎迁移时先按本端点确认识别，再以真实 6 位代码创建标的\
                  （见 `POST /api/v1/instruments` 的 fund 增强与导入知识「基金申赎」节），\
                  不走名称充代码。",
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
    let detail = fetch_fund_detail_for_api(&state, &code).await?;
    Ok(Json(FundLookup::from(detail)))
}
