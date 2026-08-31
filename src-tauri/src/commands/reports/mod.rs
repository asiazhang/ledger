//! 报表（issue #92 创建，issue #57 迁移 Amount 口径）：命令外壳 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `tests`：针对模块接口的测试（期望值由度量矩阵逐行求和得出，不复制生产 SQL）。
//!
//! 金额口径全部由 `transaction::amount` 的 kind→度量矩阵单一真源驱动：
//! - 月度汇总毛值三列：income = `income_net`（收入+分红）、
//!   expense = `expense_gross`（= expense_net + refund_gross，净值恒等式）、
//!   refund = `refund_gross`。
//! - 分类聚合净值：expense → `expense_net`（退款冲减）、income → `income_net`（含分红）。
//! - 商户消费排行（issue #192）：`expense_net` 按商户聚合、本位币口径，无商户不进排行。
//! - 年份筛选范围（issue #266）：`[最早交易年份, max(当前年, 最新交易年份)]`，空库回退单当前年。
//!
//! 命令层为薄壳（锁 DbState 后调核心函数），核心函数吃 `&Connection` 可直接单测。
//! 对外暴露的命令经 `commands/mod.rs` 的 `pub use reports::*` 重导出，
//! 注册路径与前端调用零改动。

#[cfg(test)]
mod tests;

use chrono::Datelike;
use rusqlite::Connection;
use tauri::State;

use crate::db::DbState;
use crate::db::query::query_all;
use crate::error::{AppError, Result};
use crate::models::{CategoryShare, MerchantShare, MonthlySummary, YearRange};
use crate::transaction::amount::{
    Measure, contributing_kinds_sql, expense_gross_expr, expense_net_expr, income_net_expr,
    refund_gross_expr,
};

/// 月度汇总（毛值三列）：按月分组，income / expense（毛）/ refund 独立成列，
/// 毛值与净值并存展示（用户可同时看到毛支出与退款）。
pub fn monthly_summary_rows(conn: &Connection, year: i64) -> Result<Vec<MonthlySummary>> {
    let sql = format!(
        "SELECT substr(date,1,7) AS month, \
         SUM({income}) AS income, \
         SUM({expense_gross}) AS expense, \
         SUM({refund_gross}) AS refund \
         FROM transactions WHERE substr(date,1,4)=?1 AND is_deleted=0 \
         GROUP BY month ORDER BY month",
        income = income_net_expr("transactions"),
        expense_gross = expense_gross_expr("transactions"),
        refund_gross = refund_gross_expr("transactions"),
    );
    query_all(conn, &sql, rusqlite::params![format!("{year}")])
}

/// 分类聚合（净值）：`kind == "expense"` 用 `expense_net`（退款冲减支出），
/// 其余（income）用 `income_net`（收入+分红）；参与 kind 由矩阵导出。
pub fn category_shares_rows(
    conn: &Connection,
    kind: &str,
    month: Option<&str>,
) -> Result<Vec<CategoryShare>> {
    let (measure, expr) = if kind == "expense" {
        (Measure::ExpenseNet, expense_net_expr("t"))
    } else {
        (Measure::IncomeNet, income_net_expr("t"))
    };
    let kinds = contributing_kinds_sql(measure);
    let mut sql = format!(
        "SELECT t.category_id, COALESCE(c.name,'未分类'), SUM({expr}) AS net \
         FROM transactions t LEFT JOIN categories c ON c.id=t.category_id \
         WHERE t.kind IN ({kinds}) AND t.is_deleted=0"
    );
    if month.is_some() {
        sql.push_str(" AND substr(t.date,1,7)=?1");
    }
    sql.push_str(" GROUP BY t.category_id ORDER BY net DESC");
    // month 为 None 时参数列表为空，与无占位符的 SQL 对齐。
    let month_params: Vec<&str> = month.into_iter().collect();
    query_all(conn, &sql, rusqlite::params_from_iter(month_params))
}

/// 商户消费排行（净额，issue #192）：`expense_net`（毛支出 − 退款）按商户聚合、
/// 本位币口径（`amount_native_cents`），与核心交易域净值恒等式一致。
/// 无商户关联的交易不进排行；软删商户的历史引用照常统计（JOIN 不滤 is_deleted）。
pub fn merchant_shares_rows(conn: &Connection, year: i64) -> Result<Vec<MerchantShare>> {
    let kinds = contributing_kinds_sql(Measure::ExpenseNet);
    let sql = format!(
        "SELECT t.merchant_id, m.name, SUM({expr}) AS net \
         FROM transactions t JOIN merchants m ON m.id=t.merchant_id \
         WHERE t.kind IN ({kinds}) AND t.is_deleted=0 AND substr(t.date,1,4)=?1 \
         GROUP BY t.merchant_id ORDER BY net DESC, m.name",
        expr = expense_net_expr("t"),
    );
    query_all(conn, &sql, rusqlite::params![format!("{year}")])
}

/// 报表年份筛选范围（issue #266）：对全部未删除交易各取一次最小/最大日期极值
/// （ISO 文本字典序即时间序，索引友好），年份在 Rust 侧截取；
/// 闭区间 `[最早交易年份, max(当前年, 最新交易年份)]`，空库回退 `[当前年, 当前年]`。
pub fn query_report_year_range(conn: &Connection, today: chrono::NaiveDate) -> Result<YearRange> {
    let (min_date, max_date): (Option<String>, Option<String>) = conn.query_row(
        "SELECT MIN(date), MAX(date) FROM transactions WHERE is_deleted=0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let current_year = i64::from(today.year());
    let Some((min_date, max_date)) = min_date.zip(max_date) else {
        return Ok(YearRange {
            min_year: current_year,
            max_year: current_year,
        });
    };
    Ok(YearRange {
        min_year: stored_date_year(&min_date)?,
        max_year: current_year.max(stored_date_year(&max_date)?),
    })
}

/// 存量交易日期（写入路径保证 `YYYY-MM-DD`）截取年份；异常格式视为数据损坏报错。
fn stored_date_year(date: &str) -> Result<i64> {
    let d = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|e| AppError::Db(format!("交易日期格式异常 '{date}': {e}")))?;
    Ok(i64::from(d.year()))
}

#[tauri::command]
pub fn monthly_summary(db: State<'_, DbState>, year: i64) -> Result<Vec<MonthlySummary>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    monthly_summary_rows(&conn, year)
}

#[tauri::command]
pub fn merchant_shares(db: State<'_, DbState>, year: i64) -> Result<Vec<MerchantShare>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    merchant_shares_rows(&conn, year)
}

/// 报表年份筛选范围（issue #266）：只读命令，前端年份下拉的可选范围；
/// 当前年取本地今日（同预算进度注入口径）。
#[tauri::command]
pub fn report_year_range(db: State<'_, DbState>) -> Result<YearRange> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_report_year_range(&conn, chrono::Local::now().date_naive())
}

#[tauri::command]
pub fn category_shares(
    db: State<'_, DbState>,
    kind: String,
    month: Option<String>,
) -> Result<Vec<CategoryShare>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    category_shares_rows(&conn, &kind, month.as_deref())
}
