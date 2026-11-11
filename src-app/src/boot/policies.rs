//! 保单视图读写接缝（#2050）：保单统计行集与写路径（创建 / 修改 / 软删）
//! 直调 `ledger-policy` 域入口——与旧壳 policy 命令族消费同一域函数。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

// 写连接是成对建连的写侧，读连接是只读侧（ADR-0117）。
// ---------------------------------------------------------------------------

/// 保单视角统计行集（旧壳 `list_policy_stats` 命令消费的同一域函数，逐保单
/// 合计 + 下期扣款日 + 到期态实时推导不落库，issue #363）。多语句读闭包的
/// 同事务收口在域内 `ensure_transaction`（#1702 tx_scope 纪律），本壳只消费
/// 不另立口径。`today` 取本地日历日与旧壳命令层注入同口径。读连接是成对建
/// 连的只读侧。
pub fn policy_stats_rows(db: &DbState) -> Result<Vec<ledger_policy::PolicyStats>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_policy::policy_stats(&conn, chrono::Local::now().date_naive())
}

/// 创建保单（域公开写入口直调，旧壳 `create_policy` 命令同一执行协议）：校验
/// 与归一化（保单号 / 险种非空、期间可解析且止日不早于起日、保额币种成对）
/// 在域内，码化错误原样上抛（呈现面归视图层）。写连接是成对建连的写侧。
pub fn create_policy(db: &DbState, input: ledger_policy::PolicyInput) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_policy::create_policy(conn, input, &mut || {})
    })
}

/// 按 id 编辑保单静态要素（域公开写入口直调，旧壳 `update_policy` 命令同一
/// 执行协议）：全量替换；保司「保持历史引用」语义（携带保司与既有相同 = 跳过
/// 在用校验，软删保司的历史保单仍可编辑其他要素）在域内单点。写连接是成对
/// 建连的写侧。
pub fn update_policy(db: &DbState, id: &str, input: ledger_policy::PolicyInput) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_policy::update_policy(conn, id, input, &mut || {})
    })
}

/// 软删除保单（域公开写入口直调，旧壳 `delete_policy` 命令同一执行协议）：
/// `is_deleted=1` 不物理移除，库内行与历史引用保留不置空（ADR-0051 决策 5）。
/// 写连接是成对建连的写侧。
pub fn delete_policy(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_policy::delete_policy(conn, id, &mut || {})
    })
}
