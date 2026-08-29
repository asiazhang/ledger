//! `scheduled_transactions` 单元测试索引：按行为主题拆分子模块（issue #258，纯移动重组）。
//!
//! - [`occurrence`] — 执行与币种折算（issue #59 / spec #52 折算回归 + expense/transfer 映射 + 状态流转）
//! - [`spend`] — 订阅花费双口径（ADR-0023，issue #160 / #161）
//! - [`plan_edit`] — 订阅编辑校验（issue #162，ADR-0023 决策三）
//! - [`merchant`] — 商户复制与软删引用（issue #190 / ADR-0028）
//!
//! 共享脚手架收在 [`common`]（仅限本测试目录内部）。
//! 全部基于内存库，走 `engine` 公开 API（create_plan / execute_occurrence 等）。

mod common;
mod merchant;
mod occurrence;
mod plan_edit;
mod spend;
