//! 储蓄目标读路径（词汇表「蓄水进度」）：目标清单 × 逐目标进度 + 双向推算，
//! 一次读出。
//!
//! 进度 = 专属账户余额——经账户域余额缓存读取（`cached_balance`：缺失报码化
//! 错误、不静默回退，ADR-0067）；还差为带符号差值、达成为读时派生，均不持久化；
//! 币种随行携带（= 专属账户币种，目标域不折算）。双向推算（ETA / 所需月存 /
//! 落后超前差值，issue #1753）的节奏解析与算术住 [`super::pace`]，本模块只把
//! 三段读数（目标行、余额缓存、关联计划节奏）拼成同一行。
//!
//! **读快照一致性（issue #1699 / #1702 纪律）**：目标行 × 余额 × 关联计划节奏
//! 是同屏口径的多语句读闭包（计划在语句间从在用转停即「余额有节奏、推算无计
//! 划」或反向错位），整体收进同一读事务（嵌套感知）；验收含读快照探针测试
//!（删除接线即红）。

use chrono::NaiveDate;
use rusqlite::Connection;

use ledger_infra::db::query::{FromRow, query_all};
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;

use super::model::{SavingsGoal, SavingsGoalPaceSource, SavingsGoalProgress, goal_from_row};
use super::pace::{linked_plan_pace_monthly, project};

/// 目标行 + 币种（两阶段读的第一段收齐用）：列序见下方 SELECT（实体 12 列 + 币种）。
struct GoalWithCurrency {
    goal: SavingsGoal,
    currency_code: String,
}

impl FromRow for GoalWithCurrency {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            goal: goal_from_row(row)?,
            currency_code: row.get(12)?,
        })
    }
}

/// 列出全部未删除目标的蓄水进度与双向推算，按创建先后排序。`today` 由命令层
/// 注入（本地今日），单测可传固定日期获得确定性口径（订阅花费 / 预算进度同款）。
pub fn list_savings_goal_progress(
    conn: &Connection,
    today: NaiveDate,
) -> Result<Vec<SavingsGoalProgress>> {
    ensure_transaction(conn, || list_savings_goal_progress_within_tx(conn, today))
}

/// 进度读取本体（无事务语义，由 [`list_savings_goal_progress`] 的
/// [`ensure_transaction`] 包裹）。
fn list_savings_goal_progress_within_tx(
    conn: &Connection,
    today: NaiveDate,
) -> Result<Vec<SavingsGoalProgress>> {
    // 两阶段读（读快照探针形状）：先收齐目标行（语句即收即结），再逐目标富化
    //（余额缓存 × 关联计划节奏）——goals 语句不横跨富化语句持锁，探针的注入写
    // 在无快照保护时才能真正落进语句之间（删除接线即红，issue #1702 形态）。
    let goals: Vec<GoalWithCurrency> = query_all(
        conn,
        "SELECT g.id,g.name,g.target_amount_cents,g.deadline,g.status,g.planned_monthly_cents, \
         g.account_id,g.created_at,g.updated_at,g.version,g.device_id,g.is_deleted,a.currency_code \
         FROM goals g JOIN accounts a ON a.id = g.account_id \
         WHERE g.is_deleted=0 ORDER BY g.created_at",
        [],
    )?;
    let mut progress = Vec::new();
    for GoalWithCurrency {
        goal,
        currency_code,
    } in goals
    {
        let saved_cents = ledger_accounts::balance::cached_balance(conn, &goal.account_id)?;
        let remaining_cents = goal.target_amount_cents - saved_cents;
        // 节奏来源闭集二值：关联在用计划折算月存优先，无在用计划取手填值
        //（pace 模块是关联计划查找的读口径单点）。
        let (pace_monthly_cents, pace_source) =
            match linked_plan_pace_monthly(conn, &goal.account_id)? {
                Some(monthly) => (Some(monthly), Some(SavingsGoalPaceSource::Plan)),
                None => (
                    goal.planned_monthly_cents,
                    goal.planned_monthly_cents
                        .map(|_| SavingsGoalPaceSource::Manual),
                ),
            };
        let projection = project(
            goal.deadline.as_deref(),
            remaining_cents,
            pace_monthly_cents,
            pace_source,
            today,
        );
        progress.push(SavingsGoalProgress {
            achieved: saved_cents >= goal.target_amount_cents,
            goal,
            saved_cents,
            remaining_cents,
            currency_code,
            pace_monthly_cents: projection.pace_monthly_cents,
            pace_source: projection.pace_source,
            eta_months: projection.eta_months,
            eta_month: projection.eta_month,
            required_monthly_cents: projection.required_monthly_cents,
            pace_delta_cents: projection.pace_delta_cents,
        });
    }
    Ok(progress)
}
