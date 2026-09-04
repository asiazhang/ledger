//! IPC 命令壳 · 定时计划（ScheduledTransaction）。
//!
//! 全部触碰 DB 的命令 async 化（形状乙，spec #498 / #502）：DB 调用经连接层
//! 统一 helper [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件
//! 循环线程；写路径仍在连接层统一写入口内置脏（ADR-0032 语义零改动）。
//! `set_auto_execution_enabled` 是设备级运行时镜像推送（纯内存，不触 DB），
//! 保持同步形态。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::scheduled_transactions::*;

#[tauri::command]
pub async fn create_scheduled_transaction(
    db: State<'_, DbState>,
    input: CreateScheduledInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032，#246 审计补齐）：计划与期次属账本数据，成功即置脏。
    let conn = db.conn.clone();
    run_db("create_scheduled_transaction", move || {
        crate::db::write(&conn, |conn| {
            crate::scheduled_transactions::create_plan(conn, input)
        })
    })
    .await
}

#[tauri::command]
pub async fn list_scheduled_transactions(
    db: State<'_, DbState>,
) -> Result<Vec<ScheduledTransactionWithExt>> {
    let conn = db.conn.clone();
    run_db("list_scheduled_transactions", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        crate::scheduled_transactions::list_plans(&conn)
    })
    .await
}

#[tauri::command]
pub async fn get_scheduled_transaction_detail(
    db: State<'_, DbState>,
    id: String,
) -> Result<ScheduledTransactionDetail> {
    let conn = db.conn.clone();
    run_db("get_scheduled_transaction_detail", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        crate::scheduled_transactions::get_plan_detail(&conn, &id)
    })
    .await
}

#[tauri::command]
pub async fn update_scheduled_transaction_status(
    db: State<'_, DbState>,
    input: UpdateStatusInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032，#246 审计补齐）：状态变更成功即置脏。
    let conn = db.conn.clone();
    run_db("update_scheduled_transaction_status", move || {
        crate::db::write(&conn, |conn| {
            crate::scheduled_transactions::update_plan_status(conn, &input.id, input.new_status)
        })
    })
    .await
}

/// 编辑订阅计划的非金额字段（issue #162，ADR-0023 决策三）：
/// 请求携带金额字段时后端显式拒绝并提示「改价 = 取消旧计划 + 新建」。
#[tauri::command]
pub async fn update_scheduled_subscription(
    db: State<'_, DbState>,
    input: UpdateSubscriptionInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032，#246 审计补齐）：订阅编辑成功即置脏。
    let conn = db.conn.clone();
    run_db("update_scheduled_subscription", move || {
        crate::db::write(&conn, |conn| {
            crate::scheduled_transactions::update_subscription(conn, input)
        })
    })
    .await
}

#[tauri::command]
pub async fn execute_scheduled_occurrence(
    db: State<'_, DbState>,
    input: ExecuteOccurrenceInput,
) -> Result<String> {
    // 期次执行落交易行（Writer 接缝交易增），经写入口置脏（ADR-0032）。
    let conn = db.conn.clone();
    run_db("execute_scheduled_occurrence", move || {
        crate::db::write(&conn, |conn| {
            crate::scheduled_transactions::execute_occurrence(conn, &input.occurrence_id)
        })
    })
    .await
}

#[tauri::command]
pub async fn expand_scheduled_occurrences(
    db: State<'_, DbState>,
    id: String,
) -> Result<Vec<String>> {
    // 连接层统一写入口（ADR-0032，#246 审计补齐）：期次回填写入成功即置脏。
    let conn = db.conn.clone();
    run_db("expand_scheduled_occurrences", move || {
        crate::db::write(&conn, |conn| {
            crate::scheduled_transactions::expand_occurrences(conn, &id)
        })
    })
    .await
}

/// 订阅花费总览（issue #160/#161，ADR-0023 双口径）：只读聚合，返回逐订阅行 +
/// 本月/本年实际花费 + 过去 12 个月逐月序列 + 折算月/年推算成本（本位币）。
#[tauri::command]
pub async fn subscription_spend_overview(
    db: State<'_, DbState>,
) -> Result<SubscriptionSpendOverview> {
    let conn = db.conn.clone();
    run_db("subscription_spend_overview", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        crate::scheduled_transactions::query_subscription_spend(
            &conn,
            chrono::Local::now().date_naive(),
        )
    })
    .await
}

/// 推送设备级「自动执行」开关到后端运行时镜像（issue #307 / ADR-0042）：
/// 开关真源在前端 localStorage 设备偏好，应用启动与变更时经本命令推送
/// （备份目录镜像推送先例）；后端镜像默认关，调度线程每轮从镜像读出后注入
/// 追补入口。刻意不入 `app_settings`——该表随 Backup/Restore 迁移，
/// 表达不了「这台执行、那台不执行」的设备级语义，也会把自动化意外迁移到新设备。
#[tauri::command]
pub fn set_auto_execution_enabled(enabled: bool) -> Result<()> {
    crate::scheduled_transactions::auto_run::set_enabled(enabled);
    Ok(())
}
