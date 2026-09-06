//! 保单（Policy）领域模块（issue #360 / spec #358 / ADR-0051）。
//!
//! 职责：保单静态档案的创建、编辑、软删除、列表与保单视角统计（实时推导
//! 不落库）——写入口的校验与归一化、口径接线与失效信号回调注入。
//!
//! 接缝：
//! - 域 API（单一权威）：写路径（创建/编辑/删除）与读路径
//!   （列表/保单视角统计），域语言短名；失效信号以 `notify` 回调注入
//!   （回调注入式，仿行情同步域 `sync` 的 emit 注入先例）。
//!
//! 依赖方向恒为「壳层 → policy → 基础设施」：本模块不反向依赖壳层；
//! 对 `transaction::amount` 的消费属域间横向依赖（ADR-0056 决策 2 允许）。
//! 保单实体、入参与统计行集中本域 [`model`]（#420 随域归位），
//! 消费方经域路径逐类型显式 import。
//! 保司字典（Insurer，issue #712 / ADR-0082）归保险域自有：单消费方 Policy，
//! 不进参考数据域与核心交易域；模型与 CRUD 收口 [`insurer`]（体量小不拆）。

pub mod crud;
pub mod insurer;
mod model;
pub mod stats;
pub mod validation;

/// 域 API 再导出：调用面用域语言短名（`policy::list_policies` 等），
/// 与 ADR-0056 定格形状一致（先例：`item` / `scheduled_transactions` 入口再导出）。
pub use crud::{create_policy, delete_policy, list_policies, source_display_by_ids, update_policy};
pub use insurer::{
    Insurer, InsurerInput, InsurerUpdateInput, create_insurer, create_insurer_by_name,
    delete_insurer, find_insurer_by_name, list_insurers, update_insurer,
};
pub use model::{Policy, PolicyInput, PolicySourceDisplay, PolicyStats};
pub use stats::policy_stats;

#[cfg(test)]
mod tests;
