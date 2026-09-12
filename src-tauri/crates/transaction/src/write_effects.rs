//! 交易写路径副作用接缝（issue #1090 / spec #1086 形态推广）：受影响账户余额重算的
//! 注册点与委派单点。
//!
//! 接缝形态与基础设施提交点后置动作（`db::runtime`，#1088/#1089）同构——「下层定义
//! 注册点、上层注册实现、壳层启动时接线」：本模块（核心交易域）只承诺调用时机
//! （创建/修改/软删落库后的同事务刷新点），不知道实现语义；实现由账户域余额模块
//! 提供（受影响账户推导 + 整体重算，ADR-0067），壳层启动接线
//! （`accounts::balance::install_balance_refresh_hook`）。核心交易域对账户域的直接
//! 依赖边随之消除（ADR-0071 决策 5 修订注记：transaction ⇄ accounts 双向横向边收敛
//! 为 accounts → transaction 单向；`transaction → accounts` 直接引用禁令化，见
//! `scripts/check-structure.ts` 域间禁边）。
//!
//! 未注册即接线缺失：码化错误上抛——刷新与引发它的写入同事务，失败整体回滚，
//! 不产生「源已写、缓存未刷」的静默漂移（ADR-0067 写路径同事务整体重算约束）。

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
mod tests {
    use super::*;

    #[test]
    fn 未注册即码化错误_错误码可判读() {
        // 建库经统一测试工厂（ADR-0084 规则 1）；委派入参显式传 None，
        // 与工厂顺带注册的全局钩子无涉（不读槽）。
        let conn = tauri_app_lib::test_support::open();
        let err = dispatch_balance_refresh(None, &conn, None, None).expect_err("未注册应报错");
        assert_eq!(
            err.code(),
            Some("transaction.balance-refresh-hook-unregistered")
        );
    }

    #[test]
    fn 钩子在场即委派_新旧三元组原样透传() {
        // 断言委派实参按位透传（推导语义属账户域，本域只透传三元组）。
        fn stub(
            _conn: &Connection,
            old: Option<(&str, Option<&str>, Option<&str>)>,
            new: Option<(&str, Option<&str>, Option<&str>)>,
        ) -> Result<()> {
            assert_eq!(old, None, "创建形态 old=None 原样透传");
            assert_eq!(new, Some(("acc", Some("to"), None)));
            Ok(())
        }
        let conn = tauri_app_lib::test_support::open();
        dispatch_balance_refresh(Some(stub), &conn, None, Some(("acc", Some("to"), None)))
            .expect("钩子在场应成功");
    }
}
