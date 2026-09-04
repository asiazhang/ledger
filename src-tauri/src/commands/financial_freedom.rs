//! IPC 命令壳 · 财务自由度（Financial Freedom，#405 域目录化 ADR-0056）：
//! 财务自由度总览命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；自由度计算口径权威在
//! [`crate::investment::financial_freedom`]（投资域归位，#405 / ADR-0048 /
//! ADR-0056）。注册路径与前端调用保持不变。
//!
//! 命令 async 化（形状乙，spec #498 / #502）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行（读路径锁内执行），不占用
//! 界面事件循环线程，对用户外部行为不变。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::investment as investment_domain;
use crate::investment::FinancialFreedomOverview;

/// 财务自由度总览：可投资资产 × 3% 安全提取率对年度预算总额的覆盖比例（只读）。
#[tauri::command]
pub async fn financial_freedom(db: State<'_, DbState>) -> Result<FinancialFreedomOverview> {
    let conn = db.conn.clone();
    run_db("financial_freedom", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        investment_domain::query_financial_freedom(&conn)
    })
    .await
}
