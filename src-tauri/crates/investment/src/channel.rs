//! 价格通道（PriceChannel，issue #1060）：「这个标的价格从哪里来」的后端派生
//! 事实——按标的类型、市场、代码与恒定单位价格把库内标的分进价格写入通道的
//! 判定单点。
//!
//! 分区口径即标的信息同步（InstrumentInfoSync）的通道能力分区（ADR-0036 /
//! ADR-0038 / issue #695）：行情通道（stock|etf 且市场已知）、净值通道（fund 且
//! 6 位真实代码）、恒定价格通道（标的行携带恒定单位价格，ADR-0126）、手动报价
//! 通道（其余开放录价的行）、无来源（市场未知的股票类——行情不可达且录价入口
//! 不开放，ADR-0036 决策 1）。同步编排、标的读投影
//! （`Instrument::price_channel`）与走势放行、录价入口、过期检查面共用本单点：
//! 此前端端各自镜像推断（走势场内白名单 `hasMarketSource`、录价入口
//! `canManualPrice`），场外基金一等标的定案后漂移成第二口径（issue #1060），
//! 自本派生收口。
//!
//! 恒定价格是分区成员，不是通道上的正交标记（ADR-0126 决策 1）：不存在「既是
//! 行情 / 净值通道、又是恒定价格」的标的——恒定改变的是「谁给它写价」的答案
//! （答案：没有人），因此判定顺序上恒定单位价格在场即归恒定价格通道，其余
//! 三输入（类型 × 市场 × 代码）不再参与。

use super::fund::is_six_digit_code;
use super::market::{Market, QuoteMarket};
use super::model::InstrumentType;
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

/// 价格写入通道（派生事实；除恒定价格外不落库）：随标的行投影输出，前端据此
/// 放行走势与开放录价入口。恒定价格是本闭集唯一需要持久化输入的成员——判定
/// 输入里的恒定单位价格落在标的行上（ADR-0126 决策 2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceChannel {
    /// 行情通道：股票与场内 ETF，市场已知（可构造行情查询），现价与周线经行情
    /// 同步写入（issue #695）。
    Quote,
    /// 净值通道：6 位真实代码的场外基金，净值经标的信息同步逐只写入
    /// （ADR-0038）。
    FundNav,
    /// 恒定价格通道：单位价格是定义性常量的标的（货基恒 1.0000，收益以份额
    /// 结转体现，ADR-0126）——没人给它写价，取值由读侧按标的行的恒定单位
    /// 价格在响应内合成，历史表不为它落行。
    Constant,
    /// 手动报价通道：同步覆盖不到但录价入口开放的行——自建标的（债券/ETF/其他）
    /// 与名称充代码的基金行（ADR-0036 决策 1）。
    Manual,
    /// 无价格来源：市场未知的股票类——行情不可达（无法构造行情查询）且录价入口
    /// 不开放，走势与市值四消费方全部落空。
    None,
}

impl PriceChannel {
    /// 闭集全量清单（OpenAPI 枚举等消费；先例：`InstrumentType::ALL`）。
    pub const ALL: [PriceChannel; 5] = [
        PriceChannel::Quote,
        PriceChannel::FundNav,
        PriceChannel::Constant,
        PriceChannel::Manual,
        PriceChannel::None,
    ];
}

impl std::fmt::Display for PriceChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PriceChannel::Quote => "quote",
            PriceChannel::FundNav => "fund_nav",
            PriceChannel::Constant => "constant",
            PriceChannel::Manual => "manual",
            PriceChannel::None => "none",
        };
        write!(f, "{s}")
    }
}

// OpenAPI（utoipa）：闭集枚举以小写字符串枚举值入文档，与 wire 格式一致
// （先例：`InstrumentType`，内联 schema，消费方字段直接嵌入、无需注册组件；
// 枚举值由 [`PriceChannel::ALL`] 驱动，变体增减单点同步）。
impl PartialSchema for PriceChannel {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(PriceChannel::ALL.map(|c| c.to_string())))
                .description(Some(
                    "价格写入通道（派生事实：quote 行情 / fund_nav 净值 / constant 恒定价格 / manual 手动报价 / none 无来源）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for PriceChannel {}

/// 行情分区的查询单元市场（分区出口，issue #1673）：派生通道为 Quote 当且仅当
/// 本函数返回 Some——内部消费 [`derive_price_channel`] 单点与
/// [`Market::as_quote_market`] 唯一判定点，「行情通道 ⇔ 可路由市场」由同一份
/// 判定承载。同步编排分区经本函数拿到类型化查询单元市场（`QuoteQuery`），
/// Quote 通道配不可路由市场的组合在分区出口即被排除，不再需要运行时兜底。
pub fn derive_quote_market(
    kind: InstrumentType,
    market: Market,
    symbol: &str,
    constant_unit_price: Option<i64>,
) -> Option<QuoteMarket> {
    match derive_price_channel(kind, market, symbol, constant_unit_price) {
        PriceChannel::Quote => market.as_quote_market(),
        _ => None,
    }
}

/// 价格通道派生单点：类型 × 市场 × 代码 + 恒定单位价格 → 通道。同步分区、
/// 标的读投影与过期检查面共用；判定顺序即语义——恒定单位价格在场即归恒定
/// 价格通道（当且仅当该列有值，ADR-0126 决策 2），其余按类型 × 市场 × 代码
/// 分派：市场未知的股票先于手动报价兜底拦截（它连录价入口也不开放）。行情
/// 通道守卫消费 [`Market::as_quote_market`] 唯一判定点（issue #1673）。
pub fn derive_price_channel(
    kind: InstrumentType,
    market: Market,
    symbol: &str,
    constant_unit_price: Option<i64>,
) -> PriceChannel {
    if constant_unit_price.is_some() {
        return PriceChannel::Constant;
    }
    match kind {
        InstrumentType::Stock | InstrumentType::Etf if market.as_quote_market().is_some() => {
            PriceChannel::Quote
        }
        // 股票现价归同步通道（ADR-0036 决策 1）：市场未知即无任何价格来源。
        // ETF 不在此列——自建标的类型白名单含 ETF，录价入口开放，归手动通道。
        InstrumentType::Stock => PriceChannel::None,
        InstrumentType::Fund if is_six_digit_code(symbol) => PriceChannel::FundNav,
        _ => PriceChannel::Manual,
    }
}
