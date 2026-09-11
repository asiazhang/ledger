//! 投资领域模块（Instrument / Holding / TransactionTrade / PortfolioValueTrend，
//! spec #69 / ADR-0015 / ADR-0019 / ADR-0036 / ADR-0038；域目录化 #401 / ADR-0056）。
//!
//! 职责：标的字典、市场数据（现价缓存 / 价格历史 / 汇率历史）与汇率录入、
//! 买入/卖出协议（prepare/apply/revert 三件套，issue #72）、时点持仓推算、
//! 走势与盈亏查询、手动报价、场外基金接入（按代码即拉 / AI fund 增强）、
//! 「持仓标的」判定谓词与价格写入单点（现价缓存 upsert / 价格历史周采样
//! upsert，自 `sync::persist` 随域归位迁入）、财务自由度口径（#405 自命令壳层
//! 迁入）。
//!
//! 接缝（域语言短名经本入口再导出，调用面用 `investment::` 前缀）：
//! - [`command`]：同步命令（op 载荷形态、产出单点与重放分派，issue #861）——
//!   标的字典 / 汇率 / 用户侧价格写入全域进 OpLog，东财行情外拉数据不进 op；
//! - [`channel`]：价格通道派生（PriceChannel，issue #1060）——类型 × 市场 × 代码
//!   → 行情 / 净值 / 手动报价 / 无来源的判定单点，同步分区与标的读投影共用；
//! - [`crud`]：标的字典 / 汇率 / 现价列表与写入、标的搜索（含统一模糊搜索语义）、
//!   手动创建守卫与自建标的删除守卫；
//! - [`financial_freedom`]：财务自由度口径——可投资资产 × 3% 安全提取率对
//!   年度预算总额的覆盖比例（只读，ADR-0048）；
//! - [`fund`]：场外基金接入——6 位代码校验、行情接入落库半边（`adopt_fund_quote`）、
//!   AI 降级建行、按代码即拉注入接缝（`add_fund_by_code_with`）；
//! - [`holdings`]：时点持仓（AsOfHolding）推算单点；
//! - [`lots`]：持仓批次（security_lots）单点——取批次、逐批次 FIFO 分摊与
//!   耗尽批次成本闭合、结转成本合计、修改/删除路径的两个精确回补原语
//!   （issue #1018，父 spec #1005 决策 D4）；
//! - [`manual_price`]：手动报价两落点（价格历史周采样 + 现价缓存映像规则）；
//! - [`model`]：域集中模型——全量投资类型与财务自由度总览（#422 模型域化随域
//!   归位），经本入口逐类型再导出（禁止 glob）；行情 DTO 已随 ADR-0103 收口为
//!   [`quote`] 模块的统一载荷；
//! - [`predicates`]：「持仓标的」判定谓词单点（`INVESTED_EXISTS`）；
//! - [`prices`]：价格写入单点——现价缓存 upsert、价格历史周采样 upsert、
//!   价格刻度换算（`PRICE_UNITS_PER_FEN` / `price_value_to_cents`）、东财来源标记；
//! - [`quote`]：行情接入接缝（QuoteAdoption，ADR-0103）——统一报价载荷
//!   `Quote` 与落库半边 `adopt_quote`（建档 + 落现价一体）；查询半边实现在
//!   行情同步域网络层（`sync::fetch_fund_quote_production` /
//!   `sync::fetch_stock_quote_production`），统一注入签名 `(代码, 市场)`；
//! - [`reports`]：已实现盈亏汇总与按币种累计收益查询（issue #1077）；
//! - [`source`]：交易列表标的来源反查（spec #704 / issue #709，按生成交易 id
//!   批量取证券交易记录指向的标的展示字段）；
//! - [`split`]：份额调整（split）批次成本重述单点——按比例重述在用批次
//!   （尾差归末批次）与 `security_lot_adjustments` 审计落库（ADR-0106 决策 2/3，
//!   issue #1049）；
//! - [`stock`]：股票按（市场，代码）查询的领域规则——代码形态 → 市场单点推断、
//!   报价币种推导（issue #693 / ADR-0081；东财访问在 `sync::stock`）；
//! - [`trade`]：buy/sell/convert/split 协议分派与买卖/转换明细投影
//!   （`TransactionTrade` / `TransactionConvert`）；
//! - [`trend`]：单标的 / 组合走势查询。
//! - [`unwind`]：持仓副作用撤销（Unwind）——修改/删除路径的守卫 → 级联/回补 → 清理
//!   模板单点 `remove(conn, id, kind, mode)`，`trade.*` 守卫码与文案随迁（issue #1020，
//!   父 spec #1005 决策 D2/D3）；
//!
//! 协议事务契约（ADR-0033）：prepare 校验归一化（不落库）、apply 应用副作用
//! （buy 建仓 / sell 卖出匹配 / convert 两腿结转）、revert 回退副作用（修改路径：
//! buy 与 convert 转换链+在用占用守卫+清理 / sell 回补）、release_for_delete 承载删除路径
//! （sell 回补 / buy 与 convert 级联+清理，issue #940 / #979 / ADR-0097 / ADR-0099）——
//! 两者均为薄委托，守卫与清理模板归 [`unwind`]；交易行写入由核心交易域行为层编排
//! （经 Writer 接缝），本域不再反向依赖核心交易域的行更新（双向依赖已斩断，issue #70）。
//!
//! 依赖方向恒为「壳层 → investment → 基础设施」，本模块不反向依赖壳层；
//! 对 `transaction::amount` / `transaction::search_text` 的消费属域间横向依赖
//! （ADR-0056 决策 2 允许）。IPC 参数解包、事务边界、命令注册和失效信号发射
//! 留在投资命令壳层（`commands::investment`）。

pub mod channel;
pub mod command;
pub mod crud;
pub mod financial_freedom;
pub mod fund;
pub mod holdings;
pub mod lots;
pub mod manual_price;
pub mod predicates;
pub mod prices;
pub mod quote;
pub mod reports;
pub mod source;
pub mod split;
pub mod stock;
pub mod trade;
pub mod trend;
pub mod unwind;

/// 域集中模型（#422 模型域化随域归位，样板先例：`reports::model`）：全量投资
/// 类型与财务自由度类型（自由度归投资域，ADR-0048 既有裁决）集中本文件，经
/// 域路径逐类型再导出（禁止 glob），消费方经域路径显式 import。行情 DTO 已随
/// ADR-0103 收口为 [`quote`] 模块的统一载荷。
mod model;

pub use model::{
    AccountPnl, AddFundResult, AddStockInstrumentResult, CurrencyCumulativePnl, CurrencyPnl,
    FinancialFreedomOverview, Holding, Instrument, InstrumentInput, InstrumentListFilter,
    InstrumentListResult, InstrumentPnl, InstrumentPriceTrend, InstrumentSourceDisplay,
    InstrumentType, ManualPriceInput, ManualPriceResult, MarketPrice, MarketPriceInput, PnlFilter,
    PortfolioTrendPoint, PortfolioValueTrend, PriceTrendPoint, RealizedPnlSummary,
    TransactionConvert, TransactionTrade, TrendRange, YearPnl,
};

/// 域 API 再导出：调用面用域语言短名（`investment::list_instruments` 等），
/// 与 ADR-0056 阶段 1 定格形状一致（先例：`item::domain`、`merchants::crud`）。
/// 模块级接缝（[`holdings`] / [`prices`] / [`predicates`]）按样板留在模块路径
/// 消费（先例：`item::guard` / `item::cost` 不再导出到根）。
pub use channel::{PriceChannel, derive_price_channel};
pub use command::{ExchangeRateCommand, InstrumentCommand, PriceCommand};
pub use crud::{
    create_exchange_rate, create_instrument, create_instrument_manual, create_market_price,
    delete_instrument, get_instrument, list_exchange_rates, list_holdings, list_instruments,
    list_market_prices,
};
pub use financial_freedom::query_financial_freedom;
pub use fund::{
    FundCreateOutcome, add_fund_by_code_with, adopt_fund_quote, create_fund_degraded,
    is_six_digit_code, validate_fund_code,
};
pub use manual_price::record_manual_price;
pub use quote::{Quote, QuoteAdoptionInput, QuoteAdoptionOutcome};
pub use reports::{query_cumulative_pnl_summary, query_realized_pnl_summary};
pub use source::source_display_by_transaction_ids;
pub use stock::{
    ResolvedStockCode, StockCreateOutcome, StockCreateRoute, StockEnhancePlan,
    add_stock_instrument_with_quote, adopt_stock_quote, create_stock_degraded,
    derive_quote_currency, fetch_stock_quote_for_add, is_stock_lookup_miss,
    resolve_add_stock_channel, resolve_stock_quote_candidates, route_stock_creation,
};
// 投资交易对外出口收窄为 prepare/apply/revert 三件套 + 删除路径专用 release_for_delete
// （issue #72 / spec #69 / #940 / ADR-0097）：校验归一化（prepare）、应用副作用（apply）、
// 回退副作用（revert，修改路径）、删除路径持仓回退（release_for_delete：sell 回补 /
// buy 级联）各一个入口（两者为 [`unwind`] 的薄委托，issue #1020），不再暴露
// create/update/cleanup/reverse 等散落函数；行写入经交易行为层编排。
/// 同步重放的计划重建接缝（issue #861）：crate 内供交易行为层重放入口消费。
pub(crate) use command::{
    replay_exchange_rate_command, replay_instrument_command, replay_price_command,
};
pub use trade::{
    Plan, apply, convert_fields_by_transaction_ids, get_transaction_convert, get_transaction_trade,
    prepare, release_for_delete, revert,
};
pub(crate) use trade::{replay_convert_plan, replay_plan};
pub use trend::{query_instrument_price_trend, query_portfolio_value_trend};

#[cfg(test)]
mod tests;
