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
//!
//! 同步 op 产出（issue #1756）：写编排入口在写成功后于同一事务内经
//! [`record_local`](super::command::record_local)（命令模块）追加 Goal op；落库
//! 协议（`write_*`）是本地写与重放共用的执行协议本体、无 op 产出——重放不产
//! 本地 op（ADR-0091），专属账户的级联写入由源端账户 op 以更早时钟先行到达，
//! 重放侧只落目标行。
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::db::{new_uuid, now_iso};
use ledger_infra::error::{AppError, Result};
use ledger_sync_protocol::device::device_id;
use rusqlite::Connection;

use rusqlite::OptionalExtension;

use super::command::{SavingsGoalCommand, record_local};
use super::model::{SavingsGoalInput, SavingsGoalStatus, SavingsGoalUpdateInput};

// ---------------------------------------------------------------------------
// 守卫（本地与重放共用，同码——issue #1756 AC：重放侧守卫与本地同码）。
// ---------------------------------------------------------------------------

/// 目标名称非空守卫（码化 `savings-goal.name-required`）。
fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(AppError::coded(
            "savings-goal.name-required",
            "目标名称不能为空",
        ));
    }
    Ok(())
}

/// 目标金额正数守卫（码化 `savings-goal.target-amount-positive`）。
fn validate_amount(amount_cents: i64) -> Result<()> {
    if amount_cents <= 0 {
        return Err(AppError::coded(
            "savings-goal.target-amount-positive",
            "目标金额必须为正数",
        ));
    }
    Ok(())
}

/// 手填「计划月存」携带时正数守卫（码化 `savings-goal.planned-monthly-positive`）。
fn validate_planned_monthly(planned_monthly_cents: Option<i64>) -> Result<()> {
    if let Some(monthly) = planned_monthly_cents
        && monthly <= 0
    {
        return Err(AppError::coded(
            "savings-goal.planned-monthly-positive",
            "计划月存必须为正数",
        ));
    }
    Ok(())
}

/// 删除余额守卫：专属账户余额非零（余额缓存口径，ADR-0067）时码化拒绝并引导
/// 先转出（`savings-goal.delete-balance-nonzero`）。
fn validate_delete_balance(conn: &Connection, account_id: &str) -> Result<()> {
    let balance = ledger_accounts::balance::cached_balance(conn, account_id)?;
    if balance != 0 {
        return Err(AppError::coded(
            "savings-goal.delete-balance-nonzero",
            "目标账户仍有余额，请先转出后再删除",
        ));
    }
    Ok(())
}

/// 创建储蓄目标（本地写编排入口，IPC `create_savings_goal`）：
///
/// 1. 目标金额正数守卫先行——码化拒绝、零落库（`savings-goal.target-amount-positive`）；
/// 2. 同一事务内经账户域公开写入口建专属账户（`other` 类型、币种取账本本位币、
///    账户名 = 目标名——目标名权威），再落目标行绑定该账户；
/// 3. 写成功后同一事务内追加 Goal Create op（issue #1756）——专属账户 op 由
///    账户创建协议产出、时钟更早，重放端先落账户行再落目标行。
///
/// 嵌套感知事务（`ensure_transaction`）保证任一步失败整体回滚：不存在「有账户
/// 没目标」的中间态；余额缓存行由账户创建协议同事务落好（ADR-0067）。返回目标 id。
pub fn create_savings_goal(conn: &Connection, input: &SavingsGoalInput) -> Result<String> {
    validate_amount(input.target_amount_cents)?;
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
        let id = new_uuid();
        write_create(
            conn,
            &id,
            &account_id,
            &input.name,
            input.target_amount_cents,
            input.deadline.as_deref(),
        )?;
        record_local(
            conn,
            SavingsGoalCommand::Create {
                id: id.clone(),
                account_id,
                name: input.name.clone(),
                target_amount_cents: input.target_amount_cents,
                deadline: input.deadline.clone(),
            },
        )?;
        Ok(id)
    })
}

/// 目标落库协议（本地创建与重放共用，无 op 产出）：正数守卫 + 插入目标行
/// （`planned_monthly_cents` 恒 NULL——创建入口不携带，编辑才引入；创建期状态
/// 恒 active，归档是编辑面的独立命令）。重放端账户行由源端账户 op 先行落地，
/// 外键依赖天然就绪。调用方保证处于写事务内。
fn write_create(
    conn: &Connection,
    id: &str,
    account_id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
) -> Result<()> {
    validate_amount(target_amount_cents)?;
    let now = now_iso();
    conn.execute(
        "INSERT INTO goals \
         (id,name,target_amount_cents,deadline,status,planned_monthly_cents,account_id,is_deleted,version,device_id,created_at,updated_at) \
         VALUES (?1,?2,?3,?4,'active',NULL,?5,0,1,?6,?7,?7)",
        rusqlite::params![
            id,
            name,
            target_amount_cents,
            deadline,
            account_id,
            device_id(conn)?,
            now
        ],
    )?;
    Ok(())
}

/// 重放执行：创建目标（正数守卫同码生效；账户依赖缺失挂起待裁决）。
pub(crate) fn replay_create(
    conn: &Connection,
    id: &str,
    account_id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
) -> Result<()> {
    write_create(conn, id, account_id, name, target_amount_cents, deadline)
}

/// 编辑目标（本地写编排入口，IPC `update_savings_goal`，issue #1752）：
///
/// 1. 守卫先行、零落库——目标存在（码化 NotFound `savings-goal.not-found`）、
///    名称非空（`savings-goal.name-required`）、目标金额正数（与创建同校验
///    `savings-goal.target-amount-positive`）、手填计划月存携带时为正数
///    （`savings-goal.planned-monthly-positive`）；
/// 2. 同一事务内四字段全量替换目标行（落库协议与重放共用），名称变化时经账户域
///    公开写入口同步专属账户名——目标名权威、账户名随动只读；名称未变则账户
///    零写入（不产生版本噪声与多余账户同步 op）；
/// 3. 写成功后同一事务内追加 Goal Update op（issue #1756，载荷携带落定值）——
///    改名联动产生的账户 op 时钟更早，重放端先改账户名再替换目标行。
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
        // 目标行四字段全量替换（名称先 trim 单点：目标行与账户行同值——
        // 两处名字不对不上是 AC1 要防的回归）。
        let name = input.name.trim().to_string();
        validate_name(&name)?;
        validate_amount(input.target_amount_cents)?;
        validate_planned_monthly(input.planned_monthly_cents)?;

        write_update(
            conn,
            id,
            &name,
            input.target_amount_cents,
            input.deadline.as_deref(),
            input.planned_monthly_cents,
        )?;

        // —— 改名联动（AC1）：目标名权威、账户名随动只读——经账户域公开写入口
        //    同步（记录账户同步 op、走账户编辑同一协议）；名称未变则账户零写入。
        if name != existing_name {
            ledger_accounts::update_account(
                conn,
                &bound_account_id,
                ledger_accounts::AccountUpdateInput {
                    name: Some(name.clone()),
                    currency_code: None,
                    credit_limit_cents: None,
                    statement_day: None,
                    due_day: None,
                },
            )?;
        }

        record_local(
            conn,
            SavingsGoalCommand::Update {
                id: id.to_string(),
                name,
                target_amount_cents: input.target_amount_cents,
                deadline: input.deadline.clone(),
                planned_monthly_cents: input.planned_monthly_cents,
            },
        )?;
        Ok(())
    })
}

/// 目标编辑落库协议（本地编辑与重放共用，无 op 产出、无改名联动——专属账户
/// 改名由源端账户 op 先行到达，重放侧只落目标行）：存在性检查 + 守卫 +
/// 四字段全量替换（簿记戳随行走）。调用方保证处于写事务内。
fn write_update(
    conn: &Connection,
    id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
    planned_monthly_cents: Option<i64>,
) -> Result<()> {
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
    validate_name(name)?;
    validate_amount(target_amount_cents)?;
    validate_planned_monthly(planned_monthly_cents)?;

    conn.execute(
        "UPDATE goals SET name=?2,target_amount_cents=?3,deadline=?4,planned_monthly_cents=?5, \
         updated_at=?6,version=version+1,device_id=?7 WHERE id=?1 AND is_deleted=0",
        rusqlite::params![
            id,
            name,
            target_amount_cents,
            deadline,
            planned_monthly_cents,
            now_iso(),
            device_id(conn)?,
        ],
    )?;
    Ok(())
}

/// 重放执行：编辑目标（存在性 / 名称 / 金额 / 手填月存守卫同码生效）。
pub(crate) fn replay_update(
    conn: &Connection,
    id: &str,
    name: &str,
    target_amount_cents: i64,
    deadline: Option<&str>,
    planned_monthly_cents: Option<i64>,
) -> Result<()> {
    write_update(
        conn,
        id,
        name,
        target_amount_cents,
        deadline,
        planned_monthly_cents,
    )
}

// ---------------------------------------------------------------------------
// 生命周期（issue #1754）：归档 / 取消归档、删除守卫与账户禁删守卫。
// ---------------------------------------------------------------------------

/// 归档 / 取消归档（本地写编排入口，IPC `archive_savings_goal` /
/// `unarchive_savings_goal`，issue #1754）：状态闭集二值单点翻转，写成功后
/// 同一事务内追加 Goal Archive / Unarchive op（issue #1756）。
///
/// 归档是收纳不是清算——不删账户、不动流水、不改关联计划（ADR-0133 决策 3：
/// 归档不删任何东西）；达成为读时派生展示态，本函数不写任何达成标记。
fn set_savings_goal_status(conn: &Connection, id: &str, status: SavingsGoalStatus) -> Result<()> {
    ensure_transaction(conn, || {
        write_status(conn, id, status)?;
        record_local(
            conn,
            if status == SavingsGoalStatus::Archived {
                SavingsGoalCommand::Archive { id: id.to_string() }
            } else {
                SavingsGoalCommand::Unarchive { id: id.to_string() }
            },
        )?;
        Ok(())
    })
}

/// 目标状态翻转落库协议（本地与重放共用，无 op 产出）：存在性检查 + 状态闭集
/// 单点翻转（簿记戳随行走）。调用方保证处于写事务内。
fn write_status(conn: &Connection, id: &str, status: SavingsGoalStatus) -> Result<()> {
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
}

/// 重放执行：归档 / 取消归档（存在性守卫同码生效）。
pub(crate) fn replay_status(conn: &Connection, id: &str, status: SavingsGoalStatus) -> Result<()> {
    write_status(conn, id, status)
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
/// 1. 守卫先行——目标存在（码化 NotFound）；专属账户余额非零时码化拒绝并引导
///    先转出（`savings-goal.delete-balance-nonzero`）；
/// 2. 余额为零：同一事务内目标行软删 + 级联软删专属账户（经账户域公开写入口，
///    流水保留可查——账户删除协议本就不删交易，词汇表「目标账户·删除受限」）；
/// 3. 写成功后同一事务内追加 Goal Delete op（issue #1756）——级联账户的删除
///    op 由账户删除协议产出、时钟更早，重放端先删账户行再软删目标行。
///
/// 嵌套感知事务保证任一步失败整体回滚：不存在「目标已删、账户还在」的中间态。
/// 归档态目标同样可删（守卫只看余额，不看归档与否）。
pub fn delete_savings_goal(conn: &Connection, id: &str) -> Result<()> {
    ensure_transaction(conn, || {
        let account_id = bound_account_id(conn, id)?;
        validate_delete_balance(conn, &account_id)?;
        write_delete(conn, id)?;
        ledger_accounts::delete_account(conn, &account_id)?;
        record_local(conn, SavingsGoalCommand::Delete { id: id.to_string() })?;
        Ok(())
    })
}

/// 重放执行：删除目标（禁删守卫同码生效——余额非零挂起待裁决；级联软删专属
/// 账户不在此执行，由源端账户 op 先行到达，重放不产本地 op）。
pub(crate) fn replay_delete(conn: &Connection, id: &str) -> Result<()> {
    let account_id = bound_account_id(conn, id)?;
    validate_delete_balance(conn, &account_id)?;
    write_delete(conn, id)
}

/// 目标行存在性读取单点：未删除目标的专属账户绑定（本地删除 / 重放删除共用，
/// 不存在时报码化 NotFound `savings-goal.not-found`）。
fn bound_account_id(conn: &Connection, id: &str) -> Result<String> {
    conn.query_row(
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
    })
}

/// 目标软删协议（本地删除与重放共用，无 op 产出）：软删目标行 + 簿记戳更新
/// （级联软删专属账户是本地编排入口的事务内联动，不在协议本体——重放侧账户
/// 行由源端账户 op 先行删除）。调用方保证处于写事务内。
fn write_delete(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE goals SET is_deleted=1,updated_at=?2,version=version+1,device_id=?3 \
         WHERE id=?1 AND is_deleted=0",
        rusqlite::params![id, now_iso(), device_id(conn)?],
    )?;
    Ok(())
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
