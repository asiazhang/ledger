//! 价格通道（PriceChannel，issue #1060）：「这个标的价格从哪里来」的后端派生
//! 事实——按标的类型、市场与代码把库内标的分进价格写入通道的判定单点。
//!
//! 分区口径即标的信息同步（InstrumentInfoSync）的通道能力分区（ADR-0036 /
//! ADR-0038 / issue #695）：行情通道（stock|etf 且市场已知）、净值通道（fund 且
//! 6 位真实代码）、手动报价通道（其余开放录价的行）、无来源（市场未知的股票类
//! ——行情不可达且录价入口不开放，ADR-0036 决策 1）。同步编排、标的读投影
//! （`Instrument::price_channel`）与走势放行、录价入口共用本单点：此前前端
//! 各自镜像推断（走势场内白名单 `hasMarketSource`、录价入口 `canManualPrice`），
//! 场外基金一等标的定案后漂移成第二口径（issue #1060），自本派生收口。

use super::fund::is_six_digit_code;
use super::model::InstrumentType;
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

/// 价格写入通道（派生事实，不落库）：随标的行投影输出，前端据此放行走势与
/// 开放录价入口。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceChannel {
    /// 行情通道：股票与场内 ETF，市场已知（可构造 secid），现价与周线经行情
    /// 同步写入（issue #695）。
    Quote,
    /// 净值通道：6 位真实代码的场外基金，净值经标的信息同步逐只写入
    /// （ADR-0038）。
    FundNav,
    /// 手动报价通道：同步覆盖不到但录价入口开放的行——自建标的（债券/ETF/其他）
    /// 与名称充代码的基金行（ADR-0036 决策 1）。
    Manual,
    /// 无价格来源：市场未知的股票类——行情不可达（无法构造 secid）且录价入口
    /// 不开放，走势与市值四消费方全部落空。
    None,
}

impl PriceChannel {
    /// 闭集全量清单（OpenAPI 枚举等消费；先例：`InstrumentType::ALL`）。
    pub const ALL: [PriceChannel; 4] = [
        PriceChannel::Quote,
        PriceChannel::FundNav,
        PriceChannel::Manual,
        PriceChannel::None,
    ];
}

impl std::fmt::Display for PriceChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PriceChannel::Quote => "quote",
            PriceChannel::FundNav => "fund_nav",
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
                    "价格写入通道（派生事实：quote 行情 / fund_nav 净值 / manual 手动报价 / none 无来源）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for PriceChannel {}

/// 行情通道的市场能力：已知市场（沪/深/港/美股三交易所）才可构造 secid 查询。
/// 与行情同步网络层的 `sync::http::secid_prefix` 同一闭集——同步侧绑定测试钉住
/// 两者一致（`sync::tests::instrument_info_sync`），改其一必同步另一。
pub fn quote_market(market: &str) -> bool {
    matches!(market, "sh" | "sz" | "hk" | "nasdaq" | "nyse" | "amex")
}

/// 价格通道派生单点：类型 × 市场 × 代码 → 通道。同步分区、标的读投影共用；
/// 判定顺序即语义——市场未知的股票先于手动报价兜底拦截（它连录价入口也不开放）。
pub fn derive_price_channel(kind: InstrumentType, market: &str, symbol: &str) -> PriceChannel {
    match kind {
        InstrumentType::Stock | InstrumentType::Etf if quote_market(market) => PriceChannel::Quote,
        // 股票现价归同步通道（ADR-0036 决策 1）：市场未知即无任何价格来源。
        // ETF 不在此列——自建标的类型白名单含 ETF，录价入口开放，归手动通道。
        InstrumentType::Stock => PriceChannel::None,
        InstrumentType::Fund if is_six_digit_code(symbol) => PriceChannel::FundNav,
        _ => PriceChannel::Manual,
    }
}
