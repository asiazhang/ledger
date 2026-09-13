//! 行情接入接缝（QuoteAdoption，ADR-0103）：把标的按代码从行情数据源接入
//! 账本的统一形状——「按代码查询主干道」（ADR-0081）在**新增标的路径**上的
//! 接缝化命名。基金与股票（含场内 ETF）是接缝背后的两个通道，形状进接缝、
//! 差异进通道。
//!
//! 两成员（ADR-0103 决策 1）：
//! - **查询半边**（按代码取行情，纯取数不落库）：实现在行情同步域网络层
//!   （`sync::fetch_fund_quote_production` / `sync::fetch_stock_quote_production`，
//!   根包；crate 拆分后跨包路径不再做 rustdoc 链接），以统一注入签名
//!   `FnMut(代码, 市场) -> Result<Quote>` 驱动——场外基金无交易所市场概念，
//!   市场位恒传通道字典市场 `unknown`（见 [`super::fund`]）；
//! - **落库半边**（[`adopt_quote`]，**建档 + 落现价一体**）：三个消费壳
//!   （添加基金一枪式 / 添加投资标的两段式 / AI 创建端点直调落库半边）都消费
//!   一体形态，拆开会把编排重新暴露给壳层（ADR-0103 决策 1）。
//!
//! 统一载荷 [`Quote`]（ADR-0103 决策 2）：代码、权威名称、价格、价格日期为
//! 公共成员；市场、类型提示、基金分类、净值日期等通道差异以可缺省成员承载。
//! 基金 / 股票各自的强约束（基金类型恒 fund、市场恒 unknown、场内必带精确
//! 市场与类型提示等）因此从类型形状退到**通道内判定**：
//! - 场外基金通道（[`super::fund`]）：市场恒 `unknown`、币种恒人民币、
//!   `priced_at` = 净值日期（净值日期即现价的行情日期）、净值日期随价格落库
//!   （兼任净值同步水位）；
//! - 场内通道（[`super::stock`]）：精确市场取自行情回显（候选解析单点产物）、
//!   `priced_at` = 写入时刻（无净值日期语义），类型取行情类型提示。
//!
//! 不引入 Rust trait（ADR-0103 决策 2）：依赖方向是 sync → investment（查询
//! 半边实现在 sync、落库半边在本模块），单一 trait impl 表达不了跨域两半；
//! 全库也没有「遍历一组 adapter 统一处理」的多态消费点，trait 是纯机制税。
//!
//! 界外通道（ADR-0103 决策 3）：手动报价（无查询步骤、带最新点映像规则）与
//! 标的信息同步（批量编排 + 净值水位增量）**不在接缝内**——接缝语义收窄为
//! 「按代码从数据源取行情 → 建档 → 落价」的新增标的路径；三条价格写入路径
//! （手动 / 同步 / 新增标的）共用价格写入单点
//! [`super::prices::upsert_market_price`]，共用发生在写入单点层而非接缝层。
//!
//! 纯重构：IPC / HTTP / AI 契约、错误码、schema 零变化（ADR-0103 决策 5）。

use rusqlite::Connection;

use super::crud;
use super::model::{InstrumentInput, InstrumentType};
use super::prices::{EASTMONEY_PRICE_SOURCE, MarketPriceWrite, upsert_market_price};
use ledger_infra::error::{AppError, Result};

/// 统一报价载荷（ADR-0103 决策 2）：基金与股票的行情投影同形——代码、权威
/// 名称、价格与价格日期为公共成员；市场、类型提示、基金分类、净值日期等通道
/// 差异以可缺省成员承载。基金详情 / 股票行情 / 基金净值三个旧 DTO 由此退役
///（深层统一：不再有嵌套的通道专属载荷）。
///
/// 价格一律已在访问层换算为万分之一元刻度（ADR-0038），未取到价格（停牌 /
/// 基金未公布净值）为 None。
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    /// 标的代码（场内为归一化形态：港股左补零至 5 位、美股大写）。
    pub code: String,
    /// 数据源权威名称（基金为东财简称、场内为东财回显名称）。
    pub name: String,
    /// 最新价格（万分之一元，0.0001 元，ADR-0038 价格刻度）。
    pub price_cents: Option<i64>,
    /// 价格日期（ISO 日期）：场外基金为净值日期、场内为行情的北京日历日。
    pub price_date: Option<String>,
    /// 精确市场（sh / sz / hk / nasdaq / nyse / amex）：场内通道携带；
    /// 场外基金无交易所市场概念，为 None（字典市场由通道判定为 unknown）。
    pub market: Option<String>,
    /// 类型提示（stock / etf，东财类型特征探测单点，ADR-0081）：场内通道携带；
    /// 场外基金无类型特征字段，为 None。
    pub kind_hint: Option<InstrumentType>,
    /// 东财基金分类（如「混合型-灵活」）：场外基金透传展示；场内为 None。
    pub fund_class: Option<String>,
    /// 净值日期（ISO 日期，兼任净值同步水位，ADR-0038）：场外基金携带；
    /// 场内现价无净值日期语义，为 None。
    pub nav_date: Option<String>,
}

impl Quote {
    /// 场内通道的精确市场（通道强约束：场内行情必携带回显市场）。缺省即内部
    /// 不一致——场外形态（market 缺省）不得经场内通道落库或投影，码化拒绝
    ///（先例：`sync.secid-unroutable` 的闭集外兜底），不静默降级为 unknown。
    pub fn stock_market(&self) -> Result<&str> {
        self.market
            .as_deref()
            .ok_or_else(|| AppError::coded("quote.market-missing", "行情缺少市场（内部不一致）"))
    }

    /// 场内通道的类型提示：缺省即 `Stock`（东财类型特征字段缺省/非零 → 股票，
    /// 与探测单点 `sync::stock`（行情同步域，根包）同判，ADR-0081）——误判代价仅类型标签，
    /// 已接受。
    pub fn stock_kind_hint(&self) -> InstrumentType {
        self.kind_hint.unwrap_or(InstrumentType::Stock)
    }
}

/// 落库半边输入（ADR-0103 决策 3 / 4）：通道判定出的字典形态与现价时点。
/// 通道语义留各自通道——现价时点（场外基金 = 净值日期、场内 = 写入时刻）、
/// 净值日期、市场与币种的强约束都由调用方通道判定后带入，接缝只执行建档 + 落价。
pub struct QuoteAdoptionInput<'a> {
    /// 落库类型（场外基金恒 Fund；场内取调用方提交类型 stock / etf）。
    pub kind: InstrumentType,
    /// 字典市场（场外基金恒 unknown；场内取行情精确市场）。
    pub market: &'a str,
    /// 报价币种（场外基金恒人民币；场内按市场推导，ADR-0037 决策 2）。
    pub currency_code: &'a str,
    /// 现价时点（ISO 日期）：场外基金为净值日期、场内为写入时刻。
    pub priced_at: &'a str,
    /// 净值日期（场外基金携带，兼任净值同步水位）；场内为 None。
    pub nav_date: Option<&'a str>,
}

/// 落库半边产出：标的 id + 是否落现价（价格失效信号的广播判定依据，
/// ADR-0031：零变化不广播）。回显投影（`AddFundResult` /
/// `AddStockInstrumentResult`）保持各自形状，由各通道自行构造（ADR-0103 决策 5）。
pub struct QuoteAdoptionOutcome {
    pub instrument_id: String,
    pub price_written: bool,
}

/// 行情接入落库半边（ADR-0103 决策 1）：按（代码，类型）幂等建/复用标的行
///（名称与市场随行回填，来源标记 manual，ADR-0036）+ 有价格时落现价缓存
///（价格写入单点），一体执行——三个消费壳都消费成品形态，不各自重排这两步。
pub fn adopt_quote(
    conn: &Connection,
    adoption: &QuoteAdoptionInput<'_>,
    quote: &Quote,
) -> Result<QuoteAdoptionOutcome> {
    let instrument_id = crud::create_instrument(
        conn,
        InstrumentInput {
            symbol: quote.code.clone(),
            kind: adoption.kind,
            name: Some(quote.name.clone()),
            currency_code: adoption.currency_code.to_string(),
            market: Some(adoption.market.to_string()),
        },
    )?;
    if let Some(price_cents) = quote.price_cents {
        upsert_market_price(
            conn,
            &MarketPriceWrite {
                instrument_id: &instrument_id,
                price_cents,
                currency_code: adoption.currency_code,
                priced_at: adoption.priced_at,
                nav_date: adoption.nav_date,
                source: Some(EASTMONEY_PRICE_SOURCE),
            },
        )?;
    }
    Ok(QuoteAdoptionOutcome {
        instrument_id,
        // 现价行整行覆盖（同值重复也 version+1），零写入仅在无价路径出现。
        price_written: quote.price_cents.is_some(),
    })
}
