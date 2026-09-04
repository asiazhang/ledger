//! IPC 命令壳 · 报表（Report，#405 域目录化 ADR-0056）：月度汇总、商户排行、
//! 报表日期极值与分类份额四个只读命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；聚合读模型权威在
//! [`crate::reports`]（报表域归位，#405 / ADR-0056）。注册路径与前端调用保持不变；
//! 月度汇总/商户排行/分类份额的可选 `from`/`to` 期间参数（issue #411 / ADR-0057）
//! 只增不改：任一存在即期间口径，双缺省回退遗留参数口径。
//!
//! 全部命令 async 化（形状乙，spec #498 / #502）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行（读路径锁内执行），不占用
//! 界面事件循环线程，对用户外部行为不变。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::reports as reports_domain;
use crate::reports::{CategoryShare, DateRange, MerchantShare, MonthlySummary};

#[tauri::command]
pub async fn monthly_summary(
    db: State<'_, DbState>,
    year: i64,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<MonthlySummary>> {
    let conn = db.conn.clone();
    run_db("monthly_summary", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        reports_domain::monthly_summary_rows(&conn, year, from.as_deref(), to.as_deref())
    })
    .await
}

#[tauri::command]
pub async fn merchant_shares(
    db: State<'_, DbState>,
    year: i64,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<MerchantShare>> {
    let conn = db.conn.clone();
    run_db("merchant_shares", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        reports_domain::merchant_shares_rows(&conn, year, from.as_deref(), to.as_deref())
    })
    .await
}

/// 报表日期极值范围（issue #266 / #389）：只读命令，返回未删交易日期极值对。
#[tauri::command]
pub async fn report_date_range(db: State<'_, DbState>) -> Result<DateRange> {
    let conn = db.conn.clone();
    run_db("report_date_range", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        reports_domain::query_report_date_range(&conn)
    })
    .await
}

/// 分类份额（issue #376 年份联动；#411 期间化）：可选 month/year 与 from/to 只增不改——
/// from/to 任一存在即期间口径、month/year 不参与；双端皆缺省回退遗留 month/year 口径。
#[tauri::command]
pub async fn category_shares(
    db: State<'_, DbState>,
    kind: String,
    month: Option<String>,
    year: Option<i64>,
    from: Option<String>,
    to: Option<String>,
) -> Result<Vec<CategoryShare>> {
    let conn = db.conn.clone();
    run_db("category_shares", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        reports_domain::category_shares_rows(
            &conn,
            &kind,
            month.as_deref(),
            year,
            from.as_deref(),
            to.as_deref(),
        )
    })
    .await
}
