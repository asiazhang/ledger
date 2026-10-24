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

use ledger_infra::db::boot::BootFailureGate;
use ledger_infra::db::connection::open_connection_in;
use ledger_infra::db::data_location::DB_FILE_NAME;
use ledger_infra::db::encryption::EncryptionGate;
use ledger_infra::db::migrate::BOOT_SCHEMA_AHEAD;
use ledger_infra::db::schema_guard::BOOT_SCHEMA_DRIFT;
use tauri::Manager;
use tauri_app_lib::commands::boot::get_boot_status;

use crate::isolation::isolate_home;
use crate::sync_channel::fresh_app;

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

/// 库版本超前的码冒泡（schema-ahead 接缝）：曾由更新版本应用打开过的库
///（user_version 高于本应用迁移链顶）经启动期建连入口 `open_connection_in`
/// 打开时，`init_db` 的前置判定拒绝并以稳定码 `boot.schema-ahead` 上抛——该码
/// 经引导失败登记（`set_failed(error.code())`）进失败门、由 `get_boot_status`
/// 原样上报（上一用例钉住的通道），前端失败恢复屏按码只呈现升级引导，不再
/// 落入「库不可读」的重置/恢复通道（重置会毁掉完好的新库）。库文件与版本位
/// 原样保留。
///
/// 目录按完整形态准备（含 sync.db）：成对挂载在迁移之前，缺 sync.db 的超前库
/// 会先被「拆库世界缺 sync 库」的 `db.sync-mount-failed` 拦下（那是另一个真实
/// 缺陷且重置逃生门仍通）——本用例钉的是完整目录下版本判定先行于迁移。
#[tokio::test]
async fn schema_ahead_db_fails_open_with_coded_error() {
    let dir = std::env::temp_dir().join(format!(
        "ledger-schema-ahead-it-{}",
        ledger_infra::db::new_uuid()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join(DB_FILE_NAME);
    {
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.pragma_update(None, "user_version", 9999).unwrap();
        // 空文件库即满足挂载（ATTACH 对存在文件不再裁决版本）；sync 侧表
        // 结构由迁移链建，超前库不跑迁移，无需真表。
        drop(rusqlite::Connection::open(dir.join("sync.db")).unwrap());
    }
    let err = open_connection_in(&dir).unwrap_err();
    assert_eq!(err.code(), Some(BOOT_SCHEMA_AHEAD));
    // 版本位原样：拒绝不降级、不动库。
    let conn = rusqlite::Connection::open(&db_path).unwrap();
    let kept: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 9999, "超前库 user_version 应原样保留");
}
