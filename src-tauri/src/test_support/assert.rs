//! 共享断言库：「缓存回填 == 实时计算」不变式对拍（唯一维护点，ADR-0067 /
//! ADR-0084 决策 6）与通用白盒行读取器。
//!
//! 「回填 == 实时」是余额缓存的关键不变式：任何写入入口落库后，`account_balance_cache`
//! 必须与实时计算（`compute_balance` 单一口径）逐账户一致，违约读路径报码化错误引导
//! 审计、不静默回退（ADR-0067）。本断言自 transaction/db 两域各维护一份的旧状态收归
//! 此处（transaction/tests/balance_cache.rs 现版为体），两域旧断言已删除归一。

use rusqlite::{Connection, OptionalExtension, params};

use crate::accounts::balance::{compute_balance, list_accounts_with_visibility};

/// 通用白盒行读取器：执行只读 SQL 取单个整数列，行缺失返回 `None`（既有白盒读
/// 约定是有意为之的旁路，本读取器只共享读取体、不推翻约定，ADR-0084 决策 6）。
/// SQL 由调用方书写（显式、可 grep），`query_row` + `optional` 的样板在此归一。
pub fn read_scalar_i64(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> Option<i64> {
    conn.query_row(sql, params, |r| r.get(0))
        .optional()
        .unwrap()
}

/// 一致性对拍断言：全部账户（含黑洞等隐藏账户）的缓存行 == 实时计算（逐账户比对）。
/// 断言失败即余额缓存口径漂移，消息携带账户 id 与名称定位差异。
///
/// 覆盖面：`list_accounts_with_visibility(conn, true)` 枚举全部未删除账户；缓存行
/// 缺失（`None`）与值漂移同样红。软删账户不在枚举内（读路径不读它，迁移一次性
/// 回填行留存无害，见 db 域迁移回填测试）。经种子直插的账户无写路径钩子、缓存行
/// 不会自动建立，断言前须先经整体重算接缝回填（先例：transaction 域
/// `backfill_scaffold_account`，`refresh_account_balances`）。
pub fn assert_balance_cache_matches_realtime(conn: &Connection) {
    for account in list_accounts_with_visibility(conn, true).unwrap() {
        let cached = read_scalar_i64(
            conn,
            "SELECT balance_cents FROM account_balance_cache WHERE account_id=?1",
            params![account.id],
        );
        assert_eq!(
            cached,
            Some(compute_balance(conn, &account.id).unwrap()),
            "账户 {}({}) 缓存应等于实时计算",
            account.id,
            account.name
        );
    }
}
