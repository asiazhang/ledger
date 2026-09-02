//! 交易领域模块（spec #52）：交易写入与金额口径的单一权威。
//!
//! 两个接缝：
//! - [`amount`]（口径权威）：kind 枚举真源 + kind→度量矩阵 + 本位币折算。
//! - [`writer`]（写入权威）：归一化 + 全列映射 + 审计字段生成（issue #55 落地）。
//!
//! 另有 [`search_text`]：统一模糊搜索语义的后端单一实现（ADR-0027，全库唯一定义
//! 点为核心域 TransactionSearch 词条）——拼音首字母、子序列判定与词条匹配纯函数，
//! 与数据库无关；交易搜索（壳层 `commands::search`）与投资域标的搜索共用。
//!
//! 依赖方向恒为「壳层 → transaction」：本模块不反向依赖壳层。

pub mod amount;
pub mod search_text;
pub mod writer;

#[cfg(test)]
mod tests;
