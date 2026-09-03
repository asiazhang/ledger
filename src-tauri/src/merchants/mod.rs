//! 商户领域模块（Merchant，issue #188 / ADR-0028）。
//!
//! 商户字典的 CRUD 与按名查找/即建均在本域收口；IPC 参数解包、事务边界、
//! 命令注册和失效信号发射留在商户命令壳层。域不依赖壳层，
//! 交易行为层经本入口消费商户归一化能力。商户实体与入参集中本域
//! [`model`]（#419 随域归位），消费方经域路径逐类型显式 import。

mod crud;
mod model;

pub use crud::{
    create_merchant, create_merchant_by_name, delete_merchant, find_merchant_by_name,
    list_merchants, update_merchant,
};
pub use model::{Merchant, MerchantInput, MerchantUpdateInput};

#[cfg(test)]
mod tests;
