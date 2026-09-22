//! 市场闭集单点（issue #1673）：「哪个市场走哪条行情路径」的跨 crate 唯一事实源。
//!
//! 三个类型各答一个问题，成员与拼写全部同源于 [`Market`] 的 `closed_set!` 清单
//!（字符串字面量每变体只出现一次；[`QuoteMarket`] / [`StockRoute`] 不携带第二份
//! 字面量，投影全部骑行 [`Market`] 产物）：
//! - [`Market`]：标的挂牌市场的七值闭集（沪/深/港/美股三交易所/未知），即
//!   `instruments.market` 的库层 CHECK 闭集（ADR-0081 决策 5），照
//!   `InstrumentType` 先例派生（ADR-0108）：DB 读边界 parse 一次、未知值报错附
//!   合法值清单、CHECK 字面量测试期互核；
//! - [`QuoteMarket`]：可路由子集（六值）——能构造行情查询键的市场；可路由性
//!   唯一判定点是 [`Market::as_quote_market`]，价格通道派生单点与同步编排分区
//!   经它消费（通道分区产 [`QuoteMarket`]），未知市场在类型上无法进入查询路径；
//! - [`StockRoute`]：按代码查询/创建解析流程内部的路由小闭集（沪深港 + 聚合值
//!   `Us`，ADR-0081 决策 2 修订重申）——`us` 是路由聚合值**不是市场成员**：
//!   不落库、不出响应、不进 [`Market`]（ADR-0130 换源后美股三市场共用单查询，
//!   精确交易所由行情响应自报后缀判定）。
//!
//! 市场能力的唯一住址在本模块（ADR-0103 同构、词汇表「市场（Market）」词条）：
//! 可路由性（[`Market::as_quote_market`]）、美股聚合路由（[`QuoteMarket::
//! as_stock_route`]）、报价币种按来源固定（[`derive_quote_currency`]，ADR-0081）。
//! 行情/K 线查询键的**拼写**是数据源词汇，住行情同步域取数单元（腾讯模块本地），
//! 本模块不携带；两域经类型依赖衔接，「市场 → 查询键」不存在第二份口径。

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use ledger_infra::closed_set::closed_set;

closed_set! {
/// 标的挂牌市场闭集（与 `instruments.market` 的 CHECK 约束（V002）一一对应，
/// ADR-0081 决策 5）。七份表示（enum / `ALL` / `as_str` / `parse` / `Display`）
/// 由 `closed_set!` 宏同体派生（ADR-0108，照 `InstrumentType` 先例）：字符串
/// 字面量每变体只出现一次，漏登/漏臂漂移不可表达。
///
/// 未知（Unknown）是合法成员：场外基金没有交易所市场概念（ADR-0038）、手动
/// 创建未指定市场即 unknown（ADR-0081）——但它是**不可路由**市场（见
/// [`Market::as_quote_market`]），永无行情路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Market {
    /// 上海证券交易所（沪）。
    Sh => "sh",
    /// 深圳证券交易所（深）。
    Sz => "sz",
    /// 香港联合交易所（港）。
    Hk => "hk",
    /// 纳斯达克（美股三市场之一；行情查询经聚合路由 `us` 单查询，ADR-0130）。
    Nasdaq => "nasdaq",
    /// 纽约证券交易所。
    Nyse => "nyse",
    /// 美国证券交易所（AMEX）。
    Amex => "amex",
    /// 未知市场：合法落库但不可路由（ADR-0081：永无行情，同步计入跳过统计）。
    Unknown => "unknown",
}
err_label = "市场",
err_code = "instrument.market-unknown",
}

// serde：以与 `instruments.market` 同形的小写字符串序列化（wire 格式与市场闭集
// 存储形状逐字一致）；反序列化复用 [`Market::parse`]，未知值报错文案与 parse
// 同源。先例：[`crate::model::InstrumentType`]。
impl Serialize for Market {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for Market {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Market::parse(&s).map_err(serde::de::Error::custom)
    }
}

// OpenAPI（utoipa）：闭集枚举以小写字符串枚举值入文档，与 wire 格式一致
//（先例：`InstrumentType`，内联 schema；枚举值由 [`Market::ALL`] 同源驱动）。
impl PartialSchema for Market {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(Market::ALL.map(|m| m.to_string())))
                .description(Some(
                    "标的挂牌市场（闭集，小写字符串，与 instruments.market 一致）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for Market {}

impl ToSql for Market {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for Market {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        Market::parse(value.as_str()?).map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

impl Market {
    /// 可路由性唯一判定点（issue #1673）：市场能否构造行情查询键。价格通道派生
    /// 单点（`derive_price_channel` 的行情通道守卫）与同步编排分区都经本方法
    /// 消费——「行情通道 ⇔ 可路由市场」由同一判定承载，不存在第二份市场能力
    /// 口径；unknown 返回 None，六值可路由市场返回对应 [`QuoteMarket`]。
    pub fn as_quote_market(self) -> Option<QuoteMarket> {
        match self {
            Market::Sh => Some(QuoteMarket::Sh),
            Market::Sz => Some(QuoteMarket::Sz),
            Market::Hk => Some(QuoteMarket::Hk),
            Market::Nasdaq => Some(QuoteMarket::Nasdaq),
            Market::Nyse => Some(QuoteMarket::Nyse),
            Market::Amex => Some(QuoteMarket::Amex),
            Market::Unknown => None,
        }
    }
}

/// 可路由市场子集（issue #1673）：能构造行情查询键的六值市场——[`Market`]
/// 去掉 unknown。通道分区的查询单元（`QuoteQuery`）以本类型承载市场位：
/// 不可路由市场在类型上无法进入查询路径，运行时「查询键不可路由」兜底不存在。
///
/// 本类型不携带字符串事实：[`QuoteMarket::as_str`] 骑行 [`Market::as_str`]
///（[`QuoteMarket::as_market`] 反投影），市场字面量的唯一住处在 [`Market`]
/// 的 `closed_set!` 清单。新增可路由市场 = [`Market`] 加成员 + 两处穷尽 match
///（[`Market::as_quote_market`] / [`QuoteMarket::as_market`]）编译红强制随迁。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuoteMarket {
    Sh,
    Sz,
    Hk,
    Nasdaq,
    Nyse,
    Amex,
}

impl QuoteMarket {
    /// 可路由子集全量清单（穷尽遍历消费：钉形测试按市场迭代）。
    pub const ALL: [QuoteMarket; 6] = [
        QuoteMarket::Sh,
        QuoteMarket::Sz,
        QuoteMarket::Hk,
        QuoteMarket::Nasdaq,
        QuoteMarket::Nyse,
        QuoteMarket::Amex,
    ];

    /// 回到市场闭集成员（反投影）：闭集字符串骑行 [`Market::as_str`]，
    /// 不携带第二份字面量。
    pub const fn as_market(self) -> Market {
        match self {
            QuoteMarket::Sh => Market::Sh,
            QuoteMarket::Sz => Market::Sz,
            QuoteMarket::Hk => Market::Hk,
            QuoteMarket::Nasdaq => Market::Nasdaq,
            QuoteMarket::Nyse => Market::Nyse,
            QuoteMarket::Amex => Market::Amex,
        }
    }

    /// 闭集字符串（DB 存储/wire 形状）：骑行 [`Market::as_str`] 同源。
    pub const fn as_str(self) -> &'static str {
        self.as_market().as_str()
    }

    /// 按代码查询/创建解析流程的路由值（聚合判定单点）：美股三市场共用
    /// 聚合路由 `Us` 单查询（行情源不区分交易所，ADR-0130），沪深港为精确
    /// 市场。行情键拼装的「美股聚合」决策只在这里，数据源侧消费
    /// [`StockRoute`] 拼前缀，不再自行判市场。
    pub const fn as_stock_route(self) -> StockRoute {
        match self {
            QuoteMarket::Sh => StockRoute::Sh,
            QuoteMarket::Sz => StockRoute::Sz,
            QuoteMarket::Hk => StockRoute::Hk,
            QuoteMarket::Nasdaq | QuoteMarket::Nyse | QuoteMarket::Amex => StockRoute::Us,
        }
    }
}

impl std::fmt::Display for QuoteMarket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 按代码查询/创建解析流程内部的路由小闭集（issue #1673 / ADR-0081 决策 2 修订）：
/// 沪深港为精确市场路由，`Us` 为美股聚合路由值——行情源对美股不区分交易所
///（查询键统一 `us<代码>`），精确交易所由行情响应自报后缀判定。
///
/// `Us` **不是市场成员**（ADR-0081 决策 2 修订重申）：不落库、不出响应、不进
/// [`Market`]；只活在查询路由内部，消费方为行情取数单元（按路由拼查询键）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StockRoute {
    Sh,
    Sz,
    Hk,
    /// 美股聚合路由：三交易所共用单查询，精确归属由响应自报。
    Us,
}

impl StockRoute {
    /// 路由词汇（沪深港拼写与市场闭集同形、`us` 是聚合路由值自身的名字）：
    /// 供测试断言与日志；数据源查询键的拼写住行情同步域取数单元，不经本方法。
    pub const fn as_str(self) -> &'static str {
        match self {
            StockRoute::Sh => "sh",
            StockRoute::Sz => "sz",
            StockRoute::Hk => "hk",
            StockRoute::Us => "us",
        }
    }
}

/// 报价币种缺省推导（ADR-0037 决策 2；美股三市场→USD 见 ADR-0081）：币种按
/// 来源固定——沪深→人民币、港→港币、美股三市场→美元、未知→人民币（穷尽
/// match，新增市场变体编译红强制决策）。
///
/// 依据：标的币种不参与买卖账务（持仓批次成本币种 = 账户币种），仅影响行情/
/// 市值折算展示。市场能力随类型单点（词汇表「市场（Market）」）：可路由性、
/// 聚合路由与本推导是市场闭集的关联能力，唯一住址在投资域市场类型上。
pub fn derive_quote_currency(market: Market) -> &'static str {
    match market {
        Market::Hk => "HKD",
        Market::Nasdaq | Market::Nyse | Market::Amex => "USD",
        // 沪深与未知市场均落人民币
        Market::Sh | Market::Sz | Market::Unknown => "CNY",
    }
}
