//! 交易同步命令区（共享语义，issue #855 / ADR-0091；ADR-0113 决策 2/8）。
//!
//! 唯一地图：`payload`（`TransactionCommand` 信封载荷与协议面实现）/ `fields`
//! （转换 / 份额调整 / 投资三份语义字段契约）。不变量：载荷契约被接缝与写路径共同
//! 消费，**只增不改**，旧日志可在新 schema 重放。ADR 指针：ADR-0091 / ADR-0099
//! 决策 6 / ADR-0106 决策 9。陷阱：op 产出单点住写路径 `crate::write::op`。
//!
//! 本文件只做声明与逐项再导出（ADR-0113 决策 5），不含逻辑。

mod fields;
mod payload;

pub use fields::{ConvertCommandFields, InvestmentCommandFields, SplitCommandFields};
pub use payload::TransactionCommand;
