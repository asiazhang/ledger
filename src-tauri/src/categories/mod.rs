//! 分类领域模块（Category，#404 参考数据域归位）。
//!
//! 分类 CRUD、自然键幂等创建、两级分类校验、预算删除守卫（issue #355）与排序
//! 重排在本域收口；IPC 参数解包、事务边界、命令注册和失效信号发射留在
//! `commands::categories` 壳层。域不依赖壳层。分类实体与入参、排序项集中
//! 本域 [`model`]（#419 随域归位），消费方经域路径逐类型显式 import。

mod core;
mod model;

pub use core::{
    create_category, create_category_idempotent, delete_category, list_categories,
    reorder_categories, update_category,
};
pub use model::{Category, CategoryInput, CategoryUpdateInput, ReorderItem};

#[cfg(test)]
mod tests;
