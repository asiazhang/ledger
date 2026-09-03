//! IPC 命令壳 · 报表（Report，#405 域目录化 ADR-0056）：月度汇总、商户排行、
//! 报表日期极值与分类份额四个只读命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；聚合读模型权威在
//! [`crate::reports`]（报表域归位，#405 / ADR-0056）。注册路径与前端调用保持不变；
//! 月度汇总/商户排行/分类份额的可选 `from`/`to` 期间参数（issue #411 / ADR-0057）
//! 只增不改：任一存在即期间口径，双缺省回退遗留参数口径。

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::reports as reports_domain;
use crate::reports::{CategoryShare, DateRange, MerchantShare, MonthlySummary};

#[tauri::command]
pub fn monthly_summary(
    db: State<'_, DbState>,
    year: i64,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<MonthlySummary>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    reports_domain::monthly_summary_rows(&conn, year, from.as_deref(), to.as_deref())
}

#[tauri::command]
pub fn merchant_shares(
    db: State<'_, DbState>,
    year: i64,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<MerchantShare>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    reports_domain::merchant_shares_rows(&conn, year, from.as_deref(), to.as_deref())
}

/// 报表日期极值范围（issue #266 / #389）：只读命令，返回未删交易日期极值对。
#[tauri::command]
pub fn report_date_range(db: State<'_, DbState>) -> Result<DateRange> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    reports_domain::query_report_date_range(&conn)
}

/// 分类份额（issue #376 年份联动；#411 期间化）：可选 month/year 与 from/to 只增不改——
/// from/to 任一存在即期间口径、month/year 不参与；双端皆缺省回退遗留 month/year 口径。
#[tauri::command]
pub fn category_shares(
    db: State<'_, DbState>,
    kind: String,
    month: Option<String>,
    year: Option<i64>,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<CategoryShare>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    reports_domain::category_shares_rows(
        &conn,
        &kind,
        month.as_deref(),
        year,
        from.as_deref(),
        to.as_deref(),
    )
}
