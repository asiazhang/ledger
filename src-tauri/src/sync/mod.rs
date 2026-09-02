//! 行情同步域（MarketSync，#407 域目录化归位，ADR-0056）。
//!
//! HTTP 网络爬取、东财基金访问与增全量同步编排在本域收口：
//! - [`http`]：HTTP 请求（含多主机切换、重试、限流冷却、Referer）与响应解析（报价
//!   / 日 K / 汇率 K），可独立测试；
//! - [`fund`]：东财基金详情访问（按代码即拉，issue #301 / ADR-0038）；
//! - [`fund_nav`]：东财历史净值通道——lsjz 访问、报文解析、水位语义与基金分区
//!   编排（issue #303 / ADR-0038 决策 6）；
//! - [`persist`]：`instruments` 标的字典应用 + `fx_rate_history` 周采样 upsert
//!   （issue #89 / #137；价格写入单点已随投资域归位迁入
//!   [`crate::investment::prices`]，#401 / ADR-0056）；
//! - [`orchestrate`]：全量同步编排——市场分页遍历、进度事件推送、新增/更新汇总；
//! - [`incremental`]：增量同步编排（issue #103，#137 升级，#303 基金分区）——
//!   现价 upsert + 近两年日 K 回填周线落 `price_history` + 汇率 K 线落
//!   `fx_rate_history`（ADR-0019）+ 基金历史净值按水位增量回填（ADR-0038 决策 6）；
//! - [`model`]：域模型——同步控制三类型与基金行情 DTO（#407 随域归位）；
//! - [`state`]：全量同步中断状态（运行/取消标志，issue #104）；
//! - [`progress`]：进度事件推送（主线程非阻塞投递，issue #369）；
//! - `tests`：外挂测试（HTTP 层经本地 HTTP 服务独立测试，不依赖真实网络）。
//!
//! 依赖方向：本域消费基础设施（`db` / `error` / `events`），横向消费投资域
//! （价格写入单点 `prices` / 持仓谓词 / 基金代码判定）与核心交易域
//! （币种缺省推导 `transaction::amount`），不依赖壳层。壳层 `commands::sync`
//! 只做参数解包与信号发射，对外暴露 `sync_instruments` 全量同步（修标的字典）、
//! `sync_holding_prices` 增量同步（只刷价格）与 `cancel_sync_instruments`
//! 中断三个 IPC 命令（注册路径与前端/BDD 调用零改动）。

mod fund;
mod fund_nav;
mod http;
mod incremental;
mod model;
mod orchestrate;
mod persist;
mod progress;
mod state;

#[cfg(test)]
mod tests;

pub use fund::fetch_fund_detail_production;
pub use incremental::do_incremental_sync;
pub use model::{CancelSyncResult, FundDetail, FundNav, SyncHoldingPricesResult, SyncProgress};
pub use orchestrate::{GlobalConn, SyncOutcome, do_sync};
pub use state::SyncState;

/// 失败终态进度推送：壳层 `sync_instruments` 失败路径经此收敛到同一事件投递单点。
pub(crate) use progress::emit_error_progress;
