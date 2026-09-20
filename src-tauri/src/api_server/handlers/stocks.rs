//! 股票端点：按（市场，代码）查询腾讯实时行情（沪深港美，issue #693/#696 /
//! ADR-0081 决策 1/2；换源 ADR-0130 决策 2 / issue #1567）——与基金查询端点
//! 同构的行情获取接缝（查询端点与创建增强、添加投资标的壳共用）。

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ApiState;
use ledger_infra::error::AppError;
use ledger_investment::{InstrumentType, Quote, derive_quote_currency};

/// 股票行情获取（查询端点与创建增强、添加投资标的壳共用，issue #693）：
/// 测试注入桩直接在异步上下文 await（离线驱动）；生产路径为 async 生产入口
/// 直接 `await`（连接锁外完成网络往返，单请求叠加限流冷却重试最长可达分钟级，
/// 先例：`fetch_fund_quote_for_api`，网络往返不进连接锁；async 形态 ADR-0125
/// 决策 7 / issue #1413，`spawn_blocking` 包装与 JoinError 归一化删除）。
pub async fn fetch_stock_quote_for_api(
    state: &ApiState,
    market: &str,
    code: &str,
) -> Result<Quote, AppError> {
    match &state.stock_fetch {
        Some(fetch) => fetch(market, code).await,
        None => ledger_market_sync::fetch_stock_quote_production(market, code).await,
    }
}

/// 股票查询参数：`market` 可选——缺省按代码形态单点解析
///（见 `investment::stock::resolve_stock_code`）。
#[derive(Debug, Deserialize)]
pub struct StockLookupQuery {
    /// 交易市场（可选；sh / sz / hk / nasdaq / nyse / amex）
    market: Option<String>,
}

/// 股票查询响应（`GET /api/v1/stocks/{code}`，issue #693/#696 / ADR-0081 决策 1/2）：
/// 投影对齐基金查询（FundLookup）——代码、数据源权威名称、精确市场、币种、
/// 最新价（万分之一元价格刻度）、价格日期、类型提示。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct StockLookup {
    /// 归一化代码（港股左补零至 5 位，美股大写，如 aapl → "AAPL"）
    code: String,
    /// 数据源权威名称（如「贵州茅台」）
    name: String,
    /// 精确市场（sh / sz / hk / nasdaq / nyse / amex；美股由行情源自报交易所归属）
    market: String,
    /// 报价币种（按市场推导：沪深→CNY、港→HKD、美股→USD，ADR-0037 决策 2 / ADR-0081）
    currency_code: String,
    /// 最新价（万分之一元，元 × 10000，ADR-0038 价格刻度）；停牌/无有效报价为 null
    price_cents: Option<i64>,
    /// 价格日期（交易所当地交易日，ISO 日期，ADR-0130 决策 5）；无有效时间戳为 null
    price_date: Option<String>,
    /// 类型提示（stock 股票 / etf 场内基金类；行情类型码单点探测，ADR-0081 判据 /
    /// ADR-0130 决策 3）
    kind_hint: InstrumentType,
}

impl TryFrom<Quote> for StockLookup {
    type Error = AppError;

    fn try_from(q: Quote) -> Result<Self, AppError> {
        // 场内通道强约束在投资域单点判定（缺市场即内部不一致的码化拒绝，
        // ADR-0103 决策 2：通道差异从类型形状退到通道内判定）。
        let market = q.stock_market()?.to_string();
        let kind_hint = q.stock_kind_hint();
        Ok(Self {
            currency_code: derive_quote_currency(&market).to_string(),
            code: q.code,
            name: q.name,
            market,
            price_cents: q.price_cents,
            price_date: q.price_date,
            kind_hint,
        })
    }
}

/// 按代码查询股票实时行情（AI 导入契约，issue #693/#696 / ADR-0081 决策 1/2）：
/// 只读，实时从行情源取权威名称、精确市场、最新价与类型提示，供 AI 校验「代码 →
/// 名称」映射与核对迁移标的。market 缺省按代码形态单点解析（沪深 6 位、港 5 位
/// 补零、美股字母 ticker 单次查询——精确交易所由行情源自报，ADR-0130 决策 2）；全部
/// 参数类拒绝路径在发起网络请求前返回；查无此码返回中文错误，AI 可提示用户或
/// 跳过该行。
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
    // 形态解析（推断 / 矛盾 / 不支持 / 北交所）在发起网络前完成：非法参数即刻 400。
    let candidate = ledger_investment::resolve_stock_code(query.market.as_deref(), &code)?;
    let quote = fetch_stock_quote_for_api(&state, candidate.market, &candidate.code).await?;
    Ok(Json(StockLookup::try_from(quote)?))
}
