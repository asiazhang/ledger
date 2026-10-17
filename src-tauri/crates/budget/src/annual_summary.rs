//! 预算年度汇总（issue #1948）：年度预算总额与本年已支的只读聚合。
//!
//! 口径（词汇表：预算域 `AnnualBudgetTotal` / `BudgetAnnualSummary`）：
//! - **年度预算总额** = Σ 月预算 × 12 + Σ 年预算（全部未删除预算）；无窗口、不滚动，
//!   月预算按节奏年化。跨域单点：投资域财务自由度（ADR-0048）的分母直接调用
//!   [`annual_budget_total_cents`]，不再自持第二份表达式。
//! - **本年已支** = 注入 `today` 所在自然年、**预算覆盖分类**的 `ExpenseNet` 合计：
//!   一笔支出只要其分类自身或其父分类设了预算就计入，且按交易逐笔记一次——不是
//!   按预算逐行相加，所以父预算与子预算并存时同一笔支出不会重复计入（ADR-0052）。
//! - **未设预算的分类支出不进汇总**：与年度预算总额「不回退实际支出」同一边界
//!   （ADR-0048 决策口径），否则已支会大于预算却没有对应预算行。
//! - **只服务当前自然年**：预算无历史快照（ADR-0029 永久滚动），回看往年只能拿
//!   今天的金额推算，属假历史；历年视图须先有预算金额变更历史，见 issue #1948。
//!
//! 读快照一致性（AGENTS.md 多语句读闭包纪律 / #1699 同款）：预算额与支出跨表
//! 两条语句，整体收进同一读事务（嵌套感知）——分子分母恒同时点。

use chrono::{Datelike, NaiveDate};
use rusqlite::Connection;

use super::model::BudgetAnnualSummary;
use ledger_infra::db::tx_scope::ensure_transaction;
use ledger_infra::error::Result;
use ledger_transaction::amount::{Measure, contributing_kinds_sql, expense_net_expr};

/// conn 级聚合：年度预算总额（默认币种单币种口径，分）= Σ 月预算 × 12 + Σ 年预算。
///
/// 跨域单点（ADR-0048）：投资域财务自由度的分母、预算页年度汇总卡同名口径都经
/// 本函数取数，口径表达式不复制（ADR-0131 决策 4）。
pub fn annual_budget_total_cents(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(CASE WHEN period='monthly' THEN amount_cents * 12 \
                              WHEN period='yearly' THEN amount_cents END), 0) \
         FROM budgets WHERE is_deleted=0",
        [],
        |r| r.get(0),
    )?)
}

/// conn 级聚合：预算年度汇总（年度预算总额 + 本年已支 + 超标判定），只读实时计算。
///
/// 窗口为注入 `today` 所在自然年（与预算进度命令同源注入，测试注入固定值）。
pub fn budget_annual_summary(conn: &Connection, today: NaiveDate) -> Result<BudgetAnnualSummary> {
    ensure_transaction(conn, || {
        let annual_budget_total_cents = annual_budget_total_cents(conn)?;

        let kinds = contributing_kinds_sql(Measure::ExpenseNet);
        let year = format!("{:04}", today.year());
        // 覆盖判定：交易分类自身设了预算，或其父分类设了预算——两条分支只决定
        // 「这一笔算不算」，SUM 按交易逐行累计，故父子预算并存也只计一次。
        let sql = format!(
            "SELECT COALESCE(SUM({expense_net}),0) \
             FROM transactions t \
             JOIN categories tc ON tc.id=t.category_id \
             WHERE t.kind IN ({kinds}) AND t.is_deleted=0 AND substr(t.date,1,4)=?1 \
               AND (EXISTS (SELECT 1 FROM budgets b WHERE b.category_id=tc.id AND b.is_deleted=0) \
                 OR EXISTS (SELECT 1 FROM budgets bp WHERE bp.category_id=tc.parent_id AND bp.is_deleted=0))",
            expense_net = expense_net_expr("t"),
        );
        let spent_cents: i64 = conn.query_row(&sql, rusqlite::params![year], |r| r.get(0))?;

        Ok(BudgetAnnualSummary {
            annual_budget_total_cents,
            spent_cents,
            over_budget: spent_cents > annual_budget_total_cents,
        })
    })
}
