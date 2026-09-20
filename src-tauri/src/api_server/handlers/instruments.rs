//! 标的端点：搜索（统一模糊搜索、封顶返回）与幂等创建（含行情增强：基金经东财、
//! 股票经腾讯行情，ADR-0130）。

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
    InstrumentInput, InstrumentListFilter, InstrumentListResult, InstrumentType, Quote,
    StockCreateRoute, adopt_fund_quote, adopt_stock_quote, create_fund_degraded,
    create_stock_degraded, derive_quote_currency, is_six_digit_code, reject_carried_fund_market,
    route_stock_creation,
};

/// 标的搜索查询参数（`GET /api/v1/instruments`，issue #294 / ADR-0037）。
#[derive(Debug, Deserialize)]
pub struct InstrumentSearchQuery {
    /// 搜索关键词（必填；空即 400——搜索式而非全量列表）
    query: Option<String>,
    /// 返回条数上限：缺省 20，最大 100（超出收敛为 100，小于 1 视为 1）
    limit: Option<i64>,
    /// 交易市场精确过滤（sh / sz / hk / nasdaq / nyse / amex / unknown）
    market: Option<String>,
    /// 标的类型过滤（stock/fund/bond/etf/other）：同码异类型消歧用
    #[serde(rename = "type")]
    kind: Option<InstrumentType>,
}

/// 标的搜索（AI 导入契约，issue #294 / ADR-0037）：供 AI 把流水中的标的描述
/// （代码/名称/拼音首字母）解析为可用标的 id，不提供全量列表。语义复用
/// ADR-0027 统一模糊搜索（既有标的搜索接缝 `investment::list_instruments`），
/// 不为 AI 另造第二口径；按 symbol 排序，封顶返回 + 命中总数控制上下文预算。
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
    // query 必填（trim 后为空视同缺失）：显式校验以返回统一 `{kind, message}` 中文错误。
    // 参数格式错误（如 type 非法枚举值）由 axum extractor 拒绝、同样返回 400，
    // 但响应体为其默认格式（与既有 list_transactions 先例一致）。
    let query = params
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .ok_or_else(|| {
            // ADR-0050 码化收口（#1072）：AI 导入按码自纠的入参条件，
            // message 逐字保留、无插值参数。
            AppError::coded(
                "instrument.query-required",
                "query 不能为空：标的搜索为搜索式端点，请携带关键词（不做全量列表）",
            )
        })?;
    // 封顶返回：缺省 20、上限收敛 100，AI 上下文预算可控。
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let filter = InstrumentListFilter {
        search: Some(query.to_string()),
        market: params.market.filter(|m| !m.is_empty()),
        kind: params.kind,
        only_invested: None,
        page: Some(1),
        page_size: Some(limit as usize),
    };
    read_entry("GET /api/v1/instruments", read.0, move |conn| {
        Ok(Json(ledger_investment::list_instruments(conn, &filter)?))
    })
    .await
}

/// 标的创建请求体（`POST /api/v1/instruments`，issue #296 / ADR-0037）。
///
/// 与 IPC 侧 `InstrumentInput` 的差异仅在报价币种可省：缺省按市场推导
/// （沪深→CNY、港→HKD、美股三市场→USD、未知→CNY，ADR-0081），显式传参可覆盖。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct InstrumentCreateInput {
    /// 标的代码（必填；源数据只有名称时以名称充当代码，ADR-0037 决策 3）
    symbol: String,
    /// 标的类型（闭集：stock/fund/bond/etf/other；五类全开，不经自建标的的 UI 白名单）
    #[serde(rename = "type")]
    kind: InstrumentType,
    /// 标的名称（可选）
    name: Option<String>,
    /// 交易市场（可选，缺省 unknown；sh / sz / hk / nasdaq / nyse / amex）。
    /// fund 类型市场恒 unknown（场外基金无交易所市场概念，ADR-0038）：携带非
    /// unknown 市场即 400（`instrument.fund-market-forbidden`）。
    market: Option<String>,
    /// 报价币种（可选；缺省按市场推导：沪深→CNY、港→HKD、美股三市场→USD、未知→CNY）
    currency_code: Option<String>,
}

// 报价币种缺省推导已上收投资域单点 `ledger_investment::derive_quote_currency`
// （issue #693 随股票查询接缝收口：stocks 查询端点投影同一推导，两处不漂移；
// 推导规则与依据注释见该函数，ADR-0037 决策 2 / ADR-0081）。

/// 标的幂等创建（AI 导入契约，issue #296 / ADR-0037）：find-or-create 自然键
/// （symbol, 类型），命中静默复用并按需更新名称/市场、返回既有 id，未命中创建；
/// 重复创建同一标的返回同一 id，不产生字典碎片。核心语义复用投资域核心创建函数
/// `investment::create_instrument`（不经自建标的的 UI 类型白名单，ADR-0037 决策 4），
/// 新建行来源标记 = `'manual'`（非同步即手动）。
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
    // 行情增强的往返判定（ADR-0039 决策 3 / ADR-0081 决策 2）：仅 fund + 真实 6 位代码、
    // stock/etf + 可解析真实代码触发（场内两类型同属行情通道，类型以提交为准落库；
    // 美股 ticker 缺省按候选序遍历三市场，issue #696）；名称充代码（兜底）与其他类型
    // 不发起网络请求。stock 的路由判定收口在投资域单点（route_stock_creation）：
    // 北交所与真实代码形态的 market 矛盾在发起网络前显式 400；非代码形态走通用创建路径。
    enum Enrichment {
        FundAuthoritative(Quote),
        FundDegrade,
        StockAuthoritative {
            kind: InstrumentType,
            quote: Quote,
        },
        StockDegrade {
            kind: InstrumentType,
            market: String,
            code: String,
        },
    }
    // fund 市场恒 unknown 入口守卫（ADR-0038 决策 1 修订 / issue #1194）：6 位
    // 增强/降级分支的市场由通道字典形态自造、不读调用方输入，故必须在落到写入
    // 协议前先拒绝调用方携带的非 unknown 市场——不变量对全部创建通道成立，不因
    // 6 位增强而例外（非 6 位通用通道另由标的写入协议守卫兜底，判据与文案同源）。
    if input.kind == InstrumentType::Fund {
        reject_carried_fund_market(input.market.as_deref())?;
    }
    let enrichment: Option<Enrichment> = match input.kind {
        InstrumentType::Fund if is_six_digit_code(&input.symbol) => {
            Some(
                match fetch_fund_quote_for_api(&state, &input.symbol).await {
                    // 东财命中：权威名称回填 + 净值落现价。
                    Ok(quote) => Enrichment::FundAuthoritative(quote),
                    // 查无此码（接缝约定以 sync.fund-not-found 码化 400 上抛）：显式拒绝
                    // 创建，AI 可提示用户或跳过该行。按稳定错误码判定（与下方股票分支同款
                    // is_code 形态，#1186），不靠错误变体形状——生产路径返回码化错误，
                    // 按变体匹配会把查无此码误降级为建行。
                    Err(e) if e.is_code("sync.fund-not-found") => return Err(e),
                    // 网络不可达等临时故障：降级为 AI 提供名称 + 真实代码建行，不阻塞导入。
                    Err(_) => Enrichment::FundDegrade,
                },
            )
        }
        InstrumentType::Stock | InstrumentType::Etf => {
            match route_stock_creation(input.market.as_deref(), &input.symbol) {
                StockCreateRoute::Enhance(plan) => {
                    Some(
                        match fetch_stock_quote_for_api(
                            &state,
                            plan.candidate.market,
                            &plan.candidate.code,
                        )
                        .await
                        {
                            // 行情命中：权威名称回填 + 最新价落现价（精确市场随行情自报）。
                            Ok(quote) => Enrichment::StockAuthoritative {
                                kind: input.kind,
                                quote,
                            },
                            // 查无此码（接缝约定以 sync.stock-not-found
                            // 码化 400 上抛）：显式拒绝创建，AI 可提示用户核对代码或跳过该行。
                            Err(e) if e.is_code("sync.stock-not-found") => return Err(e),
                            // 网络不可达等临时故障：降级为提交名称 + 真实代码 + 降级市场建行
                            //（显式市场或形态可推断的沪深港 → 保留市场，行情恢复后价格同步仍
                            // 可达；美股 ticker 缺省 → unknown，见 StockEnhancePlan），不阻塞导入。
                            Err(_) => Enrichment::StockDegrade {
                                kind: input.kind,
                                market: plan.degrade_market,
                                code: plan.candidate.code,
                            },
                        },
                    )
                }
                // 北交所代码 / 真实代码 + 矛盾 market：显式 400，不建错行。
                StockCreateRoute::Reject(e) => return Err(e),
                // 非代码形态（名称充代码兜底、美股 ticker 等）：通用创建路径。
                StockCreateRoute::Generic => None,
            }
        }
        _ => None,
    };
    // 报价币种可省：缺省按市场推导（沪深→CNY、港→HKD、美股三市场→USD、未知→CNY，
    // ADR-0037 决策 2 / ADR-0081）；
    // market 缺省解析（None→unknown）由核心创建函数单点承担，此处仅按同口径推导币种。
    // fund 增强分支不经此推导：字典形态收口为按代码即拉同款（市场 unknown、币种人民币）。
    let currency_code = input.currency_code.unwrap_or_else(|| {
        derive_quote_currency(input.market.as_deref().unwrap_or("unknown")).to_string()
    });
    // 壳层统一写入口（ADR-0073）：find-or-create 与信息更新同一写闭包，提交点置脏
    // 与信号内化单点；行情往返已在锁外完成（阻塞网络往返不进锁，慢闭包纪律），
    // 写闭包内零网络。泛型入参仅泛型分支消费，惰性构造；基金增强分支的落现价
    // 证据随闭包返回必达（映射单点判定发不发价格信号，ADR-0044）。
    let instrument_id = write_entry(
        "POST /api/v1/instruments",
        state.write_handle(),
        state.emitter.as_deref(),
        WriteOp::CreateInstrument,
        move |conn| {
            let (instrument_id, price_written) = match &enrichment {
                Some(Enrichment::FundAuthoritative(quote)) => {
                    // 东财命中：与按代码即拉同一落库接缝（权威名称回填 + 净值落现价）。
                    let r = adopt_fund_quote(conn, quote)?;
                    (r.instrument_id, r.price_written)
                }
                Some(Enrichment::FundDegrade) => {
                    let r = create_fund_degraded(conn, &input.symbol, input.name.clone())?;
                    (r.instrument_id, r.price_written)
                }
                Some(Enrichment::StockAuthoritative { kind, quote }) => {
                    // 行情命中：与查询端点同一行情投影落库（权威名称回填 + 最新价落现价）。
                    let r = adopt_stock_quote(conn, *kind, quote)?;
                    (r.instrument_id, r.price_written)
                }
                Some(Enrichment::StockDegrade { kind, market, code }) => {
                    // 降级：提交名称 + 真实代码 + 降级市场建行（基金恒 unknown 的镜像差异；
                    // 美股缺省遍历降级同 unknown，见 StockEnhancePlan）。
                    let r = create_stock_degraded(conn, *kind, market, code, input.name.clone())?;
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
