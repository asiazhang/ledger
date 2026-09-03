//! 数据模型（按领域拆分为多个文件，本模块为统一入口）。
//!
//! 全项目以 `crate::models::*` 引用模型；serde 结构、utoipa 契约与
//! 拆分前（单一 `models.rs`）完全一致，外部引用零改动。

mod accounts;
mod budget;
mod categories;
mod dashboard;
mod financial_freedom;
mod investment;
mod item;
mod policy;
mod reports;
mod transactions;

pub use accounts::*;
pub use budget::*;
pub use categories::*;
pub use dashboard::*;
pub use financial_freedom::*;
pub use investment::*;
pub use item::*;
pub use policy::*;
pub use reports::*;
pub use transactions::*;
