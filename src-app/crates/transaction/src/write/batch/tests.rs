//! 批量写入模块测试索引（ADR-0113 决策 6：测试随生产模块外挂）。
//!
//! - [`batch_create`]：批量写入、幂等键语义与批次汇总日志
//! - [`batch_dedup`]：内容哈希与去重身份判定

mod batch_create;
mod batch_dedup;
