//! 储蓄目标写路径（spec #1750 / issue #1751）：创建目标 = 同一事务内经账户域
//! 公开写入口建专属账户 + 落目标行。
//!
//! 蓄水与取出不进本模块——那是核心交易域既有 `transfer` / `expense` 流水，目标域
//! 零新写入路径（ADR-0133 决策 1）；本域写入面只有目标本体一行，改名联动经
//! 账户域公开写入口随动改专属账户名（目标名权威、账户侧无独立改名入口，不为
//! 此引入账户侧第二写面，issue #1752）。
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use rusqlite::Connection;

use rusqlite::OptionalExtension;

use super::model::{SavingsGoalInput, SavingsGoalUpdateInput};
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

/// 编辑目标（本地写编排入口，IPC `update_savings_goal`，issue #1752）：
///
/// 1. 守卫先行、零落库——目标存在（码化 NotFound `savings-goal.not-found`）、
///    名称非空（`savings-goal.name-required`）、目标金额正数（与创建同校验
///    `savings-goal.target-amount-positive`）、手填计划月存携带时为正数
///    （`savings-goal.planned-monthly-positive`）；
/// 2. 同一事务内四字段全量替换目标行（簿记戳随行走），名称变化时经账户域公开
///    写入口同步专属账户名——目标名权威、账户名随动只读；名称未变则账户零写入
///    （不产生版本噪声与多余同步 op）。
///
/// 嵌套感知事务（`ensure_transaction`）保证任一步失败整体回滚：改名后不存在
/// 「目标新名、账户旧名」的中间态。目标额改小即时生效——达成是读时派生
///（进度读数下一次读取即呈现达成态），本函数不落任何达成标记。
pub fn update_savings_goal(
    conn: &Connection,
    id: &str,
    input: &SavingsGoalUpdateInput,
) -> Result<()> {
    ensure_transaction(conn, || {
        let (existing_name, bound_account_id): (String, String) = conn
            .query_row(
                "SELECT name,account_id FROM goals WHERE id=?1 AND is_deleted=0",
                rusqlite::params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| {
                AppError::codedp_not_found(
                    "savings-goal.not-found",
                    format!("储蓄目标不存在: {id}"),
                    &[id],
                )
            })?;

        // —— 守卫先行（全部校验零落库）——
        let name = input.name.trim().to_string();
        if name.is_empty() {
            return Err(AppError::coded(
                "savings-goal.name-required",
                "目标名称不能为空",
            ));
        }
        if input.target_amount_cents <= 0 {
            return Err(AppError::coded(
                "savings-goal.target-amount-positive",
                "目标金额必须为正数",
            ));
        }
        if let Some(monthly) = input.planned_monthly_cents
            && monthly <= 0
        {
            return Err(AppError::coded(
                "savings-goal.planned-monthly-positive",
                "计划月存必须为正数",
            ));
        }

        // —— 目标行四字段全量替换（名称先 trim 单点：目标行与账户行同值——
        //    两处名字不对不上是 AC1 要防的回归）——
        conn.execute(
            "UPDATE goals SET name=?2,target_amount_cents=?3,deadline=?4,planned_monthly_cents=?5, \
             updated_at=?6,version=version+1,device_id=?7 WHERE id=?1 AND is_deleted=0",
            rusqlite::params![
                id,
                name,
                input.target_amount_cents,
                input.deadline,
                input.planned_monthly_cents,
                now_iso(),
                device_id(conn)?,
            ],
        )?;

        // —— 改名联动（AC1）：目标名权威、账户名随动只读——经账户域公开写入口
        //    同步（记录账户同步 op、走账户编辑同一协议）；名称未变则账户零写入。
        if name != existing_name {
            ledger_accounts::update_account(
                conn,
                &bound_account_id,
                ledger_accounts::AccountUpdateInput {
                    name: Some(name),
                    currency_code: None,
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                },
            )?;
        }
        Ok(())
    })
}
