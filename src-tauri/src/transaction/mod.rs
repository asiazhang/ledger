//! 交易领域模块（spec #52）：交易写入与金额口径的单一权威。
//!
//! 两个接缝：
//! - [`amount`]（口径权威）：kind 枚举真源 + kind→度量矩阵 + 本位币折算。
//!   （Writer 写入权威接缝由后续 issue 落地。）
//!
//! 依赖方向恒为「命令层 → transaction」：本模块不反向依赖命令层。

pub mod amount;

#[cfg(test)]
mod tests;
