//! 投资域集中模型（#422 随域归位）：金融工具、持仓、行情价、已实现盈亏、
//! 标的列表分页与财务自由度总览。
//!
//! 自全局模型目录迁入本域（#417 归属原则）；财务自由度并入为既有裁决
//! （自由度归投资域，ADR-0048）。基金与股票的行情 DTO（issue #693）随 #422 Q11
//! 归属修正迁入本域后，又随 ADR-0103 收口为行情接入接缝的**统一报价载荷**
//! [`super::quote::Quote`]（深层统一：不再有嵌套的通道专属载荷），本文件不再
//! 承载行情 DTO。全部类型经 `investment` 域路径逐类型再导出，消费方经域路径
//! 显式 import，禁止 glob。

use rusqlite::types::{FromSql, FromSqlError, ToSql, ToSqlOutput, ValueRef};
use serde::{Deserialize, Serialize};
use utoipa::openapi::{ObjectBuilder, RefOr, Schema, Type};
use utoipa::{PartialSchema, ToSchema};

use super::channel::PriceChannel;
use crate::closed_set::closed_set;
use crate::db::query::FromRow;

closed_set! {
/// 金融工具类型闭集（与 `instruments.instrument_type` 的 CHECK 约束（V002）
/// 一一对应）。五份表示（enum / `ALL` / `as_str` / `parse` / `Display`）由
/// `closed_set!` 宏同体派生（ADR-0108）：字符串字面量每变体只出现一次，
/// 漏登/漏臂漂移不可表达；随 ADR-0102 既定形态收口，serde 由
/// `rename_all = "snake_case"` derive（与 `Display`/`FromStr` 平行的第二套
/// 字符串面、漂移无守门）改为手写 impl 骑行 `as_str`/`parse`（wire 形状
/// 逐字不变：5 变体全单词，`snake_case` 展开与 `as_str` 同形）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentType {
    Stock => "stock",
    Fund => "fund",
    Bond => "bond",
    Etf => "etf",
    Other => "other",
}
err_label = "金融工具类型",
err_code = "instrument.type-unknown",
}

// serde：以与 `instruments.instrument_type` 同形的小写字符串序列化（wire 格式
// 与 `rename_all = "snake_case"` 时代的 JSON 形状逐字一致）；反序列化复用
// [`InstrumentType::parse`]，未知值报错文案与 parse 同源（serde 包装后附位置
// 信息）。先例：[`crate::transaction::TransactionKind`]。
impl Serialize for InstrumentType {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for InstrumentType {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        InstrumentType::parse(&s).map_err(serde::de::Error::custom)
    }
}

// OpenAPI（utoipa）：闭集枚举以小写字符串枚举值入文档，与 wire 格式一致
// （先例：`TransactionKind`，内联 schema，消费方字段直接嵌入、无需注册组件；
// 枚举值由 `closed_set!` 宏生成的 [`InstrumentType::ALL`] 同源驱动）。
impl PartialSchema for InstrumentType {
    fn schema() -> RefOr<Schema> {
        RefOr::T(Schema::Object(
            ObjectBuilder::new()
                .schema_type(Type::String)
                .enum_values(Some(InstrumentType::ALL.map(|k| k.to_string())))
                .description(Some(
                    "金融工具类型（闭集，小写字符串，与 instruments.instrument_type 一致）",
                ))
                .build(),
        ))
    }
}

impl ToSchema for InstrumentType {}

impl ToSql for InstrumentType {
    fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
        Ok(ToSqlOutput::from(self.as_str()))
    }
}

impl FromSql for InstrumentType {
    fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
        InstrumentType::parse(value.as_str()?).map_err(|e| FromSqlError::Other(Box::new(e)))
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct Instrument {
    pub id: String,
    pub symbol: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub name: Option<String>,
    pub currency_code: String,
    pub market: String,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
    /// 字典条目来源（自建标的，ADR-0036 决策 2）：'eastmoney' 同步 | 'manual' 手动，
    /// 与价格侧 source 同词表但语义正交；来源随行终身不变（upsert/同步均不改写）。
    pub source: String,
    /// 最新市场价格（万分之一元，0.0001 元，ADR-0038 价格刻度），同步来源；无行情时为空。
    pub price_cents: Option<i64>,
    /// 是否持有该标的（有当前持仓批次 remaining_quantity > 0，派生自 security_lots）。
    pub invested: bool,
    /// 价格写入通道（派生事实，不落库，issue #1060）：后端按类型 × 市场 × 代码
    /// 单点派生（见 [`super::channel`]），前端据此放行单标的走势与开放录价入口，
    /// 不再自行按类型与市场推断。
    pub price_channel: PriceChannel,
}

#[derive(Debug, Deserialize)]
pub struct InstrumentInput {
    pub symbol: String,
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    pub name: Option<String>,
    pub currency_code: String,
    pub market: Option<String>,
}

/// 标的列表查询过滤条件（服务端分页 + 搜索）。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstrumentListFilter {
    /// 对 symbol / name 的大小写不敏感子串匹配。
    pub search: Option<String>,
    /// 交易市场精确匹配（sh / sz / hk / unknown）。
    pub market: Option<String>,
    /// 标的类型过滤（stock/fund/bond/etf/other）：同码异类型消歧用（issue #294）。
    #[serde(rename = "type")]
    pub kind: Option<InstrumentType>,
    /// 只看持仓标的：仅返回有当前持仓（remaining_quantity > 0）的标的。
    pub only_invested: Option<bool>,
    /// 页码，从 1 开始，默认 1。
    pub page: Option<usize>,
    /// 每页条数，默认 50，上限 500。
    pub page_size: Option<usize>,
}

/// 标的列表分页结果。
#[derive(Debug, Serialize, ToSchema)]
pub struct InstrumentListResult {
    pub items: Vec<Instrument>,
    /// 满足过滤条件的总条数（用于分页条）。
    pub total: i64,
}

/// 按代码即拉添加基金的结果（issue #301 / ADR-0038 决策 1）：标的行落库 +
/// 现价写入状态。名称与东财分类来自东方财富权威数据；未取到净值时仅建标的、
/// 不落现价（`price_written=false`，IPC 层据此不广播价格失效信号）。
#[derive(Debug, Serialize)]
pub struct AddFundResult {
    pub instrument_id: String,
    pub symbol: String,
    /// 东财权威名称（已回填标的行）。
    pub name: String,
    /// 东财基金分类（如「混合型-灵活」），展示透传，不落库。
    pub fund_class: String,
    /// 最新单位净值（万分之一元，ADR-0038 价格刻度）；未取到为 None。
    pub nav_cents: Option<i64>,
    /// 净值日期（ISO 日期，兼任净值同步水位）；未取到为 None。
    pub nav_date: Option<String>,
    /// 是否落了现价缓存（价格失效信号的广播判定依据，ADR-0031：零变化不广播）。
    pub price_written: bool,
}

/// 「添加投资标的」股票侧（沪/深/港/美股通道）按代码添加的结果（issue #697 /
/// ADR-0081）：标的行落库 + 识别回显投影——东财权威名称、自动识别的类型
/// （行情命中 → stock、类型特征 → etf）、精确市场（美股为遍历命中的交易所
/// 归属）与最新价（万分之一元）。场外基金通道返回既有 [`AddFundResult`]。
#[derive(Debug, Serialize)]
pub struct AddStockInstrumentResult {
    pub instrument_id: String,
    /// 归一化代码（港股左补零至 5 位、美股大写）。
    pub symbol: String,
    /// 东财权威名称（已回填标的行）。
    pub name: String,
    /// 自动识别的类型（stock / etf）。
    #[serde(rename = "type")]
    pub kind: InstrumentType,
    /// 精确市场（sh / sz / hk / nasdaq / nyse / amex）。
    pub market: String,
    /// 报价币种（按市场推导：沪深→CNY、港→HKD、美股→USD）。
    pub currency_code: String,
    /// 最新价（万分之一元，ADR-0038 价格刻度）；停牌/无有效报价为 None。
    pub price_cents: Option<i64>,
    /// 价格日期（ISO 日期）；无有效时间戳为 None。
    pub price_date: Option<String>,
    /// 是否落了现价缓存（价格失效信号的广播判定依据，同 [`AddFundResult`]）。
    pub price_written: bool,
}

/// 交易买卖明细（issue #180）：一笔 buy/sell 交易在 `security_transactions` 扩展表
/// 中的投影（核心 `transactions` 行不含投资字段，见 ADR-0003 核心表 + 扩展表），
/// 供投资表单编辑模式回填标的/数量/价格/费用。`symbol`/`instrument_name` 为
/// JOIN `instruments` 带出的展示字段，保证回填后标的选择框可直接显示标的而非裸 id。
#[derive(Debug, Serialize, Clone)]
pub struct TransactionTrade {
    pub instrument_id: String,
    pub symbol: String,
    pub instrument_name: Option<String>,
    /// 标的类型闭集字面量（fund/stock/bond/etf/other）：前端表单据此切换录入权威
    /// 形态（基金 = 金额 + 份额必填、单价反算；其余 = 数量 + 单价，issue #302）。
    pub instrument_type: String,
    pub quantity: f64,
    pub price_cents: i64,
    pub fee_cents: Option<i64>,
}

/// 基金转换两腿明细（ADR-0099 / issue #979）：一笔 `convert` 交易在
/// `security_transactions` 扩展表中的投影（两腿同记录），供转换表单编辑模式回填。
///
/// `out_*` 为转出腿（转出标的恒为 `instrument_id` / `quantity`）、`in_*` 为转入腿
/// （`to_instrument_id` / `to_quantity`）；两侧金额是确认单权威（`out_amount_cents` /
/// `in_amount_cents`），两侧单价由前端按金额 ÷ 份额反算展示（金额权威、单价反算，
/// 与场外基金 buy/sell 同款，ADR-0038），故不随投影冗余携带。`carried_cost_cents`
/// 是行金额锚点（服务端按 FIFO 消耗算定的结转成本，非确认单金额）。
/// `symbol` / `instrument_name` 为 JOIN `instruments` 带出的展示字段，保证回填后
/// 标的选择框可直接显示标的而非裸 id。
#[derive(Debug, Serialize, Clone)]
pub struct TransactionConvert {
    pub out_instrument_id: String,
    pub out_symbol: String,
    pub out_instrument_name: Option<String>,
    pub out_quantity: f64,
    pub out_amount_cents: i64,
    pub in_instrument_id: String,
    pub in_symbol: String,
    pub in_instrument_name: Option<String>,
    pub in_quantity: f64,
    pub in_amount_cents: i64,
    /// 手续费（整数分）：如实记录，不进支出口径、不摊入持仓成本。
    pub fee_cents: i64,
    /// 结转成本（分）= 行金额锚点（转入批次总成本），审计展示用。
    pub carried_cost_cents: i64,
    /// 记账币种（行币种）。
    pub currency_code: String,
}

impl FromRow for TransactionConvert {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(TransactionConvert {
            out_instrument_id: row.get(0)?,
            out_symbol: row.get(1)?,
            out_instrument_name: row.get(2)?,
            out_quantity: row.get(3)?,
            out_amount_cents: row.get(4)?,
            in_instrument_id: row.get(5)?,
            in_symbol: row.get(6)?,
            in_instrument_name: row.get(7)?,
            in_quantity: row.get(8)?,
            in_amount_cents: row.get(9)?,
            fee_cents: row.get(10)?,
            carried_cost_cents: row.get(11)?,
            currency_code: row.get(12)?,
        })
    }
}

impl FromRow for TransactionTrade {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(TransactionTrade {
            instrument_id: row.get(0)?,
            symbol: row.get(1)?,
            instrument_name: row.get(2)?,
            instrument_type: row.get(3)?,
            quantity: row.get(4)?,
            price_cents: row.get(5)?,
            fee_cents: row.get(6)?,
        })
    }
}

/// 交易列表来源反查投影（spec #704 / issue #709，词汇表「来源列」）：按生成
/// 交易 id 反查证券交易记录 JOIN 标的字典的最小行——标的 id（来源实体 id）+
/// 代码与名称（来源列展示名原料），供核心交易域按页填充来源列。
#[derive(Debug, Clone)]
pub struct InstrumentSourceDisplay {
    /// 生成交易 id（security_transactions 主键，一交易至多一行）：调用方按它
    /// 把标的归位到交易行。
    pub transaction_id: String,
    /// 标的 id（来源实体 id，走势页签 focus 消费按它解析选中）。
    pub instrument_id: String,
    /// 标的代码（展示名原料，NOT NULL 恒在场）。
    pub symbol: String,
    /// 标的名称（可空，展示名原料）。
    pub name: Option<String>,
}

impl InstrumentSourceDisplay {
    /// 来源列展示名（走势页签标签惯例）：代码 + 名称空格连接，无名称（含空串）
    /// 退化为裸代码——代码 NOT NULL 保证展示名恒非空、链接恒可读。
    pub fn display_label(&self) -> String {
        match self.name.as_deref() {
            Some(name) if !name.is_empty() => format!("{} {}", self.symbol, name),
            _ => self.symbol.clone(),
        }
    }
}

impl FromRow for InstrumentSourceDisplay {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(InstrumentSourceDisplay {
            transaction_id: row.get(0)?,
            instrument_id: row.get(1)?,
            symbol: row.get(2)?,
            name: row.get(3)?,
        })
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Holding {
    pub id: String,
    pub account_id: String,
    pub instrument_id: String,
    pub quantity: f64,
    pub cost_basis_cents: i64,
    pub cost_currency_code: String,
    pub latest_price_cents: Option<i64>,
    pub latest_price_currency_code: Option<String>,
    /// 净值日期（透传 market_prices.nav_date，#303）：基金现价（= 最新公布
    /// 单位净值）携带，持仓可见现价对应哪天的净值；股票类恒 None。
    pub latest_nav_date: Option<String>,
    pub market_value_cents: Option<i64>,
    pub unrealized_pnl_cents: Option<i64>,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MarketPrice {
    pub id: String,
    pub instrument_id: String,
    pub price_cents: i64,
    pub currency_code: String,
    pub priced_at: String,
    /// 净值日期：场外基金现价（= 最新公布单位净值）携带，兼任净值同步水位
    /// （ADR-0038）；股票类现价无净值语义，恒为 None。
    pub nav_date: Option<String>,
    pub source: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub version: i64,
    pub device_id: String,
}

#[derive(Debug, Deserialize)]
pub struct MarketPriceInput {
    pub instrument_id: String,
    pub price_cents: i64,
    pub currency_code: String,
    pub priced_at: String,
    pub source: Option<String>,
}

/// 手动报价入参（issue #291 / ADR-0036）：标的 id + 日期（ISO YYYY-MM-DD）+ 价格
/// （万分之一元，ADR-0038 价格刻度）。后端命令不设「同步覆盖不到」守卫——
/// 录价入口收口在 UI 侧（ADR-0036 决策 1 修订）。
#[derive(Debug, Deserialize)]
pub struct ManualPriceInput {
    pub instrument_id: String,
    /// 报价对应的交易日（ISO YYYY-MM-DD）。
    pub date: String,
    /// 单价（万分之一元，价格刻度见 ADR-0038）。
    pub price_cents: i64,
}

/// 手动报价结果（issue #291）：两个落点各自的实际写入情况。
/// `history_written` = 价格历史周采样落库；`current_price_written` = 现价缓存
/// upsert（报价不早于最新价格点时写入；回填旧价为 false——现价是 PriceHistory
/// 最新一条的即时映像，回填不改变现价）。前端据此区分回执文案；价格失效信号
/// 证据经 [`ManualPriceResult::any_written`] 归一化，发射判定单点在 `signals`
/// 映射（ADR-0044 / issue #333）。
#[derive(Debug, Serialize)]
pub struct ManualPriceResult {
    pub history_written: bool,
    pub current_price_written: bool,
}

impl ManualPriceResult {
    /// 「实际写入任一落点」：价格失效信号证据（`WriteEvidence::PriceWritten`
    /// 载荷，ADR-0044）的域内归一化——任一落点实际写入即价格数据已变更。
    /// 「是否发信号」的判定单点在 `signals_for` 映射行，本方法只做结果 →
    /// 证据的形状翻译（增量同步结果 `written` 字段同款先例）。
    pub fn any_written(&self) -> bool {
        self.history_written || self.current_price_written
    }
}

/// 已实现盈亏汇总（ADR-0107）：盈亏页三视图（按年/按账户/按标的）+ 按币种分组总数。
/// 逐匹配「卖出明细」已退役（决策 1），明细数据本体（security_lot_sales 逐匹配行）
/// 不再随本投影返回。
#[derive(Debug, Serialize)]
pub struct RealizedPnlSummary {
    /// 按币种分组的已实现盈亏总数（决策 6：不做跨币种折算）。
    pub total: Vec<CurrencyPnl>,
    pub by_year: Vec<YearPnl>,
    pub by_account: Vec<AccountPnl>,
    pub by_instrument: Vec<InstrumentPnl>,
}

/// 按币种分组的已实现盈亏小计（ADR-0107 决策 6）：匹配行（security_lot_sales）币种口径，
/// 不做跨币种折算——原「各币种裸数字直接 SUM」的混算口径废止（多币种账户下合计是错的）。
#[derive(Debug, Serialize)]
pub struct CurrencyPnl {
    pub currency_code: String,
    pub realized_pnl_cents: i64,
}

/// 按币种分组的累计收益小计（issue #1077 / 词汇表「累计收益（CumulativePnl）」）：
/// 未实现盈亏（Holding）+ 已实现盈亏（RealizedPnl）两腿相加，按账户币种独立成组、
/// 不做跨币种折算（同 ADR-0107 决策 6 的持仓合计口径）。
///
/// **空值语义采 Holding 侧**：缺价 / 缺汇率的持仓其未实现腿为空值，不计入本组、
/// 不以零计入（与持仓视图合计既有的「跳过空值」语义一致）；已实现腿来自平仓匹配，
/// 不受当前持仓有无行情影响。某币种两腿皆空时该组不出现（由调用方渲染为空态）。
#[derive(Debug, Serialize)]
pub struct CurrencyCumulativePnl {
    pub currency_code: String,
    pub cumulative_pnl_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct YearPnl {
    pub year: String,
    pub currency_code: String,
    pub realized_pnl_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct AccountPnl {
    pub account_id: String,
    pub account_name: String,
    pub currency_code: String,
    pub realized_pnl_cents: i64,
}

#[derive(Debug, Serialize)]
pub struct InstrumentPnl {
    pub instrument_id: String,
    pub symbol: String,
    pub name: Option<String>,
    pub currency_code: String,
    pub realized_pnl_cents: i64,
}

#[derive(Debug, Deserialize)]
pub struct PnlFilter {
    pub account_id: Option<String>,
    pub instrument_id: Option<String>,
}

impl FromRow for Holding {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Holding {
            id: row.get(0)?,
            account_id: row.get(1)?,
            instrument_id: row.get(2)?,
            quantity: row.get(3)?,
            cost_basis_cents: row.get(4)?,
            cost_currency_code: row.get(5)?,
            latest_price_cents: row.get(6)?,
            latest_price_currency_code: row.get(7)?,
            latest_nav_date: row.get(8)?,
            market_value_cents: row.get(9)?,
            unrealized_pnl_cents: row.get(10)?,
            updated_at: row.get(11)?,
        })
    }
}

impl FromRow for MarketPrice {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(MarketPrice {
            id: row.get(0)?,
            instrument_id: row.get(1)?,
            price_cents: row.get(2)?,
            currency_code: row.get(3)?,
            priced_at: row.get(4)?,
            nav_date: row.get(5)?,
            source: row.get(6)?,
            created_at: row.get(7)?,
            updated_at: row.get(8)?,
            version: row.get(9)?,
            device_id: row.get(10)?,
        })
    }
}

impl FromRow for CurrencyPnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(CurrencyPnl {
            currency_code: row.get(0)?,
            realized_pnl_cents: row.get::<_, Option<i64>>(1)?.unwrap_or(0),
        })
    }
}

impl FromRow for CurrencyCumulativePnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(CurrencyCumulativePnl {
            currency_code: row.get(0)?,
            cumulative_pnl_cents: row.get(1)?,
        })
    }
}

impl FromRow for YearPnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(YearPnl {
            year: row.get(0)?,
            currency_code: row.get(1)?,
            realized_pnl_cents: row.get::<_, Option<i64>>(2)?.unwrap_or(0),
        })
    }
}

impl FromRow for AccountPnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(AccountPnl {
            account_id: row.get(0)?,
            account_name: row.get(1)?,
            currency_code: row.get(2)?,
            realized_pnl_cents: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
        })
    }
}

impl FromRow for InstrumentPnl {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(InstrumentPnl {
            instrument_id: row.get(0)?,
            symbol: row.get(1)?,
            name: row.get(2)?,
            currency_code: row.get(3)?,
            realized_pnl_cents: row.get::<_, Option<i64>>(4)?.unwrap_or(0),
        })
    }
}

impl FromRow for Instrument {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        // 派生事实先行（issue #1060）：价格通道由类型 × 市场 × 代码单点派生，
        // 不进 SQL 投影——SQL 无法引用 Rust 判定，行映射处消费域单点。
        let kind: InstrumentType = row.get(2)?;
        let market: String = row.get(5)?;
        let symbol: String = row.get(1)?;
        let price_channel = super::channel::derive_price_channel(kind, &market, &symbol);
        Ok(Instrument {
            id: row.get(0)?,
            symbol,
            kind,
            name: row.get(3)?,
            currency_code: row.get(4)?,
            market,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            version: row.get(8)?,
            device_id: row.get(9)?,
            source: row.get(10)?,
            price_cents: row.get(11)?,
            invested: row.get(12)?,
            price_channel,
        })
    }
}

// ---------------------------------------------------------------------------
// 走势查询（issue #138 / spec #135 / ADR-0019：PortfolioValueTrend）
// ---------------------------------------------------------------------------

/// 走势查询区间：可选起止 ISO 8601 日期，`None` 表示该侧不设界。
/// 前端预设区间（1 月 / 3 月 / 1 年 / 全部）换算成起止日期传入。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TrendRange {
    /// 起始日期（含），ISO 8601。
    pub start_date: Option<String>,
    /// 截止日期（含），ISO 8601。
    pub end_date: Option<String>,
}

/// 单标的走势采样点：周采样交易日 + 收盘价（报价币种万分之一元，ADR-0038 价格刻度）。
#[derive(Debug, Serialize)]
pub struct PriceTrendPoint {
    /// 周采样交易日（该周最后一个有报价交易日），ISO 8601 日期。
    pub date: String,
    /// 收盘价（万分之一元，报价币种）。
    pub price_cents: i64,
    /// 报价币种（港股 HKD、沪深 CNY）。
    pub currency_code: String,
}

/// 单标的走势：区间裁剪后的周采样点序列（PriceHistory 直出，从首个有效点开始）。
#[derive(Debug, Serialize)]
pub struct InstrumentPriceTrend {
    pub instrument_id: String,
    pub points: Vec<PriceTrendPoint>,
}

/// 组合走势采样点：该周各持仓标的「持有数量 × 周线价格」折算到本位币后的合计。
#[derive(Debug, Serialize)]
pub struct PortfolioTrendPoint {
    /// 所属 ISO 周的周一（周点 x 坐标，按周连续、缺口连点跨越），ISO 8601 日期。
    pub date: String,
    /// 该周组合总市值（分，本位币）。
    pub market_value_cents: i64,
}

/// 投资资产走势（PortfolioValueTrend）：组合市值周点曲线。
/// `points` 为空即无任何历史数据的空态（前端据此渲染引导文案）。
#[derive(Debug, Serialize)]
pub struct PortfolioValueTrend {
    /// 折算基准（本位币）。
    pub currency_code: String,
    pub points: Vec<PortfolioTrendPoint>,
}

// ---------------------------------------------------------------------------
// 财务自由度（issue #343 / ADR-0048；#422 自全局模型目录并入本域）
// ---------------------------------------------------------------------------

/// `financial_freedom` 命令返回的财务自由度总览（本位币口径，金额单位：分）。
///
/// 自由度 = 可投资资产 × 3% 安全提取率 ÷ 年度预算总额 × 100%（口径取舍见
/// docs/adr/0048-financial-freedom-ratio.md）。实时计算不落库；未设预算时
/// 分母为零、ratio 与 coverage_years 均为 0（占位引导在展示层，不回退实际支出）。
#[derive(Debug, Clone, Serialize)]
pub struct FinancialFreedomOverview {
    /// 自由度百分比（一位小数）
    pub ratio: f64,
    /// 分子：可投资资产合计（本位币，分）= Σ 折本位币持仓市值 + Σ 折本位币投资账户余额
    /// （排除隐藏账户；未录价持仓按空值语义不计入）
    pub numerator_cents: i64,
    /// 分母：年度预算总额（分）= Σ 月度预算 × 12 + Σ 年度预算（全部未删除，无窗口不滚动）
    pub denominator_cents: i64,
    /// 覆盖年数（一位小数）= 分子 ÷ 分母（零分母为 0）
    pub coverage_years: f64,
    /// 折算基准币种（全局默认币种）
    pub native_currency: String,
}
