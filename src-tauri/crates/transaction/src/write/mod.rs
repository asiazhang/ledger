//! 写路径区（ADR-0113 决策 2）：本域的写入编排与落库。
//!
//! 写入协议（Local / Replay 两形态同址，ADR-0105）、行写入、批量、出资准入与
//! op 产出。依赖方向唯一——写路径 → 跨域接缝 → 共享语义；写路径与读路径互不
//! 依赖，只凭「接缝 + 共享语义」即可编译。

pub mod batch;
pub mod funding;
mod op;
pub mod protocol;
pub mod writer;

#[cfg(test)]
mod tests;
