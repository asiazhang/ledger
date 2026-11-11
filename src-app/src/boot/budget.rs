//! 预算读写接缝（#2033）：预算视图的读路径（进度 / 年度汇总 / 今天锚点）
//! 与写路径（创建 / 编辑 / 删除）直调 `ledger-budget` 域入口——旧壳写入口
//! 的「预算写入刻意零信号」在壳内对应物 = 写成功后由视图广播预算失效信号。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

// ---------------------------------------------------------------------------
// 预算读写接缝（#2033）：预算视图的读路径（进度 / 年度汇总 / 支出分类字典）
// 与写路径（创建 / 编辑 / 删除）直调 `ledger-budget` 域入口——与旧壳 budget
// 族命令消费同一域函数，不经 IPC。旧壳写入口的「预算写入刻意零信号」在壳内
// 对应物 = 本壳预算写成功后由视图广播 [`crate::signals`] 预算失效信号。
// ---------------------------------------------------------------------------

/// 「今天」（本地时区日历日）：预算进度窗口与年度汇总的同一时间储，与旧壳
/// `budget_progress` / `budget_annual_summary` 命令的 `Local::now().date_naive()`
/// 同口径（窗口随自然时间自动滚动）。
pub fn today_string() -> String {
    chrono::Local::now().date_naive().to_string()
}

/// 读取预算进度行集（旧壳 `budget_progress` 命令消费的同一域函数）：月预算
/// 统计当前自然月净支出（expense_net），年预算统计当前自然年累计；子分类支出
/// 并入父分类预算，子分类预算只算自身。读连接是成对建连的只读侧（ADR-0117）。
pub fn budget_progress(db: &DbState) -> Result<Vec<ledger_budget::BudgetProgress>> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_budget::budget_progress_rows(&conn, chrono::Local::now().date_naive())
}

/// 读取预算年度汇总（旧壳 `budget_annual_summary` 命令消费的同一域函数）：
/// 年度预算总额（月预算年化）+ 本年已支 + 超标判定，默认币种单币种口径。
/// 读连接是成对建连的只读侧（ADR-0117）。
pub fn budget_annual_summary(db: &DbState) -> Result<ledger_budget::BudgetAnnualSummary> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_budget::budget_annual_summary(&conn, chrono::Local::now().date_naive())
}

/// 创建预算（域公开写入口直调，旧壳 `create_budget` 命令同一执行协议）：金额
/// / 支出分类 / 「分类 + 周期」唯一校验在域内，码化错误原样上抛（呈现面归
/// 视图层）。写连接是成对建连的写侧；op 随写事务提交/回滚（域编排内单点）。
pub fn create_budget(db: &DbState, input: &ledger_budget::BudgetInput) -> Result<String> {
    write_entry_locked(db, |conn| ledger_budget::crud::create_budget(conn, input))
}

/// 编辑预算金额与周期（域公开写入口直调，旧壳 `update_budget` 命令同一执行
/// 协议；分类不可改，改法为删旧建新）。写连接是成对建连的写侧。
pub fn update_budget(
    db: &DbState,
    id: &str,
    amount_cents: i64,
    period: Option<ledger_budget::BudgetPeriod>,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_budget::crud::update_budget(conn, id, amount_cents, period)
    })
}

/// 删除预算（软删；域公开写入口直调，旧壳 `delete_budget` 命令同一执行协议）。
/// 不存在的 id 报码化 NotFound（并发已删的呈现面归视图层）。写连接是成对建连
/// 的写侧。
pub fn delete_budget(db: &DbState, id: &str) -> Result<()> {
    write_entry_locked(db, |conn| ledger_budget::crud::delete_budget(conn, id))
}
