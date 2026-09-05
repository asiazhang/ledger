//! 标的端点：搜索（统一模糊搜索、封顶返回）与幂等创建（含东财基金增强）。

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use rusqlite::Connection;
use serde::Deserialize;

use crate::api_server::error::ErrorResponse;
use crate::api_server::handlers::funds::fetch_fund_detail_for_api;
use crate::api_server::state::ApiState;
use crate::db::run_db;
use crate::error::AppError;
use crate::investment::{
    FundCreateOutcome, FundDetail, InstrumentInput, InstrumentListFilter, InstrumentListResult,
    InstrumentType, create_fund_degraded, is_six_digit_code, persist_fund_detail,
};
use crate::signals::{WriteEvidence, WriteOp};
use crate::write_entry::{Outcome, write_entry};

/// 标的搜索查询参数（`GET /api/v1/instruments`，issue #294 / ADR-0037）。
#[derive(Debug, Deserialize)]
pub struct InstrumentSearchQuery {
    /// 搜索关键词（必填；空即 400——搜索式而非全量列表）
    query: Option<String>,
    /// 返回条数上限：缺省 20，最大 100（超出收敛为 100，小于 1 视为 1）
    limit: Option<i64>,
    /// 交易市场精确过滤（sh / sz / hk / unknown）
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
    description = "返回 `{items, total}`：`items` 为按 symbol 排序的前 `limit` 条命中标的，\
                  `total` 恒为命中总数。`query` 必填（缺失或纯空白返回 400——本端点是搜索式而非全量列表）；\
                  `limit` 缺省 20、上限 100（超出收敛为 100）。\
                  命中语义为统一模糊搜索：`query` 按空白切词、词条之间 AND；每个词条对「代码 · 名称」label 判定——\
                  原文连续子串 ∨ 拼音首字母串子序列（均大小写不敏感；无名称标的退化为裸代码），\
                  如 `gzmt` 命中「600519 贵州茅台」。\
                  `market` / `type` 可选精确过滤；同码异类型（如基金 000001 vs 股票 000001）\
                  靠 `type` 消歧。返回完整 Instrument 形状（含 `price_cents` 最新行情与 `invested` 是否持仓）。",
    params(
        ("query" = String, Query, description = "搜索关键词（必填，空即 400）"),
        ("limit" = Option<i64>, Query, description = "返回条数上限，缺省 20，最大 100（小于 1 视为 1）"),
        ("market" = Option<String>, Query, description = "交易市场精确过滤（sh / sz / hk / unknown）"),
        ("type" = InstrumentType, Query, description = "标的类型过滤（stock/fund/bond/etf/other），同码异类型消歧用")
    ),
    responses(
        (status = 200, description = "命中标的 {items, total}", body = InstrumentListResult),
        (status = 400, description = "缺 query 或 query 为纯空白；或参数非法", body = ErrorResponse),
        (status = 500, description = "数据库错误", body = ErrorResponse)
    )
)]
pub async fn search_instruments_handler(
    State(conn): State<Arc<Mutex<Connection>>>,
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
            AppError::Invalid(
                "query 不能为空：标的搜索为搜索式端点，请携带关键词（不做全量列表）".into(),
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
    run_db("GET /api/v1/instruments", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        Ok(Json(crate::investment::list_instruments(&conn, &filter)?))
    })
    .await
}

/// 标的创建请求体（`POST /api/v1/instruments`，issue #296 / ADR-0037）。
///
/// 与 IPC 侧 `InstrumentInput` 的差异仅在报价币种可省：缺省按市场推导
/// （沪深→CNY、港→HKD、未知→CNY），显式传参可覆盖。
#[derive(Debug, Deserialize, utoipa::ToSchema)]
pub struct InstrumentCreateInput {
    /// 标的代码（必填；源数据只有名称时以名称充当代码，ADR-0037 决策 3）
    symbol: String,
    /// 标的类型（闭集：stock/fund/bond/etf/other；五类全开，不经自建标的的 UI 白名单）
    #[serde(rename = "type")]
    kind: InstrumentType,
    /// 标的名称（可选）
    name: Option<String>,
    /// 交易市场（可选，缺省 unknown；sh / sz / hk）
    market: Option<String>,
    /// 报价币种（可选；缺省按市场推导：沪深→CNY、港→HKD、未知→CNY）
    currency_code: Option<String>,
}

/// 报价币种缺省推导（ADR-0037 决策 2）：沪深→人民币、港→港币、其余（含 unknown）→人民币。
///
/// 依据：标的币种不参与买卖账务（持仓批次成本币种 = 账户币种），仅影响行情/市值折算展示。
/// 与同步侧 `crate::sync::http::MARKETS` 的 market→currency 对应（该表为同步
/// 市场闭集、模块私有，本端点按 ADR 独立定义并多担 unknown 缺省）；新增市场时两处同改。
fn derive_quote_currency(market: &str) -> &'static str {
    match market {
        "hk" => "HKD",
        // 沪深与未知市场均落人民币
        _ => "CNY",
    }
}

/// 标的幂等创建（AI 导入契约，issue #296 / ADR-0037）：find-or-create 自然键
/// （symbol, 类型），命中静默复用并按需更新名称/市场、返回既有 id，未命中创建；
/// 重复创建同一标的返回同一 id，不产生字典碎片。核心语义复用投资域核心创建函数
/// `investment::create_instrument`（不经自建标的的 UI 类型白名单，ADR-0037 决策 4），
/// 新建行来源标记 = `'manual'`（非同步即手动）。
#[utoipa::path(
    post,
    path = "/api/v1/instruments",
    tag = "instruments",
    summary = "创建标的（按（代码，类型）幂等 find-or-create，fund 类型经东财增强）",
    description = "按自然键（`symbol` + `type`）幂等创建标的：已存在同码同类型行时**静默复用**\
                  并按需更新名称/市场、返回既有 id，未命中创建新行（来源标记 = `manual`）——\
                  重复创建同一标的返回同一 id，不产生字典碎片。响应照账户/分类创建先例：\
                  201 + 裸 id 字符串，无 created 标记。\
                  入参：`symbol` 必填（源数据只有名称时以名称充当代码）；`type` 为闭集五类\
                  （stock/fund/bond/etf/other，五类全开）；`name` 可选；`market` 可选（缺省 `unknown`）；\
                  `currency_code` 可选（缺省按市场推导：沪深→CNY、港→HKD、未知→CNY，显式传参可覆盖）。\
                  **fund 类型增强**：`symbol` 为真实 6 位代码时后端经东方财富校验并回填权威名称、\
                  落最新净值现价；查无此码返回 400 拒绝创建；东财网络不可达时降级为提交名称 + 真实代码建行\
                  （不阻塞导入）；非 6 位 symbol（名称充代码，仅限源数据无代码）不触发校验、不进净值通道。\
                  fund + 6 位代码分支的字典形态收口：显式 `market` / `currency_code` 不生效（恒 unknown / 人民币）。\
                  建议先搜索（GET /api/v1/instruments）无命中再创建，防同义标的碎片；\
                  基金先按代码查询（GET /api/v1/funds/{code}）确认识别，必带真实 6 位代码。",
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
    // fund 增强的东财往返判定（ADR-0039 决策 3）：仅 fund + 真实 6 位代码触发；
    // 名称充代码（非 6 位，仅限源数据无代码）与其他类型不发起网络请求。
    enum Enrichment {
        Authoritative(FundDetail),
        Degrade,
    }
    let enrichment: Option<Enrichment> =
        if input.kind == InstrumentType::Fund && is_six_digit_code(&input.symbol) {
            Some(
                match fetch_fund_detail_for_api(&state, &input.symbol).await {
                    // 东财命中：权威名称回填 + 净值落现价。
                    Ok(detail) => Enrichment::Authoritative(detail),
                    // 查无此码（接缝约定以 Invalid 上抛）：显式拒绝创建，AI 可提示用户或跳过该行。
                    Err(e @ AppError::Invalid(_)) => return Err(e),
                    // 网络不可达等临时故障：降级为 AI 提供名称 + 真实代码建行，不阻塞导入。
                    Err(_) => Enrichment::Degrade,
                },
            )
        } else {
            None
        };
    // 报价币种可省：缺省按市场推导（沪深→CNY、港→HKD、未知→CNY，ADR-0037 决策 2）；
    // market 缺省解析（None→unknown）由核心创建函数单点承担，此处仅按同口径推导币种。
    // fund 增强分支不经此推导：字典形态收口为按代码即拉同款（市场 unknown、币种人民币）。
    let currency_code = input.currency_code.unwrap_or_else(|| {
        derive_quote_currency(input.market.as_deref().unwrap_or("unknown")).to_string()
    });
    // 壳层统一写入口（ADR-0073）：find-or-create 与信息更新同一写闭包，提交点置脏
    // 与信号内化单点；东财往返已在锁外完成（阻塞网络往返不进锁，慢闭包纪律），
    // 写闭包内零网络。泛型入参仅泛型分支消费，惰性构造；基金增强分支的落现价
    // 证据随闭包返回必达（映射单点判定发不发价格信号，ADR-0044）。
    let instrument_id = write_entry(
        "POST /api/v1/instruments",
        state.conn.clone(),
        state.emitter.as_deref(),
        WriteOp::CreateInstrument,
        move |conn| {
            let outcome: FundCreateOutcome = match &enrichment {
                Some(Enrichment::Authoritative(detail)) => {
                    // 东财命中：与按代码即拉同一落库接缝（权威名称回填 + 净值落现价）。
                    let r = persist_fund_detail(conn, &input.symbol, detail)?;
                    FundCreateOutcome {
                        instrument_id: r.instrument_id,
                        price_written: r.price_written,
                    }
                }
                Some(Enrichment::Degrade) => {
                    create_fund_degraded(conn, &input.symbol, input.name.clone())?
                }
                None => {
                    let generic_input = InstrumentInput {
                        symbol: input.symbol.clone(),
                        kind: input.kind,
                        name: input.name.clone(),
                        currency_code,
                        market: input.market.clone(),
                    };
                    FundCreateOutcome {
                        instrument_id: crate::investment::create_instrument(conn, generic_input)?,
                        price_written: false,
                    }
                }
            };
            let evidence = WriteEvidence::PriceWritten(outcome.price_written);
            Ok(Outcome::Evidenced(outcome.instrument_id, evidence))
        },
    )
    .await?;
    Ok((StatusCode::CREATED, Json(instrument_id)))
}
