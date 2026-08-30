//! HTTP 集成测试入口：直连 axum Router 断言 HTTP 契约（`#[tokio::test]`）。
//!
//! 原单体 `api_server.rs`（约 2500 行）按资源域目录化拆分（纯搬迁，断言/逻辑/注释未改）：
//! 各模块与资源域一一对应，公共设施（应用装配、参考数据创建辅助、批量请求、断言辅助）
//! 集中在 `common`；散落在各资源域的 OpenAPI 文档契约断言集中到 `documentation`。

mod account_update;
mod balance;
mod batch_import;
mod common;
mod documentation;
mod instrument_create;
mod instrument_search;
mod investment_migration;
mod merchant_import;
mod reference_data;
mod transaction_crud;
mod transaction_list;
