//! 股票按（市场，代码）查询与创建增强的领域规则收口（issue #693/#694 /
//! ADR-0081 决策 1/2）：代码形态 → 市场单点推断（显式 market 校验共用同一份
//! 形态规则）、报价币种推导与创建增强的东财往返路由判定。东财网络访问在行情
//! 同步域（`sync::stock`，注入桩形态与基金详情获取接缝同构）；落库接缝
//! （[`persist_stock_quote`] / [`create_stock_degraded`]）镜像场外基金的
//! `fund.rs` 同名接缝，供 stocks 查询端点、标的创建壳与后续添加投资标的壳
//! 共用（spec #690 测试决策：唯一新增接缝，三个壳共用）。
//!
//! 推断规则（沪深港，本议题闭集）：6 位数字 6/5 开头→沪（5 开头为场内基金
//! 段，ETF/LOF 是类型提示的探测对象）、0/3/1 开头→深（1 开头为场内基金段）、
//! 5 位及以下数字→港股（左补零至 5 位归一）；4/8 开头为北交所代码，显式
//! 400 暂不支持（另行议题）；其余形态（字母 ticker 等）本议题不推断。

use rusqlite::params;

use super::crud;
use super::model::{InstrumentInput, InstrumentType, StockQuote};
use super::prices::{EASTMONEY_PRICE_SOURCE, upsert_market_price};
use crate::db::now_iso;
use crate::error::{AppError, Result};

/// 本接缝支持的查询市场（沪深港；美股三市场候选遍历随后续议题开放，
/// ADR-0081 决策 2 / spec #690 T4）。市场是行情路由的硬键，闭集在解析单点收口。
pub const STOCK_LOOKUP_MARKETS: &[&str] = &["sh", "sz", "hk"];

/// 报价币种缺省推导（ADR-0037 决策 2；美股三市场→USD 见 ADR-0081）：
/// 沪深→人民币、港→港币、美股三市场（nasdaq/nyse/amex）→美元、其余（含 unknown）→人民币。
///
/// 依据：标的币种不参与买卖账务（持仓批次成本币种 = 账户币种），仅影响行情/市值
/// 折算展示。与同步侧 `crate::sync::http::MARKETS` 的 market→currency 对应（该表
/// 为全量同步板块闭集、模块私有）；美股三市场仅入本推导与行情 secid 映射、不入
/// MARKETS——美股字典走按代码即建、不做全量同步（ADR-0081）。
pub fn derive_quote_currency(market: &str) -> &'static str {
    match market {
        "hk" => "HKD",
        "nasdaq" | "nyse" | "amex" => "USD",
        // 沪深与未知市场均落人民币
        _ => "CNY",
    }
}

/// 解析结果：市场（推断或显式校验通过）+ 归一化代码（港股左补零至 5 位）。
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
    /// 有形态但不在本接缝闭集内（6 位 2/7/9 开头的 B 股等形态、字母 ticker
    /// 美股形态、非数字等）：不推断、显式传参不通过，一律 400。
    Ambiguous,
}

fn code_shape(code: &str) -> CodeShape {
    let is_digits = !code.is_empty() && code.bytes().all(|b| b.is_ascii_digit());
    if !is_digits {
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

/// 港股代码左补零至 5 位归一（"700"→"00700"；已 5 位不变）；其余市场原样。
/// 东财港股 secid 以 5 位代码为规范形态（如 116.00700），归一在解析单点完成。
fn normalize_code(market: &str, code: &str) -> String {
    if market == "hk" {
        format!("{code:0>5}")
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

/// 解析（可选市场，代码）→（市场，归一化代码）（单点，ADR-0081 决策 1）：
/// - 缺省 market：按代码形态单点推断（6 位 6 开头→沪、6 位 0/3 开头→深、
///   ≤5 位数字→港股左补零归一）；
/// - 显式 market：须为本接缝支持市场（沪深港）且与代码形态一致，矛盾即 400；
/// - 北交所代码（4/8 开头）与无法推断的形态分别显式 400——「暂不支持」/
///   「参数矛盾」/「无法推断」三类码化错误。
///
/// 全部拒绝路径在发起网络请求前返回（先例：基金代码格式校验）。
pub fn resolve_stock_market(market: Option<&str>, code: &str) -> Result<ResolvedStockCode> {
    // 显式 market 先过支持闭集：不在闭集的市场无论代码形态一律「暂不支持」，
    // 该判定与代码形态正交（美股等后续议题放开时只扩本清单与形态规则）。
    if let Some(m) = market.filter(|m| !STOCK_LOOKUP_MARKETS.contains(m)) {
        return Err(AppError::codedp(
            "stock.market-unsupported",
            format!("暂不支持查询 {m} 市场（当前支持沪 sh/深 sz/港 hk）"),
            &[m],
        ));
    }
    match code_shape(code) {
        CodeShape::BeijingExchange => Err(bse_unsupported(code)),
        CodeShape::Single(inferred) => match market {
            None => Ok(ResolvedStockCode {
                market: inferred,
                code: normalize_code(inferred, code),
            }),
            Some(m) if m == inferred => Ok(ResolvedStockCode {
                market: inferred,
                code: normalize_code(inferred, code),
            }),
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

// ---------------------------------------------------------------------------
// 创建增强（issue #694 / ADR-0081 决策 2，镜像 fund.rs 的同名接缝）
// ---------------------------------------------------------------------------

/// 股票创建增强的落库结果（形状镜像 [`super::fund::FundCreateOutcome`]）：标的
/// id + 是否落现价（价格失效信号广播判定，见 `WriteEvidence::PriceWritten`）。
pub struct StockCreateOutcome {
    pub instrument_id: String,
    pub price_written: bool,
}

/// 创建壳的东财往返路由判定（单点，镜像 fund 增强的「真实代码才触网」前提）：
/// - [`StockCreateRoute::Enhance`]：symbol 是可解析的真实代码（沪深 6 位 / 港
///   ≤5 位，显式 market 与形态一致或缺省）——以东财校验创建，携带解析后的
///   （市场，归一化代码）；
/// - [`StockCreateRoute::Reject`]：北交所代码（暂不支持，与查询端点同一码化
///   边界），或真实代码形态与显式 market 矛盾 / 不支持——建出来只能是无法
///   估值或错挂行情的行，显式 400 优于静默错行；
/// - [`StockCreateRoute::Generic`]：非代码形态（名称充代码兜底、美股 ticker
///   等本接缝未开放的形态）——走通用创建路径，不发起网络请求；调用方提交的
///   market 原样保留（T4 美股议题开放后由该路径自然升级）。
#[derive(Debug)]
pub enum StockCreateRoute {
    Enhance(ResolvedStockCode),
    Reject(AppError),
    Generic,
}

/// 按创建入参路由东财增强（判定全部在发起网络前完成，先例：基金代码格式校验）。
pub fn route_stock_creation(market: Option<&str>, symbol: &str) -> StockCreateRoute {
    match code_shape(symbol) {
        CodeShape::BeijingExchange => StockCreateRoute::Reject(bse_unsupported(symbol)),
        CodeShape::Single(_) => match resolve_stock_market(market, symbol) {
            Ok(resolved) => StockCreateRoute::Enhance(resolved),
            // 真实代码 + 矛盾/不支持的 market：拒绝（错误随 resolve 单点措辞）。
            Err(e) => StockCreateRoute::Reject(e),
        },
        CodeShape::Ambiguous => StockCreateRoute::Generic,
    }
}

/// AI 创建端点 stock 增强的东财命中落库（镜像 [`super::fund::persist_fund_detail`]，
/// issue #694 / ADR-0081 决策 2）：以归一化真实代码 + 东财权威名称 + 解析市场建/复用
/// 标的行（来源 manual、币种按市场推导），有最新价时落现价缓存。`kind` 为调用方
/// 提交类型（stock/etf，两者同属场内行情通道；导入知识按类型提示填）——东财类型
/// 提示只在查询端点投影，不在此改写类型（自然键（代码，类型）不因探测漂移而漂移）。
/// 现价 `priced_at` = 写入时刻、`nav_date` 恒 None——与全量/增量同步的股票现价
/// 写入口径一致（净值日期是场外基金语义）；覆盖不比较新旧：本通道语义 = 东财当前
/// 最新值整体回放。
pub fn persist_stock_quote(
    conn: &rusqlite::Connection,
    kind: InstrumentType,
    quote: &StockQuote,
) -> Result<StockCreateOutcome> {
    let currency = derive_quote_currency(&quote.market);
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: quote.code.clone(),
            kind,
            name: Some(quote.name.clone()),
            currency_code: currency.to_string(),
            market: Some(quote.market.clone()),
        },
    )?;
    let price_written = quote.price_cents.is_some();
    if let Some(price_cents) = quote.price_cents {
        upsert_market_price(
            conn,
            &instrument_id,
            price_cents,
            currency,
            &now_iso(),
            None,
            Some(EASTMONEY_PRICE_SOURCE),
        )?;
    }
    Ok(StockCreateOutcome {
        instrument_id,
        price_written,
    })
}

/// AI 创建端点 stock 增强的降级落库（镜像 [`super::fund::create_fund_degraded`]，
/// 关键差异：**保留解析市场**）——东财临时不可达等临时故障时，以 AI 提交名称 +
/// 真实代码 + 解析市场建行（不阻塞导入）。`kind` 语义同 [`persist_stock_quote`]。
/// 与基金恒 unknown 不同：股票行情通道只依赖（市场，代码），降级行在行情恢复后
/// 仍可达（查询与价格同步照常服务）。既有行直接复用、名称与市场不动——降级重放
/// 不得用 AI 名称覆盖已回填的东财权威名称。
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
