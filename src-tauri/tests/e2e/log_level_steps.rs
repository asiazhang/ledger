use cucumber::{then, when};

use ledger_infra::error::AppError;
use tauri_app_lib::shell_support::logger;

use crate::world::LedgerWorld;

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
