//! 交易领域模块（spec #52）：交易写入与金额口径的单一权威。
//!
//! 两个接缝：
//! - [`amount`]（口径权威）：kind 枚举真源 + kind→度量矩阵 + 本位币折算。
//! - [`writer`]（写入权威）：归一化 + 全列映射 + 审计字段生成（issue #55 落地）。
//!
//! 依赖方向恒为「命令层 → transaction」：本模块不反向依赖命令层。

pub mod amount;
pub mod writer;

#[cfg(test)]
mod tests;
