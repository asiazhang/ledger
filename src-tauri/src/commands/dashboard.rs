//! IPC 命令壳 · 仪表盘（Dashboard，#405 域目录化 ADR-0056）：首页净资产总览命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；净资产跨币种折算聚合权威在
//! [`crate::dashboard`]（仪表盘域归位，#405 / ADR-0056）。注册路径与前端调用保持不变。
//!
//! 命令 async 化（形状乙，spec #498 / #501）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::dashboard as dashboard_domain;
use crate::dashboard::DashboardOverview;
use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};

/// 首页净资产总览：本位币净资产及其三个组成。
#[tauri::command]
pub async fn dashboard_overview(db: State<'_, DbState>) -> Result<DashboardOverview> {
    let conn = db.conn.clone();
    run_db("dashboard_overview", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        dashboard_domain::query_dashboard_overview(&conn)
    })
    .await
}
