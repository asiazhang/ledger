//! 启动状态命令面集成测试（issue #994 / ADR-0100）：`get_boot_status` 上报
//! 失败门记录的**真实**错误码——前端失败恢复屏按码区分「数据库结构异常」
//! （漂移，从备份恢复优先）与「库不可读」（重置优先）的文案与动作排序。
//!
//! 此前失败码在壳层硬编码为 `boot.db-unreadable`：漂移失败（`init_db` 尾部
//! 守卫，#992）到达前端时被改写，恢复屏无法按码呈现。本文件钉住壳层读路径
//! 「门内记录什么就上报什么」；失败登记的写路径在引导失败单点
//! `recover_boot_failure`（`set_failed(error.code())`），门本体语义归
//! `db::boot` 单测。

// 测试整体豁免（ADR-0060）：集成测试 crate 经 cfg(test) 放行六件套，生产构建零放宽。
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

use tauri::Manager;
use tauri_app_lib::commands::boot::get_boot_status;
use tauri_app_lib::db::boot::BootFailureGate;
use tauri_app_lib::db::encryption::EncryptionGate;
use tauri_app_lib::db::schema_guard::BOOT_SCHEMA_DRIFT;

use crate::sync_channel::{fresh_app, isolate_home};

#[tokio::test]
async fn boot_status_reports_the_recorded_failure_code() {
    isolate_home();
    let (app, _dir) = fresh_app("boot-status");
    app.manage(EncryptionGate::new(false));
    app.manage(BootFailureGate::new());
    let handle = app.handle().clone();

    // 未失败：明文就绪相位，error_code 为空（issue #601 wire 不变）。
    let status = get_boot_status(handle.clone()).expect("状态应可读");
    assert_eq!(status.phase, "ready");
    assert_eq!(status.error_code, None);

    // 漂移失败：上报门内记录的 drift 码，不再被改写成库不可读。
    let gate = handle.state::<BootFailureGate>();
    gate.set_failed(Some(BOOT_SCHEMA_DRIFT));
    let status = get_boot_status(handle.clone()).expect("状态应可读");
    assert_eq!(status.phase, "failed");
    assert_eq!(status.error_code.as_deref(), Some("boot.schema-drift"));

    // 非码化失败：回退既有单一码（issue #601 行为不回退）。
    gate.set_failed(None);
    let status = get_boot_status(handle.clone()).expect("状态应可读");
    assert_eq!(status.phase, "failed");
    assert_eq!(status.error_code.as_deref(), Some("boot.db-unreadable"));
}
