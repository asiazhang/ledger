//! 行情同步域（MarketSync，#407 域目录化归位，ADR-0056）。
//!
//! HTTP 网络爬取、东财基金访问与增量同步编排在本域收口：
//! - [`http`]：HTTP 请求（含多主机切换、重试、限流冷却、Referer）与响应解析（报价
//!   / 日 K / 汇率 K），价格换算按精度位单点收口（批量报价与单点行情共用），可独立测试；
//! - [`fund`]：东财基金详情访问（按代码即拉，issue #301 / ADR-0038）；
//! - [`stock`]：东财股票单点行情访问——按（市场，代码）实时查询（issue #693 /
//!   ADR-0081），类型特征探测单点隔离；
//! - [`fund_nav`]：东财历史净值通道——lsjz 访问、报文解析、水位语义与基金分区
//!   编排（issue #303 / ADR-0038 决策 6）；
//! - [`persist`]：`fx_rate_history` 周采样 upsert（issue #137；价格写入单点已随
//!   投资域归位迁入 [`crate::investment::prices`]，#401 / ADR-0056）；
//! - [`incremental`]：增量同步编排（issue #103，#137 升级，#303 基金分区，#695
//!   ETF 纳入行情分区）——现价 upsert + 近两年日 K 回填周线落 `price_history` +
//!   汇率 K 线落 `fx_rate_history`（ADR-0019）+ 基金历史净值按水位增量回填
//!   （ADR-0038 决策 6）；
//! - [`model`]：域模型——增量同步结果类型（#407 随域归位；基金行情 DTO 已因
//!   #422 Q11 归属修正迁入 [`crate::investment::model`]）；
//! - `tests`：外挂测试（HTTP 层经本地 HTTP 服务独立测试，不依赖真实网络）。
//!
//! 标的全量同步（修字典）已整体退役（ADR-0081 决策 3，issue #698）：编排、
//! 进度/取消事件与中断状态随命令删除，股票字典修正归「按代码查询/创建带回
//! 权威名称」（投资域 `stock` / `crud`）。
//!
//! 依赖方向：本域消费基础设施（`db` / `error` / `events`），横向消费投资域
//! （价格写入单点 `prices` / 持仓谓词 / 基金代码判定）与核心交易域
//! （币种缺省推导 `transaction::amount`），不依赖壳层。壳层 `commands::sync`
//! 只做参数解包与信号发射，对外暴露 `sync_holding_prices` 增量同步（只刷价格）
//! 一个 IPC 命令。

mod fund;
mod fund_nav;
mod http;
mod incremental;
mod model;
mod persist;
mod stock;

#[cfg(test)]
mod tests;

pub use fund::fetch_fund_detail_production;
pub use incremental::do_incremental_sync;
pub use model::SyncHoldingPricesResult;
pub use stock::fetch_stock_quote_production;
