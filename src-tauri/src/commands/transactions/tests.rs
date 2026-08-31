//! 交易命令域单元测试索引：按行为主题拆分子模块（issue #255，纯移动重组）。
//!
//! - [`behavior`] — 写删行为、refund 链、行为层编排入口
//! - [`query`] — 交易查询
//! - [`merchant`] — 商户携带收口（ADR-0028）
//! - [`audit`] — 审计字段与 native 折算
//!
//! 共享脚手架收在 [`common`]（仅限本测试目录内部）。

mod audit;
mod behavior;
mod common;
mod merchant;
mod query;
