//! 报表域（issue #92 创建，issue #57 迁移 Amount 口径；#405 域目录化 ADR-0056）：
//! 聚合分析读模型——月度汇总、分类聚合、商户排行与报表日期极值。
//!
//! 金额口径全部由 `transaction::amount` 的 kind→度量矩阵单一真源驱动：
//! - 月度汇总毛值三列：income = `income_net`（收入+分红）、
//!   expense = `expense_gross`（= expense_net + refund_gross，净值恒等式）、
//!   refund = `refund_gross`。
//! - 分类聚合净值：expense → `expense_net`（退款冲减）、income → `income_net`（含分红）。
//!   可选年份参数（issue #376）：随报表年份筛选联动，缺省全时段口径不变（API 只增不改）。
//! - 商户消费排行（issue #192）：`expense_net` 按商户聚合、本位币口径，无商户不进排行。
//! - 日期筛选范围（issue #266 / #389）：`{min_date, max_date}`，空库双 None。
//!
//! 核心函数吃 `&Connection` 可直接单测；IPC 参数解包与连接锁管理在壳层
//! `commands::reports`（#405 压平为单文件纯壳）。注册路径与前端调用零改动。
//!
//! 依赖方向恒为「壳层 → reports → 基础设施」，本模块不反向依赖壳层；
//! 对 `transaction::amount` 的消费属域间横向依赖（ADR-0056 决策 2 允许）。

#[cfg(test)]
mod tests;

use rusqlite::Connection;

use crate::db::query::query_all;
use crate::error::Result;
use crate::models::{CategoryShare, DateRange, MerchantShare, MonthlySummary};
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
/// 年份过滤（issue #376）：可选，传年份则按交易日期年份过滤（与月度汇总、
/// 商户排行同款 `substr(date,1,4)` 口径），退款以自身日期参与过滤；
/// 缺省（None）保持全时段口径不变（已发布 API 只增不改）。month/year 可叠加，
/// 占位符按条件追加顺序编号，与参数列表一一对齐。
pub fn category_shares_rows(
    conn: &Connection,
    kind: &str,
    month: Option<&str>,
    year: Option<i64>,
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
    let mut params: Vec<String> = Vec::new();
    if let Some(m) = month {
        params.push(m.to_string());
        sql.push_str(&format!(" AND substr(t.date,1,7)=?{}", params.len()));
    }
    if let Some(y) = year {
        params.push(y.to_string());
        sql.push_str(&format!(" AND substr(t.date,1,4)=?{}", params.len()));
    }
    sql.push_str(" GROUP BY t.category_id ORDER BY net DESC");
    query_all(conn, &sql, rusqlite::params_from_iter(params))
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

/// 报表日期极值范围（issue #266 / #389）：对全部未删除交易各取一次最小/最大日期极值
/// （ISO 文本字典序即时间序，索引友好）；返回日期对 `{min_date, max_date}`（YYYY-MM-DD，空库双 `None`）。
pub fn query_report_date_range(conn: &Connection) -> Result<DateRange> {
    let (min_date, max_date): (Option<String>, Option<String>) = conn.query_row(
        "SELECT MIN(date), MAX(date) FROM transactions WHERE is_deleted=0",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(DateRange { min_date, max_date })
}
