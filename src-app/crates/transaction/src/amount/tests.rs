//! `amount` 模块测试索引（ADR-0113 决策 6：测试随生产模块外挂）。
//!
//! - [`amount`]：kind 边界、度量矩阵、SQL 片段聚合、本位币折算
//! - [`schema_cross_check`]：`transactions.kind` CHECK 字面量 ↔ `ALL` 互核（ADR-0108）

mod amount;
mod schema_cross_check;
