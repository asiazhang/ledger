//! 数据模型（按领域拆分为多个文件，本模块为统一入口）。
//!
//! 全项目以 `crate::models::*` 引用模型；serde 结构、utoipa 契约与
//! 拆分前（单一 `models.rs`）完全一致，外部引用零改动。

mod investment;
mod transactions;

pub use investment::*;
pub use transactions::*;
