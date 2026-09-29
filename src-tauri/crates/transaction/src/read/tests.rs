//! `read` 模块测试索引（ADR-0113 决策 6：测试随生产模块外挂）。
//!
//! - [`query`]：交易查询、排序与分页
//! - [`search`]：统一模糊搜索语义与搜索行为
//! - [`order`]：订单汇总读回与读闭包（issue #1862 / ADR-0138 决策 9）
//! - [`purchase`]：购买项读回与读闭包（issue #1882 / ADR-0138 决策 9/10）
//! - [`snapshot`]：读快照一致性探针——total 与 items 同快照（issue #1699）
//! - [`row_columns`]：全列 SELECT 清单 ↔ FromRow 位置下标映射钉住（issue #1880）

mod funding;
mod order;
mod purchase;
mod query;
mod row_columns;
mod search;
mod snapshot;
