//! 账户领域模块（Account，#404 参考数据域归位）。
//!
//! 账户 CRUD、自然键幂等创建、币种锁定守卫、黑洞账户即建与余额调整交易编排
//! 在本域收口；IPC 参数解包、事务边界、命令注册和失效信号发射留在
//! `commands::accounts` 壳层。域不依赖壳层；净资产/财务自由度聚合经本入口
//! 复用余额口径（issue #142），余额调整经核心交易域创建编排入口落库（issue #310）。

mod core;

pub use core::{
    adjust_account_balance, create_account, create_account_idempotent, delete_account,
    ensure_black_hole_account, get_account, list_account_balances_for_api,
    list_account_balances_with_visibility, list_accounts, list_accounts_for_api, update_account,
};

#[cfg(test)]
mod tests;
