//! 日志等级配置持久化 BDD 步骤（spec #611）。
//!
//! 与既有设置类步骤同款：持 world 的连接锁直接读写后端领域缝
//! （`logger::persisted_level` / `logger::set_persisted_level`），不经 IPC 命令
//! （命令壳只做参数解包，没有 BDD 关注的额外行为）。「旧备份缺表」场景与
//! `settings::get` 的表缺失自愈兑底（ADR-0017）同语义。

use cucumber::{then, when};

use tauri_app_lib::error::AppError;
use tauri_app_lib::logger;

use crate::world::LedgerWorld;

/// 从最近一次操作的码化错误中取稳定错误码（与 encryption_steps 同款 helper）。
fn code_of(err: &AppError) -> Option<&str> {
    match err {
        AppError::Coded { code, .. } => Some(code),
        _ => None,
    }
}

#[when(expr = "写入持久化日志档位 {string}")]
fn write_log_level(world: &mut LedgerWorld, level: String) {
    let conn = world_conn!(world);
    logger::set_persisted_level(&conn, &level).expect("写入合法档位应成功");
}

#[when(expr = "尝试写入非法日志档位 {string}")]
fn try_write_invalid_log_level(world: &mut LedgerWorld, level: String) {
    let conn = world_conn!(world);
    let result = logger::set_persisted_level(&conn, &level);
    world.last_app_error = result.err();
}

#[when(expr = "移除 app_settings 表")]
fn drop_app_settings_table(world: &mut LedgerWorld) {
    let conn = world_conn!(world);
    conn.execute_batch("DROP TABLE IF EXISTS app_settings")
        .expect("移除 app_settings 表失败");
}

#[then(expr = "持久化日志档位应为 {string}")]
fn assert_log_level(world: &mut LedgerWorld, expected: String) {
    let conn = world_conn!(world);
    let actual = logger::persisted_level(&conn).directive();
    assert_eq!(actual, expected, "持久化日志档位不匹配");
}

#[then(expr = "应返回错误码 {string}")]
fn assert_error_code(world: &mut LedgerWorld, code: String) {
    let error = world.last_app_error.as_ref().expect("预期写入失败");
    assert_eq!(
        code_of(error),
        Some(code.as_str()),
        "错误码不匹配，实际: {error}"
    );
}
