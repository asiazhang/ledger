//! 交易读取区（读路径，ADR-0113 决策 2/8）：读取与检索的单点。
//!
//! 唯一地图：`list`（列表过滤/排序/分页与单笔读取）/ `source`（来源列与转换两腿的
//! 读时投影消费逻辑）/ `search`（SQL 下推搜索与拼音修复）。不变量：依赖方向唯一
//! ——读路径 → 跨域接缝 → 共享语义；读路径不依赖写路径。ADR 指针：ADR-0113 决策 8 /
//! ADR-0027 / ADR-0056。陷阱：接缝注册点住跨域接缝区（`crate::seams`），本区只消费。
//!
//! 本文件只做声明与逐项再导出（ADR-0113 决策 5），不含逻辑。

mod list;
pub mod search;
mod source;

pub use list::{
    get_transaction, get_transaction_internal, list_transactions, list_transactions_internal,
};

#[cfg(test)]
mod tests;
