//! 股票按（市场，代码）查询与创建增强的领域规则收口（issue #693/#694/#696 /
//! ADR-0081 决策 1/2；换源 ADR-0130 决策 2 / issue #1567）：代码形态 → 查询单元
//! 单点解析（显式 market 校验共用同一份形态规则）、报价币种推导与创建增强的
//! 行情往返路由判定。行情网络访问在行情同步域 `ledger-market-sync` crate
//! （`sync::stock`，统一注入签名的场内实例，ADR-0103 决策 2）；行情接入
//! 落库半边的场内通道（[`adopt_stock_quote`] / [`create_stock_degraded`]）镜像
//! 场外基金的 `fund.rs` 同族接缝，供 stocks 查询端点、标的创建壳与后续添加投资
//! 标的壳共用（spec #690 测试决策：唯一新增接缝，三个壳共用）。
//!
//! 推断规则：6 位数字 6/5 开头→沪（5 开头为场内基金段，ETF/LOF 是类型提示的
//! 探测对象）、0/3/1 开头→深（1 开头为场内基金段）、5 位及以下数字→港股
//! （左补零至 5 位归一）、纯字母 ticker→美股单查询（聚合路由值 `us`——行情源
//! 不区分交易所，精确交易所由响应自报后缀判定，ADR-0081 决策 2 的三市场候选
//! 遍历随换源退役，issue #1567）；4/8 开头为北交所代码，显式 400 暂不支持
//! （另行议题）；其余形态（字母数字混杂等）不推断。

use std::future::Future;

use rusqlite::params;

use super::crud;
use super::model::{AddStockInstrumentResult, InstrumentInput, InstrumentType};
use super::quote::{Quote, QuoteAdoptionInput, adopt_quote};
use ledger_infra::db::now_iso;
use ledger_infra::error::{AppError, Result};

/// 本接缝支持的显式查询市场闭集：沪深港 + 美股三市场（ADR-0081 决策 1/2，issue
/// #696）。市场是行情路由的硬键，闭集在解析单点收口；行情查询键构造见
/// `sync::tencent::tencent_query_key`（行情同步域 `ledger-market-sync` crate，
/// 两者闭集同步扩展）。
pub const STOCK_LOOKUP_MARKETS: &[&str] = &["sh", "sz", "hk", "nasdaq", "nyse", "amex"];

/// 美股聚合路由值（仅存在于查询解析产物，非市场闭集成员、不落库不出响应）：
/// 行情源对美股不区分交易所（查询键统一 `us<代码>`），显式美股市场与缺省同解，
/// 精确交易所由行情响应自报后缀判定（ADR-0130 决策 2/4 / issue #1567）。
const US_AGGREGATE_MARKET: &str = "us";

/// 美股三市场判定（代码归一与币种推导共用同一闭集定义）。
fn is_us_market(market: &str) -> bool {
    matches!(market, "nasdaq" | "nyse" | "amex")
}

/// 报价币种缺省推导（ADR-0037 决策 2；美股三市场→USD 见 ADR-0081）：
/// 沪深→人民币、港→港币、美股三市场（nasdaq/nyse/amex）→美元、其余（含 unknown）→人民币。
///
/// 依据：标的币种不参与买卖账务（持仓批次成本币种 = 账户币种），仅影响行情/市值
/// 折算展示。行情查询键映射与报价币种推导共用同一市场闭集；美股三市场仅入
/// 本推导与行情查询键映射，字典走按代码即建（ADR-0081）。
pub fn derive_quote_currency(market: &str) -> &'static str {
    if market == "hk" {
        "HKD"
    } else if is_us_market(market) {
        "USD"
    } else {
        // 沪深与未知市场均落人民币
        "CNY"
    }
}

/// 解析结果：查询路由市场 + 归一化代码（港股左补零至 5 位、美股大写）。
/// 沪深港市场即精确市场；美股恒为聚合路由值 `us`（精确交易所由行情响应自报）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedStockCode {
    pub market: &'static str,
    pub code: String,
}

/// 代码形态：推断与显式 market 校验共用同一份形态规则（单点，两分支不漂移）。
enum CodeShape {
    /// 北交所代码（6 位数字 4/8 开头）：形态即歧义消除，显式「暂不支持」（另行议题）。
    BeijingExchange,
    /// 恰好兼容一个市场：6 位 6 开头→沪、0/3 开头→深、5 开头→沪（场内基金段
    /// 50/51/52/56/58）、1 开头→深（场内基金段 15/16/18）；≤5 位数字→港股。
    /// 场内基金段入形态闭集：ETF/LOF 正是本端点类型提示（etf）的探测对象
    ///（spec #690 用户故事 12 / ADR-0081），不收录则提示成死分支。
    Single(&'static str),
    /// 纯字母 ticker（美股形态）：单查询解析为聚合路由值 `us`（行情源自报精确
    /// 交易所，issue #1567）；显式美股市场同解，显式沪深港为形态矛盾。
    UsTicker,
    /// 有形态但无法归入任何支持市场（6 位 2/7/9 开头的 B 股、字母数字混杂等）：
    /// 不推断、显式传参不通过，一律 400。
    Ambiguous,
}

fn code_shape(code: &str) -> CodeShape {
    if code.is_empty() {
        return CodeShape::Ambiguous;
    }
    if code.bytes().all(|b| b.is_ascii_alphabetic()) {
        return CodeShape::UsTicker;
    }
    if !code.bytes().all(|b| b.is_ascii_digit()) {
        return CodeShape::Ambiguous;
    }
    match code.len() {
        6 => match code.as_bytes()[0] {
            b'4' | b'8' => CodeShape::BeijingExchange,
            b'6' | b'5' => CodeShape::Single("sh"),
            b'0' | b'3' | b'1' => CodeShape::Single("sz"),
            _ => CodeShape::Ambiguous,
        },
        1..=5 => CodeShape::Single("hk"),
        _ => CodeShape::Ambiguous,
    }
}

/// 港股代码左补零至 5 位归一（"700"→"00700"；已 5 位不变）、美股聚合查询的
/// ticker 大写归一（"aapl"→"AAPL"，幂等建行同自然键）；其余市场原样。
fn normalize_code(market: &str, code: &str) -> String {
    if market == "hk" {
        format!("{code:0>5}")
    } else if market == US_AGGREGATE_MARKET {
        code.to_ascii_uppercase()
    } else {
        code.to_string()
    }
}

/// 「显式 market 与代码形态矛盾」的统一码化错误（两分支共用同一形状）。
fn market_conflict(market: &str, code: &str) -> AppError {
    AppError::codedp(
        "stock.market-conflict",
        format!("market 参数 {market} 与股票代码 {code} 的市场形态矛盾，请核对后重试"),
        &[market, code],
    )
}

/// 北交所代码的统一码化错误（查询端点与创建增强共用同一码化边界）。
fn bse_unsupported(code: &str) -> AppError {
    AppError::codedp(
        "stock.bse-unsupported",
        format!("股票代码 {code} 为北交所代码（4/8 开头），暂不支持"),
        &[code],
    )
}

/// 解析（可选市场，代码）→ 查询单元（单点，ADR-0081 决策 1/2 / ADR-0130 决策 2）。
/// 换源后查询恒为单只（美股三市场候选遍历退役）：腾讯行情键对美股不区分交易所，
/// 一次查询即由响应自报精确交易所归属，无需候选遍历。
/// - 缺省 market：按代码形态单点推断（6 位 6 开头→沪、6 位 0/3 开头→深、
///   ≤5 位数字→港股左补零归一、纯字母 ticker→美股聚合查询 `us`）；
/// - 显式 market：须为支持闭集市场且与代码形态一致（沪深港一致放行；美股三值
///   同解为聚合查询——响应自报权威归属）；矛盾即 400；
/// - 北交所代码（4/8 开头）、闭集外市场与无法推断的形态分别显式 400——
///   「暂不支持」/「参数矛盾」/「无法推断」三类码化错误。
///
/// 全部拒绝路径在发起网络请求前返回（先例：基金代码格式校验）。
pub fn resolve_stock_code(market: Option<&str>, code: &str) -> Result<ResolvedStockCode> {
    // 显式 market 先过支持闭集：不在闭集的市场无论代码形态一律「暂不支持」，
    // 该判定与代码形态正交。
    if let Some(m) = market.filter(|m| !STOCK_LOOKUP_MARKETS.contains(m)) {
        return Err(AppError::codedp(
            "stock.market-unsupported",
            format!("暂不支持查询 {m} 市场（当前支持沪 sh/深 sz/港 hk/美股 nasdaq/nyse/amex）"),
            &[m],
        ));
    }
    let resolved = |market: &'static str| {
        Ok(ResolvedStockCode {
            market,
            code: normalize_code(market, code),
        })
    };
    match code_shape(code) {
        CodeShape::BeijingExchange => Err(bse_unsupported(code)),
        CodeShape::Single(inferred) => match market {
            None => resolved(inferred),
            // 显式 market 与推断一致：值相同，取形态侧的 'static 闭集成员。
            Some(m) if m == inferred => resolved(inferred),
            Some(m) => Err(market_conflict(m, code)),
        },
        CodeShape::UsTicker => match market {
            // 缺省与显式美股市场同解：单查询 `us<代码>`，精确交易所由行情响应自报
            //（闭集校验已保证显式值为美股三市场之一）。
            None => resolved(US_AGGREGATE_MARKET),
            Some(m) if is_us_market(m) => resolved(US_AGGREGATE_MARKET),
            Some(m) => Err(market_conflict(m, code)),
        },
        CodeShape::Ambiguous => match market {
            Some(m) => Err(market_conflict(m, code)),
            None => Err(AppError::codedp(
                "stock.code-unresolvable",
                format!("无法根据股票代码 {code} 的形态推断市场，请显式传入 market 参数"),
                &[code],
            )),
        },
    }
}

/// 录入通道 → 股票查询的显式市场参数（单点）：美股通道 → None（按 ticker 形态
/// 解析为聚合查询，精确交易所由行情响应自报）；沪深港 → 显式市场。闭集外通道即
/// 内部不一致（前端下拉闭集之外不应有值，先例：`sync.secid-unroutable` 的码化拒绝）。
pub fn resolve_add_stock_channel(channel: &str) -> Result<Option<&'static str>> {
    match channel {
        "sh" => Ok(Some("sh")),
        "sz" => Ok(Some("sz")),
        "hk" => Ok(Some("hk")),
        "us" => Ok(None),
        other => Err(AppError::codedp(
            "stock.channel-unsupported",
            format!("暂不支持的投资标的录入通道 {other}（当前支持沪 sh/深 sz/港 hk/美股 us）"),
            &[other],
        )),
    }
}

/// 「添加投资标的」查询阶段（issue #697，spec #690 唯一接缝的 IPC 侧编排，注入
/// 形态与基金按代码即拉接缝同构——行情接入查询半边的统一签名
/// `(代码, 市场) → Result<Quote>`，ADR-0103 决策 2；async 形态 ADR-0125 决策 7 /
/// issue #1413：闭包返回 future，网络等待以 `await` 表达）：通道解析 → 查询单元
/// 解析（全部拒绝路径在发起网络前，先例：基金代码格式校验）→ 单次行情查询
///（查无此码与临时错误均原样上抛，分流归调用方）。本函数不触数据库：生产壳在
/// 连接锁外以生产拉取闭包 await（慢闭包纪律，先例：`fetch_fund_quote_production`），
/// 测试与 BDD 以注入桩离线驱动。
pub async fn fetch_stock_quote_for_add<F, Fut>(
    channel: &str,
    code: &str,
    fetch: &mut F,
) -> Result<Quote>
where
    F: FnMut(&str, &str) -> Fut,
    Fut: Future<Output = Result<Quote>>,
{
    let market = resolve_add_stock_channel(channel)?;
    let candidate = resolve_stock_code(market, code)?;
    fetch(&candidate.code, candidate.market).await
}

/// 添加投资标的·识别落库阶段（issue #697）：类型自动识别（行情命中 → stock、
/// 行情类型码 → etf，提示随行情在访问层投影为 `kind_hint`，探测单点在
/// `sync::tencent::detect_kind_hint`（行情同步域 `ledger-market-sync` crate））后经
/// 创建增强同一落库接缝
///（[`adopt_stock_quote`]：权威名称回填 + 精确市场落库 + 最新价落现价，来源
/// manual、币种按市场推导）。与 AI 创建端点的差异只在类型来源：对话框无类型
/// 入参，类型即识别结果（spec #690 用户故事 12）；误判代价仅类型标签，已接受
///（ADR-0081）。返回投影供壳层回显（识别回显）。
pub fn add_stock_instrument_with_quote(
    conn: &rusqlite::Connection,
    quote: &Quote,
) -> Result<AddStockInstrumentResult> {
    let kind = quote.stock_kind_hint();
    let market = quote.stock_market()?;
    let outcome = adopt_stock_quote(conn, kind, quote)?;
    Ok(AddStockInstrumentResult {
        instrument_id: outcome.instrument_id,
        symbol: quote.code.clone(),
        name: quote.name.clone(),
        kind,
        market: market.to_string(),
        currency_code: derive_quote_currency(market).to_string(),
        price_cents: quote.price_cents,
        price_date: quote.price_date.clone(),
        price_written: outcome.price_written,
    })
}

// ---------------------------------------------------------------------------
// 创建增强（issue #694 / ADR-0081 决策 2，镜像 fund.rs 的同名接缝）
// ---------------------------------------------------------------------------

/// 股票创建增强的落库结果（形状镜像 [`super::fund::FundCreateOutcome`]）：标的
/// id + 是否落现价（价格失效信号广播判定，见 `WriteEvidence::PriceWritten`）。
pub struct StockCreateOutcome {
    pub instrument_id: String,
    pub price_written: bool,
}

/// 创建壳的行情往返路由判定（单点，镜像 fund 增强的「真实代码才触网」前提）：
/// - [`StockCreateRoute::Enhance`]：symbol 是可解析的真实代码（沪深 6 位 /
///   港 ≤5 位 / 美股字母 ticker，显式 market 与形态一致或缺省）——以行情源校验
///   创建，携带查询单元与降级市场（[`StockEnhancePlan`]）；
/// - [`StockCreateRoute::Reject`]：北交所代码（暂不支持，与查询端点同一码化
///   边界），或真实代码形态与显式 market 矛盾 / 不支持——建出来只能是无法
///   估值或错挂行情的行，显式 400 优于静默错行；
/// - [`StockCreateRoute::Generic`]：非代码形态（名称充代码兜底）——走通用
///   创建路径，不发起网络请求；调用方提交的 market 原样保留。
#[derive(Debug)]
pub enum StockCreateRoute {
    Enhance(StockEnhancePlan),
    Reject(AppError),
    Generic,
}

/// 创建增强的查询计划：查询单元 + 行情临时不可达时的降级市场。
#[derive(Debug)]
pub struct StockEnhancePlan {
    /// 查询单元（换源后恒单只：行情响应自报精确市场与类型，issue #1567）。
    pub candidate: ResolvedStockCode,
    /// 行情临时不可达降级建行时的市场：离线可知的市场保留（显式市场，或形态
    /// 可推断的沪深港）——降级行行情通道仍可达，issue #694 语义；美股 ticker
    /// 缺省在无网络时无法预知交易所归属，降级为 unknown（诚实无行情通道，镜像
    /// 基金恒 unknown；查询先行流先经查询端点取得精确市场再显式传参创建，
    /// 不会进入本分支）。
    pub degrade_market: String,
}

/// 按创建入参路由行情增强（判定全部在发起网络前完成，先例：基金代码格式校验）。
pub fn route_stock_creation(market: Option<&str>, symbol: &str) -> StockCreateRoute {
    let shape = code_shape(symbol);
    match shape {
        CodeShape::BeijingExchange => StockCreateRoute::Reject(bse_unsupported(symbol)),
        CodeShape::Single(_) | CodeShape::UsTicker => {
            match resolve_stock_code(market, symbol) {
                Ok(candidate) => StockCreateRoute::Enhance(StockEnhancePlan {
                    candidate,
                    // 降级市场按「离线可知」取：显式市场恒保留（校验已保证与形态
                    // 一致）；缺省时形态可完全推断的沪深港保留推断值，美股 ticker
                    // 无法预知交易所 → unknown。
                    degrade_market: match market {
                        Some(m) => m.to_string(),
                        None => match shape {
                            CodeShape::Single(m) => m.to_string(),
                            _ => "unknown".to_string(),
                        },
                    },
                }),
                // 真实代码 + 矛盾/不支持的 market：拒绝（错误随解析单点措辞）。
                Err(e) => StockCreateRoute::Reject(e),
            }
        }
        CodeShape::Ambiguous => StockCreateRoute::Generic,
    }
}

/// 行情接入落库半边的场内通道（镜像 [`super::fund::adopt_fund_quote`]，issue #694 /
/// ADR-0081 决策 2 / ADR-0103 决策 3）：精确市场与币种在本通道判定（市场取行情
/// 回显 = 解析单点产物经数据源自报确认，币种按市场推导），建档 + 落现价交
/// [`adopt_quote`] 一体执行。`kind` 为调用方提交类型（stock/etf，两者同属场内行情
/// 通道；导入知识按类型提示填）——行情类型提示只在查询端点投影，不在此改写类型
///（自然键（代码，类型）不因探测漂移而漂移）。现价 `priced_at` = 写入时刻、
/// `nav_date` 恒 None——与同步通道的股票现价写入口径一致（净值日期是场外基金
/// 语义）；价格来源随取数产物携带（`Quote::price_source`，ADR-0130 决策 7，
/// 腾讯投影单点填入）。覆盖不比较新旧：本通道语义 = 数据源当前最新值整体回放。
pub fn adopt_stock_quote(
    conn: &rusqlite::Connection,
    kind: InstrumentType,
    quote: &Quote,
) -> Result<StockCreateOutcome> {
    let market = quote.stock_market()?;
    let outcome = adopt_quote(
        conn,
        &QuoteAdoptionInput {
            kind,
            market,
            currency_code: derive_quote_currency(market),
            // 场内现价时点 = 写入时刻（无净值日期语义）。
            priced_at: &now_iso(),
            nav_date: None,
        },
        quote,
    )?;
    Ok(StockCreateOutcome {
        instrument_id: outcome.instrument_id,
        price_written: outcome.price_written,
    })
}

/// AI 创建端点 stock 增强的降级落库（镜像 [`super::fund::create_fund_degraded`]，
/// 关键差异：**保留解析市场**）——行情源临时不可达等临时故障时，以 AI 提交名称 +
/// 真实代码 + 降级市场建行（不阻塞导入）。`kind` 语义同 [`adopt_stock_quote`]。
/// 与基金恒 unknown 不同：股票行情通道只依赖（市场，代码），降级行在行情恢复后
/// 仍可达（查询与价格同步照常服务）。既有行直接复用、名称与市场不动——降级重放
/// 不得用 AI 名称覆盖已回填的权威名称。
pub fn create_stock_degraded(
    conn: &rusqlite::Connection,
    kind: InstrumentType,
    market: &str,
    code: &str,
    ai_name: Option<String>,
) -> Result<StockCreateOutcome> {
    let existing_id: Option<String> = conn
        .query_row(
            "SELECT id FROM instruments WHERE symbol=?1 AND instrument_type=?2",
            params![code, kind],
            |r| r.get(0),
        )
        .ok();
    if let Some(instrument_id) = existing_id {
        return Ok(StockCreateOutcome {
            instrument_id,
            price_written: false,
        });
    }
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: code.to_string(),
            kind,
            name: ai_name,
            currency_code: derive_quote_currency(market).to_string(),
            market: Some(market.to_string()),
        },
    )?;
    Ok(StockCreateOutcome {
        instrument_id,
        price_written: false,
    })
}
