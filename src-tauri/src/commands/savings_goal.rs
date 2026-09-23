//! IPC 命令壳 · 储蓄目标（SavingsGoal）（spec #1750 / issue #1751 / ADR-0133）：
//! 创建目标（同一事务自动建专属账户）与蓄水进度读命令（已存 / 还差 / 达成态）。
//!
//! 只做参数解包与统一读写入口一行调用；目标行为权威在 [`ledger_savings_goal`]
//!（ADR-0056 分层）。
//!
//! 信号约定：储蓄目标是独立领域（ADR-0133），复用 `ledger:changed` 同名事件——
//! 目标 store 订阅后自动重拉；同事务建出的专属账户随参考表重拉对账户列表与各
//! 下拉可见。信号经统一写入口按写操作身份发射（ADR-0073）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
//（tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime, State};

use crate::shell_support::read_entry::read_entry;
use crate::shell_support::write_entry::{Outcome, write_entry};
use ledger_infra::db::DbState;
use ledger_infra::error::Result;
use ledger_infra::signals::WriteOp;
use ledger_savings_goal::{self as savings_goal_domain, SavingsGoalInput, SavingsGoalProgress};

/// 蓄水进度读命令：目标清单 × 逐目标进度（已存 = 专属账户余额、读余额缓存，
/// 还差与达成态读时派生）。返回按创建先后排序。
#[tauri::command]
pub async fn savings_goal_progress(db: State<'_, DbState>) -> Result<Vec<SavingsGoalProgress>> {
    let conn = db.read_handle();
    read_entry("savings_goal_progress", conn, move |conn| {
        savings_goal_domain::list_savings_goal_progress(conn)
    })
    .await
}

/// 创建目标（名称、目标金额、可选截止日期）：域内同一事务经账户域公开写入口
/// 自动建专属账户（`other` 类型、1:1 绑定），返回目标 id。
#[tauri::command]
pub async fn create_savings_goal<R: Runtime>(
    db: State<'_, DbState>,
    app: AppHandle<R>,
    input: SavingsGoalInput,
) -> Result<String> {
    let conn = db.write_handle();
    // 域内自持事务保证目标行 + 专属账户两表原子；域事务在闭包返回前已提交，
    // 写入口的 is_autocommit 复核与置脏照常生效（ADR-0033 嵌套感知）。
    write_entry(
        "create_savings_goal",
        conn,
        Some(&app),
        WriteOp::CreateSavingsGoal,
        move |conn| savings_goal_domain::create_savings_goal(conn, &input).map(Outcome::Silent),
    )
    .await
}
