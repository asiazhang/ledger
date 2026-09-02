//! 核心交易域测试索引（#403 域目录化随迁收口）。
//!
//! - [`amount`]：kind 边界、度量矩阵、SQL 片段聚合、本位币折算
//! - [`audit`]：审计字段统一生成与 native 本位币折算
//! - [`behavior`]：写删行为、refund 链、嵌套感知事务与即建商户证据
//! - [`common`]：交易行为/查询脚手架
//! - [`merchant`]：商户携带收口与即建商户证据
//! - [`query`]：交易查询、排序与分页
//! - [`search`]：统一模糊搜索语义与搜索行为
//! - [`batch_common`]：批量写入共享脚手架
//! - [`batch_create`]：批量写入、幂等键语义与批次汇总日志
//! - [`batch_dedup`]：内容哈希与去重身份判定

mod amount;
mod audit;
mod batch_common;
mod batch_create;
mod batch_dedup;
mod behavior;
mod common;
mod merchant;
mod query;
mod search;
