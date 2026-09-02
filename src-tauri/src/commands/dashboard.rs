//! IPC 命令壳 · 仪表盘（Dashboard，#405 域目录化 ADR-0056）：首页净资产总览命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；净资产跨币种折算聚合权威在
//! [`crate::dashboard`]（仪表盘域归位，#405 / ADR-0056）。注册路径与前端调用保持不变。

use tauri::State;

use crate::dashboard as dashboard_domain;
use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::models::DashboardOverview;

/// 首页净资产总览：本位币净资产及其两个组成。
#[tauri::command]
pub fn dashboard_overview(db: State<'_, DbState>) -> Result<DashboardOverview> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    dashboard_domain::query_dashboard_overview(&conn)
}
