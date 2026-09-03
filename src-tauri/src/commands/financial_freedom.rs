//! IPC 命令壳 · 财务自由度（Financial Freedom，#405 域目录化 ADR-0056）：
//! 财务自由度总览命令。
//!
//! 只做参数解包与连接锁管理，不含业务语义；自由度计算口径权威在
//! [`crate::investment::financial_freedom`]（投资域归位，#405 / ADR-0048 /
//! ADR-0056）。注册路径与前端调用保持不变。

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::investment as investment_domain;
use crate::investment::FinancialFreedomOverview;

/// 财务自由度总览：可投资资产 × 3% 安全提取率对年度预算总额的覆盖比例（只读）。
#[tauri::command]
pub fn financial_freedom(db: State<'_, DbState>) -> Result<FinancialFreedomOverview> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    investment_domain::query_financial_freedom(&conn)
}
