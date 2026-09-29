//! op 写失败注入（issue #1896，ADR-0084 决策 1 准入：currencies / investment /
//! transaction / sync-engine 四域同体消费）：「op 落库必失败」的确定性测试侧
//! 注入器具——`BEFORE INSERT ON sync.sync_ops` 的触发器 `RAISE(ABORT)` 挡下
//! op 产出，供跨库原子性测试共用（ADR-0139 决策 2「业务写与 op 同生共死」
//! 的红/绿断言现场：修复前业务行残留即红，修复后两库同滚皆绿）。
//!
//! **temp schema**：普通触发器只能引用触发器所在库的表；temp 触发器可跨库
//! 引用且仅本连接可见、不留 schema 残迹——内存库（attached 侧为 `:memory:`）
//! 与文件库（attached 侧为 sync.db）同一形态可用（先例：`db::tx_scope` 行为
//! 单测的触发器 RAISE(ABORT)，本器具是其跨域重复形态的收编单点）。
//!
//! **注入消息即断言契约**：消息固定 [`FAILURE_MESSAGE`]，[`assert_op_write_failure`]
//! 断言错误确来自本注入（注入消息随代码漂移即红，防止「错误来自别处」的
//! 静默假绿）；注入解除经 [`unblock_op_writes`]（重投递场景的解除注入）。

use ledger_infra::error::AppError;
use rusqlite::Connection;

/// temp 触发器名（器具内部细节；解除注入经 [`unblock_op_writes`]，不依赖本名）。
const TRIGGER_NAME: &str = "op_write_failure_probe";

/// 注入消息（断言契约：消费方经 [`assert_op_write_failure`] 间接断言）。
const FAILURE_MESSAGE: &str = "测试注入：op 写失败";

/// 臂装注入：此后本连接上任何对 `sync.sync_ops` 的 INSERT 一律 `RAISE(ABORT)`。
/// 仅影响本连接（temp 触发器），库内容零残留。
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

/// 解除注入（重投递场景：注入迫使跨库回滚后，解除并重投递验证两库同生）。
pub fn unblock_op_writes(conn: &Connection) {
    conn.execute(&format!("DROP TRIGGER {TRIGGER_NAME}"), [])
        .expect("解除 op 写失败注入");
}

/// 断言错误确来自本注入（失败确实发生在 op 落库这一步，而非编排体其他位置）。
pub fn assert_op_write_failure(err: &AppError) {
    let text = err.to_string();
    assert!(
        text.contains(FAILURE_MESSAGE),
        "错误应来自 op 落库失败注入（{FAILURE_MESSAGE}），实际 {err:?}"
    );
}
