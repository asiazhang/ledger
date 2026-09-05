//! HTTP 集成测试入口：直连 axum Router 断言 HTTP 契约（`#[tokio::test]`）。
//!
//! 原单体 `api_server.rs`（约 2500 行）按资源域目录化拆分（纯搬迁，断言/逻辑/注释未改）：
//! 各模块与资源域一一对应，公共设施（应用装配、参考数据创建辅助、批量请求、断言辅助）
//! 集中在 `common`；散落在各资源域的 OpenAPI 文档契约断言集中到 `documentation`。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

mod account_update;
mod balance;
mod batch_import;
mod boot_gate;
mod common;
mod documentation;
mod encryption_gate;
mod error_codes;
mod fund_lookup;
mod instrument_create;
mod instrument_create_fund;
mod instrument_search;
mod investment_migration;
mod merchant_import;
mod reference_data;
mod signal_delivery;
mod transaction_crud;
mod transaction_list;
