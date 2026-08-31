//! 数据模型（按领域拆分为多个文件，本模块为统一入口）。
//!
//! 全项目以 `crate::models::*` 引用模型；serde 结构、utoipa 契约与
//! 拆分前（单一 `models.rs`）完全一致，外部引用零改动。

mod accounts;
mod budget;
mod categories;
mod currencies;
mod dashboard;
mod financial_freedom;
mod fx;
mod investment;
mod item;
mod merchants;
mod reports;
mod sync;
mod transactions;

pub use accounts::*;
pub use budget::*;
pub use categories::*;
pub use currencies::*;
pub use dashboard::*;
pub use financial_freedom::*;
pub use fx::*;
pub use investment::*;
pub use item::*;
pub use merchants::*;
pub use reports::*;
pub use sync::*;
pub use transactions::*;
