//! 储蓄目标读路径（词汇表「蓄水进度」）：目标清单 × 逐目标进度，一次读出。
//!
//! 进度 = 专属账户余额——经账户域余额缓存读取（`cached_balance`：缺失报码化
//! 错误、不静默回退，ADR-0067）；还差为带符号差值、达成为读时派生，均不持久化；
//! 币种随行携带（= 专属账户币种，目标域不折算）。

use ledger_infra::error::Result;
use rusqlite::Connection;

use super::model::{SavingsGoalProgress, goal_from_row};

/// 列出全部未删除目标的蓄水进度，按创建先后排序。
pub fn list_savings_goal_progress(conn: &Connection) -> Result<Vec<SavingsGoalProgress>> {
    let mut stmt = conn.prepare(
        "SELECT g.id,g.name,g.target_amount_cents,g.deadline,g.status,g.planned_monthly_cents, \
         g.account_id,g.created_at,g.updated_at,g.version,g.device_id,g.is_deleted,a.currency_code \
         FROM goals g JOIN accounts a ON a.id = g.account_id \
         WHERE g.is_deleted=0 ORDER BY g.created_at",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((goal_from_row(row)?, row.get::<_, String>(12)?))
    })?;
    let mut progress = Vec::new();
    for row in rows {
        let (goal, currency_code) = row?;
        let saved_cents = ledger_accounts::balance::cached_balance(conn, &goal.account_id)?;
        let remaining_cents = goal.target_amount_cents - saved_cents;
        progress.push(SavingsGoalProgress {
            achieved: saved_cents >= goal.target_amount_cents,
            goal,
            saved_cents,
            remaining_cents,
            currency_code,
        });
    }
    Ok(progress)
}
