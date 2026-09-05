//! IPC 命令壳 · 物品（Item）（issue #115 / #117 / #118 / #120 / #122 / spec #113 /
//! ADR-0014）：创建、列出、编辑、处置、软删除物品与「在用物品每天成本合计」聚合
//! 七个命令。
//!
//! 只做参数解包、事务壳与信号发射，不含业务语义；行为权威在
//! [`crate::item::domain`]（阶段 1 域目录化，#397 / ADR-0056）。
//!
//! 信号约定：物品是独立领域（非参考数据，ADR-0014），复用 `ledger:changed`
//! 同名事件——物品 store 与消费界面订阅后自动重拉。信号经统一写入口按写操作
//! 身份发射（ADR-0073）；域内 notify 参数保留为 BDD 计数注入点（ADR-0044
//! 决策 8），生产壳层传空回调。
//!
//! 写命令经壳层统一写入口 [`crate::write_entry::write_entry`]（ADR-0073）：
//! 仪式（锁、事务、置脏、信号）内化单点；读命令经 `run_db`（形状乙）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::item::domain;
use crate::item::{ItemDailyCost, ItemDailyTotal, ItemDisposeInput, ItemInput, ItemWithDailyCost};
use crate::signals::WriteOp;
use crate::write_entry::{Outcome, write_entry};

#[tauri::command]
pub async fn list_items(db: State<'_, DbState>) -> Result<Vec<ItemWithDailyCost>> {
    let conn = db.conn.clone();
    run_db("list_items", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        domain::list_items(&conn)
    })
    .await
}

/// 计算单件物品的每天使用成本（issue #121，只读命令不发失效信号）：
/// `reference_date` 缺省/为 null 时沿用列表口径（在用 → 今天；已处置 → 处置日）。
#[tauri::command]
pub async fn calculate_item_cost(
    db: State<'_, DbState>,
    id: String,
    reference_date: Option<String>,
) -> Result<ItemDailyCost> {
    let conn = db.conn.clone();
    run_db("calculate_item_cost", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        domain::calculate_item_cost(&conn, &id, reference_date.as_deref())
    })
    .await
}

/// 全部在用物品每天成本合计（issue #122，只读聚合不发失效信号），
/// 供 dashboard 汇总卡展示（默认币种）。
#[tauri::command]
pub async fn item_daily_total(db: State<'_, DbState>) -> Result<ItemDailyTotal> {
    let conn = db.conn.clone();
    run_db("item_daily_total", move || {
        let conn = conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
        domain::item_daily_total(&conn)
    })
    .await
}

#[tauri::command]
pub async fn create_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ItemInput,
) -> Result<String> {
    let conn = db.conn.clone();
    write_entry(
        "create_item",
        conn,
        Some(&app),
        WriteOp::CreateItem,
        // notify 是 BDD 计数注入点（ADR-0044 决策 8）；信号已由写入口按身份
        // 在提交成功后发射，生产壳层传空回调。
        move |conn| domain::create_item(conn, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn update_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "update_item",
        conn,
        Some(&app),
        WriteOp::UpdateItem,
        move |conn| domain::update_item(conn, &id, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn dispose_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemDisposeInput,
) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "dispose_item",
        conn,
        Some(&app),
        WriteOp::DisposeItem,
        move |conn| domain::dispose_item(conn, &id, input, &mut || {}).map(Outcome::Silent),
    )
    .await
}

#[tauri::command]
pub async fn delete_item(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    let conn = db.conn.clone();
    write_entry(
        "delete_item",
        conn,
        Some(&app),
        WriteOp::DeleteItem,
        move |conn| domain::delete_item(conn, &id, &mut || {}).map(Outcome::Silent),
    )
    .await
}
