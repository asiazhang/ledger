//! 预算进度域行为：按当前自然月/年实时计算支出净额。

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::model::BudgetProgress;
use crate::db::query::query_all;
use crate::error::Result;
use crate::transaction::amount::{Measure, contributing_kinds_sql, expense_net_expr};

/// 预算 spent = `expense_net`（毛支出 − 退款，退款冲减支出），与报表分类净值同口径；参与 kind 由矩阵导出（不含 buy/sell 等投资类）。
/// 窗口为当前自然周期，由注入的 `today` 驱动、与存储的 start_date 无关（存量旧日期
/// 行零迁移滚动生效）：monthly = today 所在自然月，yearly = today 所在自然年。
/// `today` 由命令层注入（本地今日，与订阅花费口径一致），测试注入固定值。
pub fn budget_progress_rows(conn: &Connection, today: NaiveDate) -> Result<Vec<BudgetProgress>> {
    let kinds = contributing_kinds_sql(Measure::ExpenseNet);
    let month = format!("{:04}-{:02}", today.year(), today.month());
    let year = format!("{:04}", today.year());
    let sql = format!(
        "SELECT b.id,b.category_id,b.period,b.amount_cents,b.start_date,b.created_at,b.updated_at,b.version,b.device_id,b.is_deleted,c.name, \
         COALESCE((SELECT SUM({expense_net}) \
                   FROM transactions t \
                   JOIN categories tc ON tc.id=t.category_id \
                   WHERE (tc.id=b.category_id OR tc.parent_id=b.category_id) \
                     AND t.kind IN ({kinds}) \
                     AND t.is_deleted=0 \
                     AND (CASE WHEN b.period='monthly' THEN substr(t.date,1,7)=?1 \
                               ELSE substr(t.date,1,4)=?2 END)),0) \
         FROM budgets b LEFT JOIN categories c ON c.id=b.category_id \
         WHERE b.is_deleted=0 ORDER BY b.created_at",
        expense_net = expense_net_expr("t"),
    );
    query_all(conn, &sql, rusqlite::params![month, year])
}
