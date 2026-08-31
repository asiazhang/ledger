//! 预算（issue #91 创建，issue #58 迁移 Amount 口径，issue #182 滚动窗口，issue #184 编辑金额）：
//! 命令外壳 + 内嵌测试外迁。
//!
//! 目录组织：
//! - `tests`：针对模块接口的测试（期望值由度量矩阵逐行求和得出，不复制生产 SQL）。
//!
//! 金额口径由 `transaction::amount` 的 kind→度量矩阵单一真源驱动：
//! 预算 spent = `expense_net`（毛支出 − 退款），与报表分类净值口径一致。
//!
//! 命令层为薄壳（经连接层统一写入口 `db.write` 调核心函数，ADR-0032：写成功即置脏，
//! issue #245 补上此前的置脏缺口），核心函数吃 `&Connection` 可直接单测。
//! 对外暴露的命令经 `commands/mod.rs` 的 `pub use budget::*` 重导出，
//! 注册路径与前端调用零改动。

#[cfg(test)]
mod tests;

use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, OptionalExtension};
use tauri::State;

use crate::db::query::query_all;
use crate::db::{DbState, device_id, new_uuid, now_iso};
use crate::error::{AppError, Result};
use crate::models::{Budget, BudgetInput, BudgetPeriod, BudgetProgress, BudgetUpdateInput};
use crate::transaction::amount::{Measure, contributing_kinds_sql, expense_net_expr};

#[tauri::command]
pub fn list_budgets(db: State<'_, DbState>) -> Result<Vec<Budget>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    query_all(
        &conn,
        "SELECT id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted \
         FROM budgets WHERE is_deleted=0 ORDER BY created_at",
        [],
    )
}

/// 金额与分类校验（issue #184 起 create 与 update 共用）：金额必须为正数；
/// 只能挂支出分类（收入分类与不存在的分类均拒绝）。
fn validate_amount_and_category(
    conn: &Connection,
    category_id: &str,
    amount_cents: i64,
) -> Result<()> {
    if amount_cents <= 0 {
        return Err(AppError::coded(
            "budget.amount-positive",
            "预算金额必须为正数",
        ));
    }
    let kind: Option<String> = conn
        .query_row(
            "SELECT kind FROM categories WHERE id=?1 AND is_deleted=0",
            rusqlite::params![category_id],
            |r| r.get(0),
        )
        .optional()?;
    match kind.as_deref() {
        None => Err(AppError::codedp_not_found(
            "budget.category-not-found",
            format!("分类不存在: {category_id}"),
            &[category_id],
        )),
        Some("expense") => Ok(()),
        Some(other) => Err(AppError::codedp(
            "budget.category-not-expense",
            format!("预算只能设置在支出分类上（该分类为{other}分类）"),
            &[other],
        )),
    }
}

/// 预算写入校验与落库核心（issue #183）：命令外壳与测试共用。三条底线——
/// 1) 金额必须为正数；2) 只能挂支出分类（金额与分类校验经 [`validate_amount_and_category`]）；
/// 3) 同「分类 + 周期」已有未删除预算时明确拒绝（中文提示并引导编辑），不静默覆盖。
pub fn create_budget_internal(conn: &Connection, input: &BudgetInput) -> Result<String> {
    validate_amount_and_category(conn, &input.category_id, input.amount_cents)?;
    let period = input.period.unwrap_or(BudgetPeriod::Monthly);
    let duplicate: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM budgets WHERE category_id=?1 AND period=?2 AND is_deleted=0 LIMIT 1",
            rusqlite::params![input.category_id, period.to_string()],
            |r| r.get(0),
        )
        .optional()?;
    if duplicate.is_some() {
        return Err(match period {
            BudgetPeriod::Monthly => AppError::coded(
                "budget.duplicate-monthly",
                "该分类已存在按月预算，可编辑该预算的金额",
            ),
            BudgetPeriod::Yearly => AppError::coded(
                "budget.duplicate-yearly",
                "该分类已存在按年预算，可编辑该预算的金额",
            ),
        });
    }
    let id = new_uuid();
    let now = now_iso();
    conn.execute(
        "INSERT INTO budgets (id,category_id,period,amount_cents,start_date,created_at,updated_at,version,device_id,is_deleted) \
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,0)",
        rusqlite::params![
            id,
            input.category_id,
            period.to_string(),
            input.amount_cents,
            input.start_date,
            now,
            now,
            1,
            device_id()
        ],
    )?;
    Ok(id)
}

#[tauri::command]
pub fn create_budget(db: State<'_, DbState>, input: BudgetInput) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：预算写入也是账本数据变化，成功即置脏
    // （issue #245 行为修复：此前预算写路径完全不在置脏范围）。
    db.write(|conn| create_budget_internal(conn, &input))
}

/// 预算编辑核心（issue #184）：仅修改金额，沿用软删除同一套
/// updated_at/version/device_id 更新机制；金额与支出分类校验复用创建侧逻辑。
/// 分类/周期不可改（改法为删旧建新）。
pub fn update_budget_internal(conn: &Connection, id: &str, amount_cents: i64) -> Result<()> {
    let category_id: String = conn
        .query_row(
            "SELECT category_id FROM budgets WHERE id=?1 AND is_deleted=0",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| {
            AppError::codedp_not_found("budget.not-found", format!("预算不存在: {id}"), &[id])
        })?;
    validate_amount_and_category(conn, &category_id, amount_cents)?;
    conn.execute(
        "UPDATE budgets SET amount_cents=?2, updated_at=?3, version=version+1, device_id=?4 WHERE id=?1",
        rusqlite::params![id, amount_cents, now_iso(), device_id()],
    )?;
    Ok(())
}

#[tauri::command]
pub fn update_budget(db: State<'_, DbState>, id: String, input: BudgetUpdateInput) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：同创建，成功即置脏（issue #245 行为修复）。
    db.write(|conn| update_budget_internal(conn, &id, input.amount_cents))
}

/// 预算删除核心：软删除 + 审计字段更新，与创建/编辑共用同一套机制
/// （提取与 `create_budget_internal` / `update_budget_internal` 对齐，测试与 e2e 同款）。
pub fn delete_budget_internal(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE budgets SET is_deleted=1, updated_at=?2, version=version+1, device_id=?3 WHERE id=?1",
        rusqlite::params![id, now_iso(), device_id()],
    )?;
    Ok(())
}

#[tauri::command]
pub fn delete_budget(db: State<'_, DbState>, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：同创建，成功即置脏（issue #245 行为修复）。
    db.write(|conn| delete_budget_internal(conn, &id))
}

/// 预算进度核心（issue #182 永久滚动预算）：spent = `expense_net`（毛支出 − 退款，
/// 退款冲减支出），与报表分类净值同口径；参与 kind 由矩阵导出（不含 buy/sell 等投资类）。
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

#[tauri::command]
pub fn budget_progress(db: State<'_, DbState>) -> Result<Vec<BudgetProgress>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    budget_progress_rows(&conn, chrono::Local::now().date_naive())
}
