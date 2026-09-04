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
//!
//! 全部命令 async 化（形状乙，spec #498 / #502）：DB 调用经连接层统一 helper
//! [`crate::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程；
//! 写路径仍在连接层统一写入口内置脏（ADR-0032 语义零改动）。notify 回调里的
//! 信号发射经 `post_emit_with` 投递主线程队尾（ADR-0044），从阻塞线程调用安全，
//! 对用户外部行为不变。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::db::{DbState, run_db};
use crate::error::{AppError, Result};
use crate::item::domain;
use crate::item::{ItemDailyCost, ItemDailyTotal, ItemDisposeInput, ItemInput, ItemWithDailyCost};
use crate::signals::{WriteEvidence, WriteOp, emit_for};

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
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("create_item", move || {
        crate::db::write(&conn, |conn| {
            domain::create_item(conn, input, &mut || {
                emit_for(&app, WriteOp::CreateItem, WriteEvidence::None);
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn update_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("update_item", move || {
        crate::db::write(&conn, |conn| {
            domain::update_item(conn, &id, input, &mut || {
                emit_for(&app, WriteOp::UpdateItem, WriteEvidence::None);
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn dispose_item(
    db: State<'_, DbState>,
    app: tauri::AppHandle,
    id: String,
    input: ItemDisposeInput,
) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("dispose_item", move || {
        crate::db::write(&conn, |conn| {
            domain::dispose_item(conn, &id, input, &mut || {
                emit_for(&app, WriteOp::DisposeItem, WriteEvidence::None);
            })
        })
    })
    .await
}

#[tauri::command]
pub async fn delete_item(db: State<'_, DbState>, app: tauri::AppHandle, id: String) -> Result<()> {
    // 连接层统一写入口（ADR-0032）：成功即置脏，写路径对备份域零感知。
    let conn = db.conn.clone();
    run_db("delete_item", move || {
        crate::db::write(&conn, |conn| {
            domain::delete_item(conn, &id, &mut || {
                emit_for(&app, WriteOp::DeleteItem, WriteEvidence::None);
            })
        })
    })
    .await
}
