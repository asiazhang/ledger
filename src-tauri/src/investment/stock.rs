//! 股票按（市场，代码）查询的领域规则收口（issue #693 / ADR-0081 决策 1）：
//! 代码形态 → 市场单点推断（显式 market 校验共用同一份形态规则）与报价币种
//! 推导。东财网络访问在行情同步域（`sync::stock`，注入桩形态与基金详情获取
//! 接缝同构）；本模块只收纯领域规则，供 stocks 查询端点与后续创建增强、
//! 添加投资标的壳共用（spec #690 测试决策：唯一新增接缝，三个壳共用）。
//!
//! 推断规则（沪深港，本议题闭集）：6 位数字 6/5 开头→沪（5 开头为场内基金
//! 段，ETF/LOF 是类型提示的探测对象）、0/3/1 开头→深（1 开头为场内基金段）、
//! 5 位及以下数字→港股（左补零至 5 位归一）；4/8 开头为北交所代码，显式
//! 400 暂不支持（另行议题）；其余形态（字母 ticker 等）本议题不推断。

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
        CodeShape::BeijingExchange => Err(AppError::codedp(
            "stock.bse-unsupported",
            format!("股票代码 {code} 为北交所代码（4/8 开头），暂不支持查询"),
            &[code],
        )),
        CodeShape::Single(inferred) => match market {
            None => Ok(ResolvedStockCode {
                market: inferred,
                code: normalize_code(inferred, code),
            }),
            Some(m) if m == inferred => Ok(ResolvedStockCode {
                market: inferred,
                code: normalize_code(inferred, code),
            }),
            Some(m) => Err(AppError::codedp(
                "stock.market-conflict",
                format!("market 参数 {m} 与股票代码 {code} 的市场形态矛盾，请核对后重试"),
                &[m, code],
            )),
        },
        CodeShape::Ambiguous => match market {
            Some(m) => Err(AppError::codedp(
                "stock.market-conflict",
                format!("market 参数 {m} 与股票代码 {code} 的市场形态矛盾，请核对后重试"),
                &[m, code],
            )),
            None => Err(AppError::codedp(
                "stock.code-unresolvable",
                format!("无法根据股票代码 {code} 的形态推断市场，请显式传入 market 参数"),
                &[code],
            )),
        },
    }
}
