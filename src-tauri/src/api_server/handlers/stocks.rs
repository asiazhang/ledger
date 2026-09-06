//! 股票端点：按（市场，代码）查询东财实时行情（沪深港，issue #693 / ADR-0081
//! 决策 1）——与基金查询端点同构的东财行情获取接缝（查询端点与后续创建增强、
//! 添加投资标的壳共用）。

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ApiState;
use crate::error::AppError;
use crate::investment::{InstrumentType, StockQuote, derive_quote_currency, resolve_stock_market};

/// 东财股票行情获取（查询端点与后续创建增强、添加投资标的壳共用，issue #693）：
/// 测试注入桩直接同步调用（离线驱动）；生产路径经 `spawn_blocking` 在连接锁外
/// 完成阻塞网络往返（单请求叠加限流冷却重试最长可达分钟级，先例：
/// `fetch_fund_detail_for_api`，网络往返不进连接锁）。
pub async fn fetch_stock_quote_for_api(
    state: &ApiState,
    market: &str,
    code: &str,
) -> Result<StockQuote, AppError> {
    match &state.stock_fetch {
        Some(fetch) => fetch(market, code),
        None => {
            let market = market.to_string();
            let code = code.to_string();
            tauri::async_runtime::spawn_blocking(move || {
                crate::sync::fetch_stock_quote_production(&market, &code)
            })
            .await
            .map_err(|e| AppError::Io(format!("股票行情查询任务执行失败: {e}")))?
        }
    }
}

/// 股票查询参数：`market` 可选——缺省按代码形态单点推断
///（见 `investment::stock::resolve_stock_market`）。
#[derive(Debug, Deserialize)]
pub struct StockLookupQuery {
    /// 交易市场（可选；sh / sz / hk，本端点当前支持沪深港）
    market: Option<String>,
}

/// 股票查询响应（`GET /api/v1/stocks/{code}`，issue #693 / ADR-0081 决策 1）：
/// 投影对齐基金查询（FundLookup）——代码、东财权威名称、精确市场、币种、
/// 最新价（万分之一元价格刻度）、价格日期、类型提示。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct StockLookup {
    /// 归一化代码（港股左补零至 5 位，如 700 → "00700"）
    code: String,
    /// 东财权威名称（如「贵州茅台」）
    name: String,
    /// 精确市场（sh / sz / hk）
    market: String,
    /// 报价币种（按市场推导：沪深→CNY、港→HKD，ADR-0037 决策 2）
    currency_code: String,
    /// 最新价（万分之一元，元 × 10000，ADR-0038 价格刻度）；停牌/无有效报价为 null
    price_cents: Option<i64>,
    /// 价格日期（最新价的北京日历日，ISO 日期）；无有效时间戳为 null
    price_date: Option<String>,
    /// 类型提示（stock 股票 / etf 场内基金类；东财类型特征字段单点探测，ADR-0081）
    kind_hint: InstrumentType,
}

impl From<StockQuote> for StockLookup {
    fn from(q: StockQuote) -> Self {
        Self {
            currency_code: derive_quote_currency(&q.market).to_string(),
            code: q.code,
            name: q.name,
            market: q.market,
            price_cents: q.price_cents,
            price_date: q.price_date,
            kind_hint: q.kind_hint,
        }
    }
}

/// 按代码查询股票实时行情（AI 导入契约，issue #693 / ADR-0081 决策 1）：只读，
/// 实时从东方财富取权威名称、精确市场、最新价与类型提示，供 AI 校验「代码 →
/// 名称」映射与核对迁移标的。market 缺省按代码形态单点推断（沪深 6 位、港 5 位
/// 补零）；全部参数类拒绝路径在发起网络请求前返回；查无此码返回中文错误，AI
/// 可提示用户或跳过该行。
#[utoipa::path(
    get,
    path = "/api/v1/stocks/{code}",
    tag = "stocks",
    summary = "按代码查询股票实时行情（只读，东财实时，沪深港）",
    description = "返回东财实时行情：`code`（归一化代码，港股左补零至 5 位）/ `name`（东财权威名称）\
                  / `market`（精确市场）/ `currency_code`（币种：沪深→CNY、港→HKD）\
                  / `price_cents`（最新价，万分之一元）/ `price_date`（价格日期，ISO 日期）\
                  / `kind_hint`（类型提示：stock=股票、etf=场内基金类）。\
                  停牌或无有效报价时 `price_cents` / `price_date` 为 null。\
                  `market` 为可选查询参数：缺省按代码形态单点推断（6 位 6 开头→沪 sh、0/3 开头→深 sz、\
                  5 开头→沪 / 1 开头→深的场内基金段（ETF/LOF）、5 位及以下数字→港 hk 左补零归一）；\
                  显式传参须与代码形态一致。北交所代码（4/8 开头）暂不支持、无法推断的代码形态、\
                  `market` 与代码形态矛盾均返回 400 中文错误；查无此码返回 400 中文错误；\
                  本端点实时访问东方财富，网络故障返回 500。\
                  股票迁移先按本端点确认识别（名称/市场/最新价/类型提示），\
                  再以真实代码与精确市场创建标的，不走名称充代码。",
    params(
        ("code" = String, Path, description = "股票代码（沪深 6 位数字 / 港股 5 位及以下数字）"),
        ("market" = Option<String>, Query, description = "交易市场（可选：sh/sz/hk；缺省按代码形态推断）")
    ),
    responses(
        (status = 200, description = "股票行情（名称/精确市场/币种/最新价/价格日期/类型提示）", body = StockLookup),
        (status = 400, description = "北交所代码暂不支持；代码形态无法推断；market 与代码形态矛盾；查无此码", body = ErrorResponse),
        (status = 500, description = "东财网络不可达等临时故障", body = ErrorResponse)
    )
)]
pub async fn lookup_stock_handler(
    State(state): State<ApiState>,
    Path(code): Path<String>,
    Query(query): Query<StockLookupQuery>,
) -> Result<Json<StockLookup>, AppError> {
    // 形态解析（推断 / 矛盾 / 不支持 / 北交所）在发起网络前完成：非法参数即刻 400。
    let resolved = resolve_stock_market(query.market.as_deref(), &code)?;
    let quote = fetch_stock_quote_for_api(&state, resolved.market, &resolved.code).await?;
    Ok(Json(StockLookup::from(quote)))
}
