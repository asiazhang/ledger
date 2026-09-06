//! 股票端点：按（市场，代码）查询东财实时行情（沪深港美，issue #693/#696 /
//! ADR-0081 决策 1/2）——与基金查询端点同构的东财行情获取接缝（查询端点与
//! 创建增强、添加投资标的壳共用）。

use axum::Json;
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use utoipa::ToSchema;

use crate::api_server::error::ErrorResponse;
use crate::api_server::state::ApiState;
use crate::error::AppError;
use crate::investment::{
    InstrumentType, ResolvedStockCode, StockQuote, derive_quote_currency, is_stock_lookup_miss,
    resolve_stock_quote_candidates,
};

/// 东财股票行情获取（查询端点与创建增强、添加投资标的壳共用，issue #693）：
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

/// 按候选序查询首个命中（issue #696 / ADR-0081 决策 1）：候选序由解析单点
///（`resolve_stock_quote_candidates`）决定，「哪些错误算未命中」由域谓词
///（`is_stock_lookup_miss`）决定，本助手只执行遍历——未命中继续下一候选，
/// 临时错误立即上抛（不盲试剩余候选），全部候选未命中时报查无此码。
/// 查询端点与创建增强共用同一遍历形状（spec #690 唯一接缝纪律）。
pub async fn fetch_stock_quote_first_hit_for_api(
    state: &ApiState,
    candidates: &[ResolvedStockCode],
) -> Result<StockQuote, AppError> {
    let mut last_miss: Option<AppError> = None;
    for candidate in candidates {
        match fetch_stock_quote_for_api(state, candidate.market, &candidate.code).await {
            Ok(quote) => return Ok(quote),
            Err(e) if is_stock_lookup_miss(&e) => last_miss = Some(e),
            Err(e) => return Err(e),
        }
    }
    // 候选非空由解析单点保证（每个形态分支至少产出一个候选）；未命中错误在
    // 全不命中时必有值。unwrap 不可用（ADR-0060），以码化内部不一致兌底。
    match last_miss {
        Some(e) => Err(e),
        None => Err(AppError::codedp(
            "sync.secid-unroutable",
            "候选列表为空（内部不一致）",
            &[],
        )),
    }
}

/// 股票查询参数：`market` 可选——缺省按代码形态单点解析
///（见 `investment::stock::resolve_stock_quote_candidates`）。
#[derive(Debug, Deserialize)]
pub struct StockLookupQuery {
    /// 交易市场（可选；sh / sz / hk / nasdaq / nyse / amex）
    market: Option<String>,
}

/// 股票查询响应（`GET /api/v1/stocks/{code}`，issue #693/#696 / ADR-0081 决策 1/2）：
/// 投影对齐基金查询（FundLookup）——代码、东财权威名称、精确市场、币种、
/// 最新价（万分之一元价格刻度）、价格日期、类型提示。
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct StockLookup {
    /// 归一化代码（港股左补零至 5 位，美股大写，如 aapl → "AAPL"）
    code: String,
    /// 东财权威名称（如「贵州茅台」）
    name: String,
    /// 精确市场（sh / sz / hk / nasdaq / nyse / amex）
    market: String,
    /// 报价币种（按市场推导：沪深→CNY、港→HKD、美股→USD，ADR-0037 决策 2 / ADR-0081）
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

/// 按代码查询股票实时行情（AI 导入契约，issue #693/#696 / ADR-0081 决策 1/2）：只读，
/// 实时从东方财富取权威名称、精确市场、最新价与类型提示，供 AI 校验「代码 →
/// 名称」映射与核对迁移标的。market 缺省按代码形态单点解析（沪深 6 位、港 5 位
/// 补零、美股字母 ticker 遍历三市场，首个命中生效并返回精确交易所归属）；全部
/// 参数类拒绝路径在发起网络请求前返回；查无此码返回中文错误，AI 可提示用户或
/// 跳过该行。
#[utoipa::path(
    get,
    path = "/api/v1/stocks/{code}",
    tag = "stocks",
    summary = "按代码查询股票实时行情（只读，东财实时，沪深港美）",
    description = "返回东财实时行情：`code`（归一化代码，港股左补零至 5 位、美股大写）/ `name`（东财权威名称）\
                  / `market`（精确市场）/ `currency_code`（币种：沪深→CNY、港→HKD、美股→USD）\
                  / `price_cents`（最新价，万分之一元）/ `price_date`（价格日期，ISO 日期）\
                  / `kind_hint`（类型提示：stock=股票、etf=场内基金类）。\
                  停牌或无有效报价时 `price_cents` / `price_date` 为 null。\
                  `market` 为可选查询参数：缺省按代码形态单点解析（6 位 6 开头→沪 sh、0/3 开头→深 sz、\
                  5 开头→沪 / 1 开头→深的场内基金段（ETF/LOF）、5 位及以下数字→港 hk 左补零归一、\
                  纯字母 ticker→美股三市场候选遍历，按 nasdaq/nyse/amex 序尝试、首个命中生效并返回精确交易所归属）；\
                  显式传参须与代码形态一致（美股 ticker 可显式传 nasdaq/nyse/amex，零遍历开销）。北交所代码（4/8 开头）暂不支持、\
                  无法推断的代码形态、`market` 与代码形态矛盾均返回 400 中文错误；全候选未命中（查无此码）返回 400 中文错误；\
                  本端点实时访问东方财富，网络故障返回 500。\
                  股票迁移先按本端点确认识别（名称/市场/币种/最新价/类型提示），\
                  再以真实代码与精确市场创建标的，不走名称充代码。",
    params(
        ("code" = String, Path, description = "股票代码（沪深 6 位数字 / 港股 5 位及以下数字 / 美股字母 ticker，大小写不敏感）"),
        ("market" = Option<String>, Query, description = "交易市场（可选：sh/sz/hk/nasdaq/nyse/amex；缺省按代码形态解析）")
    ),
    responses(
        (status = 200, description = "股票行情（名称/精确市场/币种/最新价/价格日期/类型提示）", body = StockLookup),
        (status = 400, description = "北交所代码暂不支持；代码形态无法推断；market 与代码形态矛盾或不在支持闭集；查无此码", body = ErrorResponse),
        (status = 500, description = "东财网络不可达等临时故障", body = ErrorResponse)
    )
)]
pub async fn lookup_stock_handler(
    State(state): State<ApiState>,
    Path(code): Path<String>,
    Query(query): Query<StockLookupQuery>,
) -> Result<Json<StockLookup>, AppError> {
    // 形态解析（推断 / 遍历候选 / 矛盾 / 不支持 / 北交所）在发起网络前完成：非法参数即刻 400。
    let candidates = resolve_stock_quote_candidates(query.market.as_deref(), &code)?;
    let quote = fetch_stock_quote_first_hit_for_api(&state, &candidates).await?;
    Ok(Json(StockLookup::from(quote)))
}
