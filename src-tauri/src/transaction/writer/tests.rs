//! `transaction::writer` 接缝的单元测试（issue #55 / spec #52）。
//!
//! 断言模块外部行为：normalize 的校验/退款继承/本位币折算、insert_row 的全列映射
//! 与审计字段生成、update_row 的字段覆盖与幂等身份保留。全部基于内存库。
//!
//! #261 按行为主题拆为子模块（纯移动）：
//! - [`normalize`]：归一化校验——通用 kind 直通、金额、transfer 必填目标账户、
//!   仅接受通用 kind、本位币折算；
//! - [`refund`]：refund 链——来源关联与字段继承、来源合法性拒绝；
//! - [`merchant`]：商户相关写入语义——透传/校验/退款继承与修改路径豁免；
//! - [`rows`]：行级写入语义——insert_row / update_row、端到端与置脏触发。
//!
//! 共享脚手架收在 [`common`]（仅限本测试目录内部）。

mod common;
mod merchant;
mod normalize;
mod refund;
mod rows;
