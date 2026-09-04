//! HTTP 端点处理函数，按资源域分文件（issue #429）：资源域划分与 `tests/api_server/`
//! 集成测试一致（测试侧再按场景细分）。壳层职责零变化：只做参数解包、事务壳
//!（连接层统一写入口，ADR-0032）、信号发射（`write_ops::emit_after_write`），业务语义零增量。
//!
//! 全部触 DB 端点 async 化（形状乙，spec #498 / #503）：DB 调用经连接层统一
//! helper [`crate::db::run_db`] 进 tauri 阻塞线程池执行，tokio worker 不再被
//! 毫秒级 DB 调用占用；helper 显式携带调用方 span 与 dispatcher 跨线程，SQL
//! 耗时归因沿 tower_http 请求 span 不漂移（集成测试契约
//! `test_http_sql_duration_attributed_to_request_span`）。`run_db` 的端点名用
//! 声明表 [`super::write_ops::HTTP_ENDPOINT_WRITE_OPS`] 同款身份格式
//!（`METHOD /path`）。不触 DB 的端点（基金查询实时网络往返、导入知识、OpenAPI
//! 文档）形态不变：基金详情拉取在连接锁外完成（分钟级阻塞网络往返不进锁，
//! 慢闭包纪律），先例 `fetch_fund_detail_for_api`。

pub mod accounts;
pub mod categories;
pub mod currencies;
pub mod funds;
pub mod import;
pub mod instruments;
pub mod merchants;
pub mod transactions;
