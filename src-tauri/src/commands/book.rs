//! 账本（Book）注册表命令壳层（issue #833 / ADR-0089 决策 2 后果条款）。
//!
//! 只做参数解包与引导内核调用：登记变更的业务规则（同目录不得重复登记、
//! 活动账本不可移除、目标可用性校验、首次登记落新格式）在引导内核
//! [`crate::db::book_registry`]，写入时机契约预检（注册表可读 + 无推迟搬迁
//! 窗口）在 [`data_location::mutable_registry`]，本文件不含领域规则。
//!
//! 切换语义（ADR-0089 决策 3）：写活动指针落盘后返回目标账本，原位重引导由
//! 前端在命令成功后复用既有 `restart_app`（ADR-0080）完成——重载 WebView 后
//! 引导序列按注册表进入目标账本（明文库直进主界面、密文库落解锁屏）。
//!
//! 全部命令 async 化（形状乙，spec #498/#503 先例）：注册表文件读写与目录
//! 创建是阻塞文件 IO，经连接层统一 helper [`crate::db::run_db`] 进 tauri
//! 阻塞线程池执行，不占用界面事件循环线程（`commands::data_location` 同型）。
//
// 豁免（ADR-0060）：tauri 宏为 async 命令生成的 `_check = unreachable!()`
// （tauri-macros wrapper.rs，宏不透传逐点 allow，无法在源头消除，升 tauri 后移除）。
#![allow(clippy::unreachable)]

use tauri::{AppHandle, Runtime};

use crate::commands::boot::current_boot;
use crate::commands::data_location::default_data_dir;
use crate::db::book_registry::{self, BookListInfo};
use crate::db::data_location;
use crate::db::run_db;
use crate::error::Result;

/// 列出全部已登记账本：清单、活动指针、登记变更可用性与回退警示。清单读
/// 注册表最新落盘态（登记变更落盘后立即可见）；注册表损坏时清单为空且不可
/// 变更，回退原因随行（`fallback_reason`）。
#[tauri::command]
pub async fn list_books<R: Runtime>(app: AppHandle<R>) -> Result<BookListInfo> {
    run_db("list_books", move || {
        let default_dir = default_data_dir(&app)?;
        Ok(data_location::gather_book_list(
            &default_dir,
            current_boot(&app).as_ref(),
        ))
    })
    .await
}

/// 新建账本：在应用数据目录下自动创建子目录并登记（只登记不建库，空目录
/// 由既有建连迁移在首次进入时建出全新空库）。
#[tauri::command]
pub async fn create_book<R: Runtime>(
    app: AppHandle<R>,
    name: String,
) -> Result<book_registry::Book> {
    run_db("create_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::create_book_entry(&default_dir, &name)
    })
    .await
}

/// 切换活动账本：写活动指针落盘并返回目标账本；前端随即原位重引导重载进
/// 目标账本（明文库直进、密文库落解锁屏）。
#[tauri::command]
pub async fn switch_book<R: Runtime>(app: AppHandle<R>, id: String) -> Result<book_registry::Book> {
    run_db("switch_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::switch_active_book(&default_dir, &id)
    })
    .await
}

/// 改账本展示名：只动注册表元数据，目录与库文件零变化。
#[tauri::command]
pub async fn rename_book<R: Runtime>(
    app: AppHandle<R>,
    id: String,
    name: String,
) -> Result<book_registry::Book> {
    run_db("rename_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::rename_book_entry(&default_dir, &id, &name)
    })
    .await
}

/// 移除账本：仅摘登记，目录与库文件原样保留（文件生命周期归用户，可重新
/// 登记找回）。活动账本不可移除——先切换到其他账本。
#[tauri::command]
pub async fn remove_book<R: Runtime>(app: AppHandle<R>, id: String) -> Result<()> {
    run_db("remove_book", move || {
        let default_dir = default_data_dir(&app)?;
        data_location::mutable_registry(current_boot(&app).as_ref())?;
        book_registry::remove_book_entry(&default_dir, &id)
    })
    .await
}
