//! HTTP 端点处理函数，按资源域分文件（issue #429）：资源域划分与 `tests/api_server/`
//! 集成测试一致（测试侧再按场景细分）。壳层职责零变化：只做参数解包、事务壳
//!（连接层统一写入口，ADR-0032）、信号发射（`write_ops::emit_after_write`），业务语义零增量。

pub mod accounts;
pub mod categories;
pub mod currencies;
pub mod funds;
pub mod import;
pub mod instruments;
pub mod merchants;
pub mod transactions;
