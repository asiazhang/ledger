//! 储蓄目标写路径（spec #1750 / issue #1751）：创建目标 = 同一事务内经账户域
//! 公开写入口建专属账户 + 落目标行。
//!
//! 蓄水与取出不进本模块——那是核心交易域既有 `transfer` / `expense` 流水，目标域
//! 零新写入路径（ADR-0133 决策 1）；本域写入面只有目标本体一行，改名联动经
//! 账户域公开写入口随动改专属账户名（目标名权威、账户侧无独立改名入口，不为
//! 此引入账户侧第二写面，issue #1752）。生命周期守卫（issue #1754）亦在本域：
//! 归档 / 取消归档（状态闭集二值单点翻转）、删除目标（余额非零码化拒绝引导先
//! 转出、余额为零级联软删专属账户）与在用目标的专属账户禁删守卫（壳层编排
//! 消费——账户域不反向依赖目标域，ADR-0133 决策 4）。
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use rusqlite::Connection;

use rusqlite::OptionalExtension;

use super::model::{SavingsGoalInput, SavingsGoalStatus, SavingsGoalUpdateInput};
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

// ---------------------------------------------------------------------------
// 生命周期（issue #1754）：归档 / 取消归档、删除守卫与账户禁删守卫。
// ---------------------------------------------------------------------------

/// 归档 / 取消归档（本地写编排入口，IPC `archive_savings_goal` /
/// `unarchive_savings_goal`，issue #1754）：状态闭集二值单点翻转。
///
/// 归档是收纳不是清算——不删账户、不动流水、不改关联计划（ADR-0133 决策 3：
/// 归档不删任何东西）；达成为读时派生展示态，本函数不写任何达成标记。取消
/// 归档恢复 active 即回默认列表。目标不存在时报码化 NotFound。
fn set_savings_goal_status(conn: &Connection, id: &str, status: SavingsGoalStatus) -> Result<()> {
    ensure_transaction(conn, || {
        let exists: Option<i64> = conn
            .query_row(
                "SELECT 1 FROM goals WHERE id=?1 AND is_deleted=0",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(AppError::codedp_not_found(
                "savings-goal.not-found",
                format!("储蓄目标不存在: {id}"),
                &[id],
            ));
        }
        conn.execute(
            "UPDATE goals SET status=?2,updated_at=?3,version=version+1,device_id=?4 \
             WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id, status.to_string(), now_iso(), device_id(conn)?],
        )?;
        Ok(())
    })
}

/// 归档目标（词汇表「达成与归档」）：退出默认列表（读命令照常返回该行，
/// `status='archived'` 由视图层分组展示），账户 / 流水 / 关联计划原样保留。
pub fn archive_savings_goal(conn: &Connection, id: &str) -> Result<()> {
    set_savings_goal_status(conn, id, SavingsGoalStatus::Archived)
}

/// 取消归档目标：恢复 active 回默认列表。
pub fn unarchive_savings_goal(conn: &Connection, id: &str) -> Result<()> {
    set_savings_goal_status(conn, id, SavingsGoalStatus::Active)
}

/// 删除目标（本地写编排入口，IPC `delete_savings_goal`，issue #1754）：
///
/// 1. 守卫先行——目标存在（码化 NotFound）；专属账户余额非零（余额缓存口径，
///    ADR-0067）时码化拒绝并引导先转出（`savings-goal.delete-balance-nonzero`）；
/// 2. 余额为零：同一事务内目标行软删 + 级联软删专属账户（经账户域公开写入口，
///    流水保留可查——账户删除协议本就不删交易，词汇表「目标账户·删除受限」）。
///
/// 嵌套感知事务保证任一步失败整体回滚：不存在「目标已删、账户还在」的中间态。
/// 归档态目标同样可删（守卫只看余额，不看归档与否）。
pub fn delete_savings_goal(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        let account_id: String = conn
            .query_row(
                "SELECT account_id FROM goals WHERE id=?1 AND is_deleted=0",
                rusqlite::params![id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                AppError::codedp_not_found(
                    "savings-goal.not-found",
                    format!("储蓄目标不存在: {id}"),
                    &[id],
                )
            })?;
        let balance = ledger_accounts::balance::cached_balance(conn, &account_id)?;
        if balance != 0 {
            return Err(AppError::coded(
                "savings-goal.delete-balance-nonzero",
                "目标账户仍有余额，请先转出后再删除",
            ));
        }
        conn.execute(
            "UPDATE goals SET is_deleted=1,updated_at=?2,version=version+1,device_id=?3 \
             WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id, now_iso(), device_id(conn)?],
        )?;
        ledger_accounts::delete_account(conn, &account_id)
    })
}

/// 在用目标的专属账户禁删守卫（壳层编排消费，issue #1754 / ADR-0133 决策 4）：
/// 账户域不反向依赖目标域，「删账户命令先经目标域校验」由壳层在删除账户入口
///（IPC 与 HTTP 同构调用）先行调用本函数——命中任一未删除目标的绑定即码化拒绝，
/// 引导先在目标页删除对应目标。守卫只读、零写入、零信号。
pub fn ensure_account_not_goal_bound(conn: &Connection, account_id: &str) -> Result<()> {
    let bound: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM goals WHERE account_id=?1 AND is_deleted=0 LIMIT 1",
            rusqlite::params![account_id],
            |row| row.get(0),
        )
        .optional()?;
    if bound.is_some() {
        return Err(AppError::coded(
            "savings-goal.account-in-use",
            "该账户是储蓄目标专属账户，请先删除对应储蓄目标",
        ));
    }
    Ok(())
}
