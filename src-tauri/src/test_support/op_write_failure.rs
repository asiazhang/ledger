use ledger_infra::error::AppError;
use rusqlite::Connection;

const TRIGGER_NAME: &str = "op_write_failure_probe";

const FAILURE_MESSAGE: &str = "测试注入：op 写失败";

pub fn block_op_writes(conn: &Connection) {
    conn.execute(
        &format!(
            "CREATE TEMP TRIGGER {TRIGGER_NAME} BEFORE INSERT ON sync.sync_ops \
             BEGIN SELECT RAISE(ABORT, '{FAILURE_MESSAGE}'); END"
        ),
        [],
    )
    .expect("臂装 op 写失败注入（temp 触发器）");
}

pub fn unblock_op_writes(conn: &Connection) {
    conn.execute(&format!("DROP TRIGGER {TRIGGER_NAME}"), [])
        .expect("解除 op 写失败注入");
}

pub fn assert_op_write_failure(err: &AppError) {
    let text = err.to_string();
    assert!(
        text.contains(FAILURE_MESSAGE),
        "错误应来自 op 落库失败注入（{FAILURE_MESSAGE}），实际 {err:?}"
    );
}
