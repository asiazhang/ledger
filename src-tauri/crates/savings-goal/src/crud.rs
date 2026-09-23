//! 储蓄目标写路径（spec #1750 / issue #1751）：创建目标 = 同一事务内经账户域
//! 公开写入口建专属账户 + 落目标行。
//!
//! 蓄水与取出不进本模块——那是核心交易域既有 `transfer` / `expense` 流水，目标域
//! 零新写入路径（ADR-0133 决策 1）；本域唯一写入面就是目标本体一行。

use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use rusqlite::Connection;

use super::model::SavingsGoalInput;

/// 创建储蓄目标（本地写编排入口，IPC `create_savings_goal`）：
///
/// 1. 目标金额正数守卫先行——码化拒绝、零落库（`savings-goal.target-amount-positive`）；
/// 2. 同一事务内经账户域公开写入口建专属账户（`other` 类型、币种取账本本位币、
///    账户名 = 目标名——目标名权威，随动编辑是后续票），再落目标行绑定该账户。
///
/// 嵌套感知事务（`ensure_transaction`）保证任一步失败整体回滚：不存在「有账户
/// 没目标」的中间态；余额缓存行由账户创建协议同事务落好（ADR-0067）。返回目标 id。
pub fn create_savings_goal(conn: &Connection, input: &SavingsGoalInput) -> Result<String> {
    if input.target_amount_cents <= 0 {
        return Err(AppError::coded(
            "savings-goal.target-amount-positive",
            "目标金额必须为正数",
        ));
    }
    ensure_transaction(conn, || {
        let account_id = ledger_accounts::create_account(
            conn,
            ledger_accounts::AccountInput {
                name: input.name.clone(),
                kind: ledger_accounts::AccountType::Other,
                currency_code: ledger_transaction::amount::default_currency_code(conn)?,
                initial_balance_cents: None,
                credit_limit_cents: None,
                statement_day: None,
                due_day: None,
            },
        )?;
        let now = now_iso();
        let id = new_uuid();
        conn.execute(
            "INSERT INTO goals \
             (id,name,target_amount_cents,deadline,status,planned_monthly_cents,account_id,is_deleted,version,device_id,created_at,updated_at) \
             VALUES (?1,?2,?3,?4,'active',NULL,?5,0,1,?6,?7,?7)",
            rusqlite::params![
                id,
                input.name,
                input.target_amount_cents,
                input.deadline,
                account_id,
                device_id(conn)?,
                now
            ],
        )?;
        Ok(id)
    })
}
