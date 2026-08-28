use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::scheduled_transactions::*;

#[tauri::command]
pub fn create_scheduled_transaction(
    db: State<'_, DbState>,
    input: CreateScheduledInput,
) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::scheduled_transactions::create_plan(&conn, input)
}

#[tauri::command]
pub fn list_scheduled_transactions(
    db: State<'_, DbState>,
) -> Result<Vec<ScheduledTransactionWithExt>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::scheduled_transactions::list_plans(&conn)
}

#[tauri::command]
pub fn get_scheduled_transaction_detail(
    db: State<'_, DbState>,
    id: String,
) -> Result<ScheduledTransactionDetail> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::scheduled_transactions::get_plan_detail(&conn, &id)
}

#[tauri::command]
pub fn update_scheduled_transaction_status(
    db: State<'_, DbState>,
    input: UpdateStatusInput,
) -> Result<()> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::scheduled_transactions::update_plan_status(&conn, &input.id, input.new_status)
}

#[tauri::command]
pub fn execute_scheduled_occurrence(
    db: State<'_, DbState>,
    input: ExecuteOccurrenceInput,
) -> Result<String> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::scheduled_transactions::execute_occurrence(&conn, &input.occurrence_id)
}

#[tauri::command]
pub fn expand_scheduled_occurrences(db: State<'_, DbState>, id: String) -> Result<Vec<String>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::scheduled_transactions::expand_occurrences(&conn, &id)
}

/// 订阅花费总览（issue #160/#161，ADR-0023 双口径）：只读聚合，返回逐订阅行 +
/// 本月/本年实际花费 + 过去 12 个月逐月序列 + 折算月/年推算成本（本位币）。
#[tauri::command]
pub fn subscription_spend_overview(db: State<'_, DbState>) -> Result<SubscriptionSpendOverview> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    crate::scheduled_transactions::query_subscription_spend(
        &conn,
        chrono::Local::now().date_naive(),
    )
}
