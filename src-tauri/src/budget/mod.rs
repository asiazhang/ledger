//! 预算领域模块（ADR-0029 / ADR-0052）。
//!
//! 预算 CRUD 与当前周期进度在域内按主题组织；IPC 参数解包、事务边界与命令注册
//! 留在 `commands::budget.rs` 壳层。域不依赖壳层，金额口径继续复用核心交易域的
//! `ExpenseNet` 度量矩阵。预算实体、入参与进度集中本域 [`model`]（#420 随域归位），
//! 消费方经域路径逐类型显式 import。

pub mod crud;
mod model;
pub mod progress;

pub use crud::{create_budget, delete_budget, list_budgets, update_budget};
pub use model::{Budget, BudgetInput, BudgetPeriod, BudgetProgress, BudgetUpdateInput};
pub use progress::budget_progress_rows;

#[cfg(test)]
mod tests;
