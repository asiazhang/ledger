//! HTTP 壳（外部 axum 服务）：与 IPC 壳（`commands/`）并列的外部壳，目录模块
//! （issue #429 / ADR-0056 壳层目录化）。壳层职责零变化：只做参数解包、事务壳
//!（ADR-0032/0033）、信号发射（ADR-0044），不含业务语义。
//!
//! 分主题模块（自单文件拆出，issue #429）：
//! - [`state`]：服务器状态（连接 + 发射槽 + 东财基金详情接缝）与 `FromRef` 提取器；
//! - [`error`]：统一错误响应（`AppError` → HTTP 状态/JSON）与错误 OpenAPI schema；
//! - [`openapi`]：OpenAPI 契约装配（`ApiDoc`）与契约自举端点；
//! - [`write_ops`]：「端点 → 写操作身份」声明表与单点映射发射；
//! - [`router`]：路由表与服务器启动；
//! - [`handlers`]：端点处理函数，按资源域分文件（资源域划分与集成测试一致）。
//!
//! 挂载点与引用面零变化（issue #429 验收项）：`lib.rs` 经 `crate::api_server::`
//! 消费 `start_http_server`；信号交叉核对测试消费 `ApiDoc` / `HTTP_ENDPOINT_WRITE_OPS`；
//! HTTP 集成测试消费 `build_router` / `ApiState` / `EmitterSlot` / `FundDetailFetcher`
//! ——均由本入口再导出承载，路径不变。

mod error;
mod handlers;
mod openapi;
mod router;
mod state;
mod write_ops;

// 壳层引用面单点：`crate::api_server::` 对外路径零变化（issue #429）。
pub use router::{build_router, start_http_server};
pub use state::{ApiState, EmitterSlot, FundDetailFetcher};
pub use write_ops::HTTP_ENDPOINT_WRITE_OPS;
// 交叉核对测试（signals_cross_check，#[cfg(test)]）在 crate 内消费契约装配本体；
// 非测试构建本再导出无消费方，allow 压制单边 unused 告警（引用面保持 issue #429 原状）。
#[allow(unused_imports)]
pub(crate) use openapi::ApiDoc;
