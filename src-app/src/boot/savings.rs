//! 储蓄目标视图读写接缝（#2056）：目标清单写路径（创建 / 修改 / 归档 /
//! 恢复 / 删除）直调 `ledger-savings-goal` 域入口——进度行集读接缝在
//! 账户接缝（savings_goal_progress_rows），本文件只持写面。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use crate::shell_support::write_entry_locked;

// 专属账户随域同一事务。写连接是成对建连的写侧。
// ---------------------------------------------------------------------------

/// 创建储蓄目标（域公开写入口直调，旧壳 create_savings_goal 命令同一执行
/// 协议）：目标金额正数守卫在域内码化拒绝；同一事务经账户域公开写入口自动
/// 建专属账户（1:1 绑定）。返回目标 id。写连接是成对建连的写侧。
pub fn create_savings_goal(
    db: &DbState,
    input: &ledger_savings_goal::SavingsGoalInput,
) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::create_savings_goal(conn, input)
    })
}

/// 编辑储蓄目标（四字段全量替换，域公开写入口直调，旧壳 update_savings_goal
/// 命令同一执行协议）：名称变化联动专属账户名（目标名权威），非法输入
///（目标额非正 / 空名 / 计划月存非正）由域守卫码化拒绝零落库。写连接是成对
/// 建连的写侧。
pub fn update_savings_goal(
    db: &DbState,
    id: &str,
    input: &ledger_savings_goal::SavingsGoalUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::update_savings_goal(conn, id, input)
    })
}

/// 归档储蓄目标（域入口直调，旧壳 archive_savings_goal 命令同一执行协议）：
/// 退出默认列表，账户 / 流水 / 关联计划原样保留。写连接是成对建连的写侧。
pub fn archive_savings_goal(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::archive_savings_goal(conn, id)
    })
}

/// 取消归档储蓄目标（域入口直调，旧壳 unarchive_savings_goal 命令同一执行
/// 协议）：恢复 active 回默认列表。写连接是成对建连的写侧。
pub fn unarchive_savings_goal(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::unarchive_savings_goal(conn, id)
    })
}

/// 删除储蓄目标（域公开写入口直调，旧壳 delete_savings_goal 命令同一执行
/// 协议）：余额非零被域守卫码化拒绝（引导先转出），余额为零时域内同一事务
/// 目标软删 + 级联软删专属账户（历史流水保留可查）。写连接是成对建连的写侧。
pub fn delete_savings_goal(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::delete_savings_goal(conn, id)
    })
}

// 保单视图读写接缝（#2050）：保单清单 / 保单视角统计 / 保司字典（含软删显示
// 映射面）直调 `ledger-policy` 域入口——与旧壳 list_policies / list_policy_stats
// / create / update / delete_policy 与保司（resolve-insurer 的 create_insurer
// 消费面）命令消费的同一域函数，不经 IPC。保单写路径的校验（保司在用、保单号
// / 险种非空、期间、保额币种成对）与失效信号回调注入全在域内，本壳不自建语义。
