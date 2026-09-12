//! 交易×商户接缝（spec #1086 / issue #1092）：商户名归一化（AI 导入契约）的注册
//! 点与委派单点。
//!
//! 形态与 [`super::write_effects`] 同族——「下层定义注册点、上层注册实现、壳层
//! 启动时接线」（ADR-0112 决策 5）：行为层计划装配的「先查名、校验通过后即建」
//! 两段式协议留在本域（失败行不产生碎商户的时序是写入协议语义），对商户表的
//! 查询与建行归商户域实现（`merchants::install_merchant_hooks`），壳层启动接线。
//! 两支钩子一个槽位原子注册，避免「只装查、缺建」的半套接线。
//!
//! 未注册即接线缺失：码化错误上抛（携带商户名的写入显式失败，不静默丢即建）。

use std::sync::OnceLock;

use rusqlite::Connection;

use crate::error::{AppError, Result};

/// 商户名钩子组：按名精确查找（命中复用）与按名即建（find-or-create 语义，
/// trim 归一归实现侧，AI 提交字符串即可）。
pub struct MerchantNameHooks {
    /// 精确匹配在用商户名：命中返回 id，未命中返回 `None`。
    pub find_by_name: fn(&Connection, &str) -> Result<Option<String>>,
    /// 按名即建商户：名字命中在用商户时直接复用，返回商户 id。
    pub create_by_name: fn(&Connection, &str) -> Result<String>,
}

static MERCHANT_HOOKS: OnceLock<MerchantNameHooks> = OnceLock::new();

/// 注册商户名钩子组（幂等：进程级一次，重复注册保留首次）。调用点在商户域
/// `install_merchant_hooks`，壳层启动接线，业务代码不直接调用。
pub fn register_merchant_hooks(hooks: MerchantNameHooks) {
    let _ = MERCHANT_HOOKS.set(hooks);
}

/// 未注册错误的单一构造（纯函数，可测）：码化 Invalid——接线缺失是程序缺陷。
fn merchant_hooks_missing_error() -> AppError {
    AppError::coded(
        "transaction.merchant-hooks-unregistered",
        "商户名钩子未注册：商户名解析与即建被跳过（壳层启动接线缺失）",
    )
}

/// 商户名查找委派（计划装配「先查」段）：未注册即码化错误。
pub(crate) fn find_merchant_by_name(conn: &Connection, name: &str) -> Result<Option<String>> {
    let hooks = MERCHANT_HOOKS
        .get()
        .ok_or_else(merchant_hooks_missing_error)?;
    (hooks.find_by_name)(conn, name)
}

/// 商户即建委派（计划装配「后建」段，行内校验全部通过后调用）：未注册即码化错误。
pub(crate) fn create_merchant_by_name(conn: &Connection, name: &str) -> Result<String> {
    let hooks = MERCHANT_HOOKS
        .get()
        .ok_or_else(merchant_hooks_missing_error)?;
    (hooks.create_by_name)(conn, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 钩子未注册即码化错误_错误码可判读() {
        let err = merchant_hooks_missing_error();
        assert_eq!(err.code(), Some("transaction.merchant-hooks-unregistered"));
    }
}
