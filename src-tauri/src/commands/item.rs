//! IPC 命令壳 · 物品（Item）（issue #115 / #117 / #118 / #120 / #122 / spec #113 /
//! ADR-0014）：创建、列出、编辑、处置、软删除物品与「在用物品每天成本合计」聚合
//! 七个命令。
//!
//! 只做参数解包、事务壳与信号发射，不含业务语义；行为权威在
//! [`crate::item::domain`]（阶段 1 域目录化，#397 / ADR-0056）。
//!
//! 信号约定：物品是独立领域（非参考数据，ADR-0014），复用 `ledger:changed`
//! 同名事件——物品 store 与消费界面订阅后自动重拉。发不发、发哪个由映射
//! 单点判定（ADR-0044 决策 8），notify 只是发射钩子。
//!
//! 置脏触发已收口连接层统一写入口（`db::write`，ADR-0032）：写路径对备份域
//! 零感知，置脏/到期检查由写入口闭包在提交点单点执行。

use tauri::State;

use crate::db::DbState;
use crate::error::{AppError, Result};
use crate::item::domain;
use crate::item::{ItemDailyCost, ItemDailyTotal, ItemDisposeInput, ItemInput, ItemWithDailyCost};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

#[tauri::command]
pub fn list_items(db: State<'_, DbState>) -> Result<Vec<ItemWithDailyCost>> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    domain::list_items(&conn)
}

/// 计算单件物品的每天使用成本（issue #121，只读命令不发失效信号）：
/// `reference_date` 缺省/为 null 时沿用列表口径（在用 → 今天；已处置 → 处置日）。
#[tauri::command]
pub fn calculate_item_cost(
    db: State<'_, DbState>,
    id: String,
    reference_date: Option<String>,
) -> Result<ItemDailyCost> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    domain::calculate_item_cost(&conn, &id, reference_date.as_deref())
}

/// 全部在用物品每天成本合计（issue #122，只读聚合不发失效信号），
/// 供 dashboard 汇总卡展示（默认币种）。
#[tauri::command]
pub fn item_daily_total(db: State<'_, DbState>) -> Result<ItemDailyTotal> {
    let conn = db.conn.lock().map_err(|e| AppError::Db(e.to_string()))?;
    domain::item_daily_total(&conn)
}

#[tauri::command]
pub fn create_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    input: ItemInput,
) -> Result<String> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        domain::create_item(conn, input, &mut || {
            emit_for(&app, WriteOp::CreateItem, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn update_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        domain::update_item(conn, &id, input, &mut || {
            emit_for(&app, WriteOp::UpdateItem, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn dispose_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemDisposeInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        domain::dispose_item(conn, &id, input, &mut || {
            emit_for(&app, WriteOp::DisposeItem, WriteEvidence::None);
        })
    })
}

#[tauri::command]
pub fn delete_item(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    db.write(|conn| {
        domain::delete_item(conn, &id, &mut || {
            emit_for(&app, WriteOp::DeleteItem, WriteEvidence::None);
        })
    })
}
