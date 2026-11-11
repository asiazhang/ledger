//! 定时计划读写接缝（#2042）：期次详情、订阅花费概览与写路径（建计划 /
//! 改状态 / 改订阅 / 执行期次 / 预生成期次）直调 `ledger-scheduled` 域
//! 入口——定时视图四件套消费。

use ledger_infra::db::DbState;
use ledger_infra::error::Result;

use super::conn_lock;
use crate::shell_support::write_entry_locked;

// ---------------------------------------------------------------------------
// 定时计划读写接缝（#2042）：订阅式计划清单 / 计划详情 / 订阅花费总览读 +
// 计划写入（建档 / 状态 / 编辑 / 期次执行 / 期次展开）+ 即建商户直调
// `ledger-scheduled` / `ledger-merchants` 域入口——与旧壳 scheduled 族命令
//（listScheduledTransactions / getScheduledTransactionDetail /
// subscriptionSpendOverview / createScheduledTransaction /
// updateScheduledTransactionStatus / updateScheduledSubscription /
// executeScheduledOccurrence / expandScheduledOccurrences / createMerchant）
// 消费同一域函数，不经 IPC。写连接是成对建连的写侧，读连接是只读侧。
// 旧壳「计划写入刻意零信号」在壳内对应物 = 本壳视图层在写成功回调里自行
// 重拉（清单 / 花费面板）；期次执行落交易行属交易域变化，视图层手动广播
// [`crate::signals`] 交易失效信号（交易列表重读）。
// ---------------------------------------------------------------------------

/// 计划完整详情（旧壳 `getScheduledTransactionDetail` 命令消费的同一域函数）：
/// 含扩展字段与全量期次（含 failed / cancelled 各状态，按 scheduled_date
/// 升序）。计划级读快照一致性收口在域内 `ensure_transaction`。读连接是成对
/// 建连的只读侧（ADR-0117）。
pub fn scheduled_plan_detail(
    db: &DbState,
    id: &str,
) -> Result<ledger_scheduled::ScheduledTransactionDetail> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_scheduled::get_plan_detail(&conn, id)
}

/// 订阅花费总览（旧壳 `subscriptionSpendOverview` 命令消费的同一域函数）：
/// 逐订阅行 + 本月 / 本年实际花费 + 过去 12 个月逐月序列 + 折算月 / 年推算
/// 成本（本位币）。今日取本地时区日历日（旧壳命令层注入同口径）。读失败
///（如本位币缺汇率）不静默混算：面板呈现失败态（旧壳 loadFailed 同语义）。读
/// 连接是成对建连的只读侧（ADR-0117）。
pub fn subscription_spend_overview(
    db: &DbState,
) -> Result<ledger_scheduled::SubscriptionSpendOverview> {
    let conn = conn_lock(&db.read_conn)?;
    ledger_scheduled::query_subscription_spend(&conn, chrono::Local::now().date_naive())
}

/// 创建定时交易计划（域公开写入口直调，旧壳 `createScheduledTransaction`
/// 命令同一执行协议）：建档协议（校验 + 核心表 / 扩展表 + 预生成期次）与 op
/// 产出一事务收口在域内 create_plan（自持事务）。返回计划 id。写连接是成对
/// 建连的写侧。
pub fn create_scheduled_plan(
    db: &DbState,
    input: ledger_scheduled::CreateScheduledInput,
) -> Result<String> {
    write_entry_locked(db, |conn| ledger_scheduled::create_plan(conn, input))
}

/// 变更计划状态（域公开写入口直调，旧壳 `updateScheduledTransactionStatus`
/// 命令同一执行协议）：守护裁决 + 取消级联在域内（opcode 单点）。写连接是
/// 成对建连的写侧。
pub fn update_scheduled_plan_status(
    db: &DbState,
    id: &str,
    new_status: ledger_scheduled::ScheduledStatus,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_scheduled::update_plan_status(conn, id, new_status)
    })
}

/// 编辑订阅计划的非金额字段（域公开写入口直调，旧壳
/// `updateScheduledSubscription` 命令同一执行协议）：金额哨兵拒绝、商户保持
/// 历史引用语义与「保单缴费协议不挂商户」裁决全在域内单点。写连接是成对
/// 建连的写侧。
pub fn update_scheduled_subscription(
    db: &DbState,
    input: ledger_scheduled::UpdateSubscriptionInput,
) -> Result<()> {
    write_entry_locked(db, |conn| {
        ledger_scheduled::update_subscription(conn, input)
    })
}

/// 执行一条 pending / failed 期次（域公开写入口直调，旧壳
/// `executeScheduledOccurrence` 命令同一执行协议）：经 Writer 接缝归一化折算
/// 落交易行、回填期次、计划完成检查全在域内单正文（事务自持）。返回落地
/// 交易 id。写连接是成对建连的写侧。
pub fn execute_scheduled_occurrence(db: &DbState, occurrence_id: &str) -> Result<String> {
    write_entry_locked(db, |conn| {
        ledger_scheduled::execute_occurrence(conn, occurrence_id)
    })
}

/// 预生成下一批期次（域公开写入口直调，旧壳 `expandScheduledOccurrences`
/// 命令同一执行协议）：返回本次新生成的期次 id 列表。写连接是成对建连的
/// 写侧。
pub fn expand_scheduled_occurrences(db: &DbState, id: &str) -> Result<Vec<String>> {
    write_entry_locked(db, |conn| ledger_scheduled::expand_occurrences(conn, id))
}
