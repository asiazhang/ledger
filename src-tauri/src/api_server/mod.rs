//! HTTP 壳（外部 axum 服务）：与 IPC 壳（`commands/`）并列的外部壳，目录模块
//! （issue #429 / ADR-0056 壳层目录化）。壳层职责零变化：只做参数解包与统一
//! 写入口/读 helper 一行调用（事务、置脏、信号内化单点，ADR-0073），不含业务语义。
//!
//! 分主题模块（自单文件拆出，issue #429）：
//! - [`state`]：服务器状态（连接 + 发射槽 + 东财基金详情接缝）与 `FromRef` 提取器；
//! - [`error`]：统一错误响应（`AppError` → HTTP 状态/JSON）与错误 OpenAPI schema；
//! - [`openapi`]：OpenAPI 契约装配（`ApiDoc`）与契约自举端点；
//! - [`router`]：路由表与服务器启动；
//! - [`handlers`]：端点处理函数，按资源域分文件（资源域划分与集成测试一致）。
//!
//! 挂载点与引用面零变化（issue #429 验收项）：`lib.rs` 经 `crate::api_server::`
//! 消费 `start_http_server`；信号守门测试消费 `ApiDoc`（端点集真源）；
//! HTTP 集成测试消费 `build_router` / `ApiState` / `EmitterSlot` / `FundDetailFetcher`
//! / `StockQuoteFetcher`
//! ——均由本入口再导出承载，路径不变。
//! 「端点 → 写操作身份」手写声明表（`write_ops.rs`，ADR-0044 #335）已随
//! ADR-0073（spec #523）消亡为源码扫描派生物：身份内化进统一写入口调用点，
//! 接线由扫描守门核对（`signals_cross_check`）。

mod error;
mod handlers;
mod openapi;
mod router;
mod state;

// 壳层引用面单点：`crate::api_server::` 对外路径零变化（issue #429）。
pub use router::{build_router, start_http_server};
pub use state::{ApiState, EmitterSlot, FundDetailFetcher, StockQuoteFetcher};
// 信号守门测试（signals_cross_check，#[cfg(test)]）在 crate 内消费契约装配本体；
// 非测试构建本再导出无消费方，allow 压制单边 unused 告警（引用面保持 issue #429 原状）。
#[allow(unused_imports)]
pub(crate) use openapi::ApiDoc;
