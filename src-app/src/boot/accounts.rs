//! 账户视图读写接缝（#2041）：余额清单三分区读、储蓄目标进度、计划行集
//! 与写路径（创建 / 修改 / 删除 / 归档 / 恢复 / 余额调整）直调
//! `ledger-accounts` 域入口——写经壳层统一写入口（提交点置脏）。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

// ---------------------------------------------------------------------------
// 账户视图读写接缝（#2041）：账户余额清单（V017 持久化缓存口径）与目标绑定
// 集 / 计划引用集直调域入口——与旧壳 list_account_balances、
// savingsGoalProgress、listScheduledTransactions 命令消费的同一域函数，不经
// IPC。账户写路径（创建 / 编辑 / 删除 / 归档 / 恢复 / 余额调整）直调域公开
// 写入口——与旧壳账户族命令同一执行协议；写连接是成对建连的写侧。
// ---------------------------------------------------------------------------

/// 账户余额行集（旧壳 `list_account_balances` 命令同一执行协议）：**不含黑洞
/// 账户**（include_hidden = false，用户侧查询面板径）。余额读 V017 持久化
/// 缓存（ADR-0067）：行读数来自域余额缓存行（写路径在写事务内对受影响账户
/// 整体重算回写），本壳只消费不另立算术；缓存行缺失报码化错误
/// （`balance.cache-row-missing`），不静默回退。读连接是成对建连的只读侧
/// （ADR-0117）。
pub fn account_balance_rows(db: &DbState) -> Result<Vec<ledger_accounts::AccountBalance>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_accounts::list_account_balances_with_visibility(&conn, false)
}

/// 储蓄目标进度行集（旧壳 savingsGoalProgress 命令消费的同一域函数）：账户页
/// 「储蓄目标」分组与「名称随动只读」判定的绑定集消费面——未删除目标全集
/// （含已归档目标，旧壳 store goalAccountIds 同口径）。读连接是成对建连的
/// 只读侧；读失败由视图呈现加载错误（不用绑集判定照常回退为「无绑定」会静默
/// 改分组，不做降级）。
pub fn savings_goal_progress_rows(
    db: &DbState,
) -> Result<Vec<ledger_savings_goal::SavingsGoalProgress>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_savings_goal::list_savings_goal_progress(&conn, chrono::Local::now().date_naive())
}

/// 定时计划行集（旧壳 `listScheduledTransactions` 命令消费的同一域函数，软删
///滤除在域内）：归档确认框「未结束计划数」提示的引用集消费面（提示是佐证
/// 信息，读失败按 0 处理的丢弃语义归视图侧，与旧壳 plan-usage 消费同口径）。
/// 读连接是成对建连的只读侧。
pub fn plan_rows(db: &DbState) -> Result<Vec<ledger_scheduled::ScheduledTransactionWithExt>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_scheduled::list_plans(&conn)
}

/// 创建账户（域公开写入口直调，旧壳 `create_account` 命令同一执行协议）：
/// 编排入口 [`ledger_accounts::create_account`]——落库 + 缓存行 + op 产出在域
/// 内单正文（自持事务，中途失败整体回滚）。写连接是成对建连的写侧。
pub fn create_account(db: &DbState, input: ledger_accounts::AccountInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_accounts::create_account(conn, input))
}

/// 按 id 全字段替换账户编辑面（域公开写入口直调，旧壳 `update_account` 命令
/// 同一执行协议）：归档可选键携带 `true` 时先经目标域守卫（旧壳命令壳同位，
/// 见 [delete_account] 同形理由）。不存在的 id 报码化 NotFound。写连接是成对
/// 建连的写侧。
pub fn update_account(
    db: &DbState,
    id: &str,
    input: ledger_accounts::AccountUpdateInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        if input.is_archived == Some(true) {
            ledger_savings_goal::ensure_account_can_archive(conn, id)?;
        }
        ledger_accounts::update_account(conn, id, input)
    })
}

/// 删除账户（软删；域公开写入口直调，旧壳 `delete_account` 命令同一执行协议）：
/// 目标域守卫在入口先行（「删账户命令先经目标域校验」，旧壳命令壳同位——账户
/// 域不反向依赖目标域，ADR-0133 决策 4）：在用目标的专属账户被码化拒绝，普通
/// 账户照常放行。写连接是成对建连的写侧。
pub fn delete_account(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::ensure_account_not_goal_bound(conn, id)?;
        ledger_accounts::delete_account(conn, id)
    })
}

/// 归档账户（幂等；域入口直调，旧壳 `archive_account` 命令同一执行协议）：
/// 绑定目标的账户被码化拒绝（守卫住目标域，旧壳命令壳同位）。写连接是成对
/// 建连的写侧。
pub fn archive_account(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_savings_goal::ensure_account_can_archive(conn, id)?;
        ledger_accounts::set_account_archived(conn, id, true)
    })
}

/// 恢复账户（归档的唯一回退动作；域入口直调，旧壳 `unarchive_account` 命令
/// 同一执行协议）。写连接是成对建连的写侧。
pub fn unarchive_account(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_accounts::set_account_archived(conn, id, false)
    })
}

/// 余额调整（域公开写入口直调，旧壳 `adjust_account_balance` 命令同一执行
/// 协议）：把余额校准到目标值，机制为生成一笔与黑洞账户的转账（ADR-0026）；
/// 黑洞账户 ensure 与交易写入同事务（域内 [`ledger_accounts::adjust_account_balance`]
/// 的外层事务壳承担）。返回（新交易 id, 是否按需新建了黑洞账户）。写连接是
/// 成对建连的写侧。
pub fn adjust_account_balance(
    db: &DbState,
    id: &str,
    input: &ledger_accounts::AccountBalanceAdjustInput,
) -> Result<(String, bool)> {
    write_entry_locked(db, |conn| {
        ledger_accounts::adjust_account_balance(conn, id, input)
    })
}
