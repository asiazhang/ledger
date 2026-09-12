//! 交易×账户接缝（跨域接缝区，issue #1090）：受影响账户余额重算的注册点。
//!
//! 职责：本域只承诺调用时机（创建/修改/软删落库后的同事务刷新点），实现由账户域
//! 提供（受影响账户推导 + 整体重算，ADR-0067），壳层启动接线。不变量：刷新与引发
//! 它的写入同事务，未注册即码化错误（失败整体回滚，不产生「源已写、缓存未刷」）；
//! 本域只交出账户引用三元组，推导与重算在实现侧。ADR 指针：ADR-0112 决策 5 /
//! ADR-0067 / ADR-0071 决策 5 修订注记。陷阱：实现经
//! `accounts::balance::install_balance_refresh_hook` 装入，本域对账户域零直接依赖。

use std::sync::OnceLock;

use rusqlite::Connection;

use ledger_infra::error::{AppError, Result};

/// 余额刷新钩子签名：连接 + 新旧两行的账户引用三元组
/// `(account_id, to_account_id, funding_account_id)`（transactions 表上的三列闭集，
/// 本域自有数据；创建 `old=None`、删除 `new=None`）。
///
/// 「算哪些」（受影响账户推导）与「怎么算」（口径表达式整体重算）的语义都属账户域
/// 实现，本域只交出引用三元组——推导唯一性（issue #533 / ADR-0096 决策 5）不因接缝
/// 反转而分散。
pub type BalanceRefreshHook = fn(
    &Connection,
    Option<(&str, Option<&str>, Option<&str>)>,
    Option<(&str, Option<&str>, Option<&str>)>,
) -> Result<()>;

/// 余额刷新钩子的进程级单例（登记点反转的承接面）：先装者优先、重复注册零动作。
static BALANCE_REFRESH_HOOK: OnceLock<BalanceRefreshHook> = OnceLock::new();

/// 注册余额刷新实现（幂等：进程级一次，重复注册保留首次实现）。
///
/// 调用点在壳层启动接线与测试建库单点（`test_support::open`、BDD world），
/// 与生产同形；实现由账户域提供（`accounts::balance::install_balance_refresh_hook`），
/// 业务代码不直接调用本函数。
pub fn register_balance_refresh_hook(hook: BalanceRefreshHook) {
    let _ = BALANCE_REFRESH_HOOK.set(hook);
}

/// 未注册错误的单一构造（纯函数，可测）：码化 Invalid——接线缺失是程序缺陷，
/// 不该被业务路径静默吞掉。
fn balance_refresh_hook_missing_error() -> AppError {
    AppError::coded(
        "transaction.balance-refresh-hook-unregistered",
        "余额刷新钩子未注册：交易写入的余额缓存重算被跳过将产生缓存漂移（壳层启动接线缺失）",
    )
}

/// 委派内核（纯函数，可测）：钩子在场即调用，缺席即码化错误。静态槽的薄封装
/// 拆出 Option 入参，使「未注册」分支不依赖进程级全局状态即可测（OnceLock 注册
/// 幂等且不可回退，全局形态无法在测试里反复置空）。
pub(crate) fn dispatch_balance_refresh(
    hook: Option<BalanceRefreshHook>,
    conn: &Connection,
    old: Option<(&str, Option<&str>, Option<&str>)>,
    new: Option<(&str, Option<&str>, Option<&str>)>,
) -> Result<()> {
    match hook {
        Some(hook) => hook(conn, old, new),
        None => Err(balance_refresh_hook_missing_error()),
    }
}

/// 交易写路径的余额缓存重算入口（创建/修改/软删三条路径共用，ADR-0067 写路径
/// 同事务整体重算，禁止增量加减）：委派给注册的实现。
///
/// 必须在调用方既有写事务内调用（与引发重算的写入同事务，ADR-0067）；未注册即
/// 接线缺失，码化错误上抛（写入随事务回滚，缓存不漂移）。
pub fn refresh_affected_balances(
    conn: &Connection,
    old: Option<(&str, Option<&str>, Option<&str>)>,
    new: Option<(&str, Option<&str>, Option<&str>)>,
) -> Result<()> {
    dispatch_balance_refresh(BALANCE_REFRESH_HOOK.get().copied(), conn, old, new)
}

#[cfg(test)]
mod tests;
