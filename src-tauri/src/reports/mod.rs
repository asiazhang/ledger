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
//!   可选 topN 截断与全量合计（issue #588）：`top_n`（None = 全量）在排序之后应用、
//!   收口后端；载荷携带本期全部商户净支出合计（tooltip 占比分母，与截断无关）。
//!
//! 期间过滤（issue #411 / ADR-0057）：三个聚合函数统一新增可选 `from`/`to`
//! （YYYY-MM-DD 含边界，字典序区间比较），任一端存在即期间口径、遗留参数
//! （year / month+year）不参与；双端皆缺省回退遗留口径（已发布 API 只增不改，
//! 遗留参数冻结保留、前端不再使用）。
//! - 日期筛选范围（issue #266 / #389）：`{min_date, max_date}`，空库双 None。
//!
//! 报表读模型类型集中本域 [`model`]（#421 随域归位），消费方经域路径逐类型
//! 显式 import。
//!
//! 核心函数吃 `&Connection` 可直接单测；IPC 参数解包与连接锁管理在壳层
//! `commands::reports`（#405 压平为单文件纯壳）。注册路径与前端调用零改动。
//!
//! 依赖方向恒为「壳层 → reports → 基础设施」，本模块不反向依赖壳层；
//! 对 `transaction::amount` 的消费属域间横向依赖（ADR-0056 决策 2 允许）。

#[cfg(test)]
mod tests;

mod model;

pub use model::{CategoryShare, DateRange, MerchantShare, MerchantSharesReport, MonthlySummary};

use rusqlite::Connection;

use crate::db::query::query_all;
use crate::error::Result;
use crate::transaction::amount::{
    Measure, contributing_kinds_sql, expense_gross_expr, expense_net_expr, income_net_expr,
    refund_gross_expr,
};

/// 期间过滤条件追加单点（issue #411）：`from`/`to` 各边独立、含边界，
/// 占位符按追加顺序编号与参数列表对齐；`column` 为带别名限定的日期列
///（如 `date` / `t.date`）。字典序区间比较与交易列表日期过滤同构。
fn push_period_conditions(
    sql: &mut String,
    params: &mut Vec<String>,
    column: &str,
    from: Option<&str>,
    to: Option<&str>,
) {
    if let Some(f) = from {
        params.push(f.to_string());
        sql.push_str(&format!(" AND {column}>=?{}", params.len()));
    }
    if let Some(t) = to {
        params.push(t.to_string());
        sql.push_str(&format!(" AND {column}<=?{}", params.len()));
    }
}

/// 月度汇总（毛值三列）：按月分组，income / expense（毛）/ refund 独立成列，
/// 毛值与净值并存展示（用户可同时看到毛支出与退款）。
///
/// 期间过滤（issue #411 / ADR-0057 决策 4）：可选 `from`/`to`（YYYY-MM-DD 含边界，
/// 字典序区间比较，各边独立）任一端存在即期间口径——「期间内按月分布」统一口径
///（分组按月不变、不切日粒度：年期间按月至多 12 行、季至多 3 行、月期间 1 行，
/// 期间内无流水的月份不成行），此时遗留 `year` 不参与；双端皆缺省回退遗留年份
/// 口径（已发布 API 只增不改）。
pub fn monthly_summary_rows(
    conn: &Connection,
    year: i64,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<Vec<MonthlySummary>> {
    // INDEXED BY 钉定表达式索引（issue #490）：GROUP BY month 的分组序与聚合列
    // 全在 idx_transactions_month_expr 内，钉定防 planner 统计边际摇摆退回
    // 临时 B-tree（不钉会退回 ~1s 量级）。
    let mut sql = format!(
        "SELECT substr(date,1,7) AS month, \
         SUM({income}) AS income, \
         SUM({expense_gross}) AS expense, \
         SUM({refund_gross}) AS refund \
         FROM transactions INDEXED BY idx_transactions_month_expr WHERE is_deleted=0",
        income = income_net_expr("transactions"),
        expense_gross = expense_gross_expr("transactions"),
        refund_gross = refund_gross_expr("transactions"),
    );
    let mut params: Vec<String> = Vec::new();
    push_period_conditions(&mut sql, &mut params, "date", from, to);
    if params.is_empty() {
        params.push(format!("{year}"));
        sql.push_str(" AND substr(date,1,4)=?1");
    }
    sql.push_str(" GROUP BY month ORDER BY month");
    query_all(conn, &sql, rusqlite::params_from_iter(params))
}

/// 分类聚合（净值）：`kind == "expense"` 用 `expense_net`（退款冲减支出），
/// 其余（income）用 `income_net`（收入+分红）；参与 kind 由矩阵导出。
/// 年份过滤（issue #376）：可选，传年份则按交易日期年份过滤（与月度汇总、
/// 商户排行同款 `substr(date,1,4)` 口径），退款以自身日期参与过滤；
/// 缺省（None）保持全时段口径不变（已发布 API 只增不改）。month/year 可叠加，
/// 占位符按条件追加顺序编号，与参数列表一一对齐。
///
/// 期间过滤（issue #411 / ADR-0057 决策 4）：可选 `from`/`to`（YYYY-MM-DD 含边界，
/// 字典序区间比较，各边独立）任一端存在即期间口径、遗留 month/year 不参与
///（退款同样以自身日期参与期间过滤）；双端皆缺省回退遗留 month/year 口径。
pub fn category_shares_rows(
    conn: &Connection,
    kind: &str,
    month: Option<&str>,
    year: Option<i64>,
    from: Option<&str>,
    to: Option<&str>,
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
    if from.is_some() || to.is_some() {
        // 期间口径（issue #411）：遗留 month/year 不参与。
        push_period_conditions(&mut sql, &mut params, "t.date", from, to);
    } else {
        if let Some(m) = month {
            params.push(m.to_string());
            sql.push_str(&format!(" AND substr(t.date,1,7)=?{}", params.len()));
        }
        if let Some(y) = year {
            params.push(y.to_string());
            sql.push_str(&format!(" AND substr(t.date,1,4)=?{}", params.len()));
        }
    }
    sql.push_str(" GROUP BY t.category_id ORDER BY net DESC");
    query_all(conn, &sql, rusqlite::params_from_iter(params))
}

/// 商户消费排行（净额，issue #192）：`expense_net`（毛支出 − 退款）按商户聚合、
/// 本位币口径（`amount_native_cents`），与核心交易域净值恒等式一致。
/// 无商户关联的交易不进排行；软删商户的历史引用照常统计（JOIN 不滤 is_deleted）。
///
/// 期间过滤（issue #411 / ADR-0057 决策 4）：可选 `from`/`to`（YYYY-MM-DD 含边界，
/// 字典序区间比较，各边独立）任一端存在即期间口径，遗留 `year` 不参与；
/// 双端皆缺省回退遗留年份口径（已发布 API 只增不改）。
///
/// 可选 topN 截断 + 全量合计（issue #588）：`top_n`（None = 全量，既有行为不变）在
/// ORDER BY（net DESC, name）之后应用——SQL 输出已排序，域内截断只取前 N 行，
/// 参与 kind 集合、排序与 JOIN 语义零改动；载荷同时携带本期全部商户净支出合计
/// （tooltip 占比分母，与截断无关），top_n 不影响合计。
pub fn merchant_shares_report(
    conn: &Connection,
    year: i64,
    from: Option<&str>,
    to: Option<&str>,
    top_n: Option<i64>,
) -> Result<MerchantSharesReport> {
    let kinds = contributing_kinds_sql(Measure::ExpenseNet);
    let mut sql = format!(
        "SELECT t.merchant_id, m.name, SUM({expr}) AS net \
         FROM transactions t JOIN merchants m ON m.id=t.merchant_id \
         WHERE t.kind IN ({kinds}) AND t.is_deleted=0",
        expr = expense_net_expr("t"),
    );
    let mut params: Vec<String> = Vec::new();
    push_period_conditions(&mut sql, &mut params, "t.date", from, to);
    if params.is_empty() {
        params.push(format!("{year}"));
        sql.push_str(" AND substr(t.date,1,4)=?1");
    }
    sql.push_str(" GROUP BY t.merchant_id ORDER BY net DESC, m.name");
    let rows: Vec<MerchantShare> = query_all(conn, &sql, rusqlite::params_from_iter(params))?;
    let total_cents = rows.iter().map(|r| r.amount_cents).sum();
    // 截断在排序之后应用（issue #588）：SQL 已按 net DESC, name 输出，取前 N 行即可；
    // 负值/零防御性归为空集（档位闭集二：5/10，正常调用方不会发出）。
    let rows = match top_n.map(|n| usize::try_from(n).unwrap_or(0)) {
        Some(limit) => rows.into_iter().take(limit).collect(),
        None => rows,
    };
    Ok(MerchantSharesReport { rows, total_cents })
}

/// 报表日期极值范围（issue #266 / #389）：对全部未删除交易各取一次最小/最大日期极值
/// （ISO 文本字典序即时间序，索引友好）；返回日期对 `{min_date, max_date}`（YYYY-MM-DD，空库双 `None`）。
pub fn query_report_date_range(conn: &Connection) -> Result<DateRange> {
    // 两个标量子查询（issue #490）：单条 MIN+MAX 聚合无法同时双向走索引
    // （SQLite 只对单聚合优化 MIN/MAX 极值查找），拆开后各自经列表序索引
    // 首尾定位，由全表/全索引扫描降为两次索引端点探测（904ms→~1ms）。
    let (min_date, max_date): (Option<String>, Option<String>) = conn.query_row(
        "SELECT (SELECT MIN(date) FROM transactions WHERE is_deleted=0), \
         (SELECT MAX(date) FROM transactions WHERE is_deleted=0)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(DateRange { min_date, max_date })
}
