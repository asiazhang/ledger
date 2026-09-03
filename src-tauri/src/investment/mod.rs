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
//! - [`crud`]：标的字典 / 汇率 / 现价列表与写入、标的搜索（含统一模糊搜索语义）、
//!   手动创建守卫与自建标的删除守卫；
//! - [`financial_freedom`]：财务自由度口径——可投资资产 × 3% 安全提取率对
//!   年度预算总额的覆盖比例（只读，ADR-0048）；
//! - [`fund`]：场外基金接入——6 位代码校验、详情落库、AI 降级建行、
//!   按代码即拉注入接缝（`add_fund_by_code_with`）；
//! - [`holdings`]：时点持仓（AsOfHolding）推算单点；
//! - [`manual_price`]：手动报价两落点（价格历史周采样 + 现价缓存映像规则）；
//! - [`predicates`]：「持仓标的」判定谓词单点（`INVESTED_EXISTS`）；
//! - [`prices`]：价格写入单点——现价缓存 upsert、价格历史周采样 upsert、
//!   价格刻度换算（`PRICE_UNITS_PER_FEN` / `price_value_to_cents`）、东财来源标记；
//! - [`reports`]：已实现盈亏汇总查询；
//! - [`trade`]：buy/sell 协议三件套与买卖明细投影（`TransactionTrade`）；
//! - [`trend`]：单标的 / 组合走势查询。
//!
//! 协议事务契约（ADR-0033）与可卖数量守卫零变化：prepare 校验归一化（不落库）、
//! apply 应用副作用（buy 建仓 / sell 卖出匹配）、revert 回退副作用（buy 守卫+清理 /
//! sell 回补）；交易行写入由核心交易域行为层编排（经 Writer 接缝），本域不再反向
//! 依赖核心交易域的行更新（双向依赖已斩断，issue #70）。
//!
//! 依赖方向恒为「壳层 → investment → 基础设施」，本模块不反向依赖壳层；
//! 对 `transaction::amount` / `transaction::search_text` 的消费属域间横向依赖
//! （ADR-0056 决策 2 允许）。IPC 参数解包、事务边界、命令注册和失效信号发射
//! 留在投资命令壳层（`commands::investment`）。

pub mod crud;
pub mod financial_freedom;
pub mod fund;
pub mod holdings;
pub mod manual_price;
pub mod predicates;
pub mod prices;
pub mod reports;
pub mod trade;
pub mod trend;

/// 域集中模型（#422 模型域化，样板先例：`reports::model`）：财务自由度类型
/// 并入本域集中模型（自由度归投资域，ADR-0048 既有裁决），经域路径逐类型
/// 再导出（禁止 glob），消费方经域路径显式 import。
mod model;

pub use model::FinancialFreedomOverview;

/// 域 API 再导出：调用面用域语言短名（`investment::list_instruments` 等），
/// 与 ADR-0056 阶段 1 定格形状一致（先例：`item::domain`、`merchants::crud`）。
/// 模块级接缝（[`holdings`] / [`prices`] / [`predicates`]）按样板留在模块路径
/// 消费（先例：`item::guard` / `item::cost` 不再导出到根）。
pub use crud::{
    create_exchange_rate, create_instrument, create_instrument_manual, create_market_price,
    delete_instrument, list_exchange_rates, list_holdings, list_instruments, list_market_prices,
};
pub use financial_freedom::query_financial_freedom;
pub use fund::{
    FundCreateOutcome, add_fund_by_code_with, create_fund_degraded, is_six_digit_code,
    persist_fund_detail, validate_fund_code,
};
pub use manual_price::record_manual_price;
pub use reports::query_realized_pnl_summary;
// 投资交易对外出口收窄为 prepare/apply/revert 三件套（issue #72 / spec #69）：
// 校验归一化（prepare）、应用副作用（apply）、回退副作用（revert）各一个入口，
// 不再暴露 create/update/cleanup/reverse 等散落函数；行写入经交易行为层编排。
pub use trade::{Plan, apply, get_transaction_trade, prepare, revert};
pub use trend::{query_instrument_price_trend, query_portfolio_value_trend};

#[cfg(test)]
mod tests;
