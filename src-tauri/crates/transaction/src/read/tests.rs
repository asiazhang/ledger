//! `read` 模块测试索引（ADR-0113 决策 6：测试随生产模块外挂）。
//!
//! - [`query`]：交易查询、排序与分页
//! - [`search`]：统一模糊搜索语义与搜索行为
//! - [`search_repair`]：拼音辅助数据一键修复（积压回填、幂等、收敛）

mod query;
mod search;
mod search_repair;
