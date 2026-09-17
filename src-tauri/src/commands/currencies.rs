//! IPC 命令壳 · 币种（Currency）。
//!
//! 只负责参数解包与命令注册；币种为种子权威参考数据，无写命令、无失效信号，
//! 清单查询实现位于 [`crate::currencies`]。注册路径与前端调用保持不变。
//!
//! 全部命令 async 化（形状乙，spec #498 / #501）：DB 调用经连接层统一 helper
//! [`ledger_infra::db::run_db`] 进 tauri 阻塞线程池执行，不占用界面事件循环线程。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::State;

use crate::shell_support::read_entry::read_entry;
use ledger_currencies as currency_domain;
use ledger_currencies::Currency;
use ledger_infra::db::DbState;
use ledger_infra::error::Result;

/// 币种清单：全部种子币种按 `code` 排序。
#[tauri::command]
pub async fn list_currencies(db: State<'_, DbState>) -> Result<Vec<Currency>> {
    let conn = db.read_handle();
    read_entry("list_currencies", conn, move |conn| {
        currency_domain::list_currencies(conn)
    })
    .await
}
