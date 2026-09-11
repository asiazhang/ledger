//! 事务作用域原语 `ensure_transaction`（「保证处于事务中」，嵌套感知；ADR-0033
//! 决策 2 / issue #1013）：数据库连接的事务能力原语，住址在基础设施（ADR-0056：
//! 事务原语是 DB 能力、不是交易域资产，`db/` 在结构守门白名单）。
//!
//! 跨域消费：交易行为层三编排入口、定时引擎（`scheduled_transactions::engine`）、
//! 同步引擎（`sync_engine::apply_ops` / `checkpoint`）、item / categories /
//! accounts / policy / merchants / budget 各域写路径——经 `crate::db::tx_scope::`
//! 显式 import，交易域不再出口事务原语（#1013）。
//!
//! 行为单测：`tests/tx_scope.rs` 锁定四个行为——嵌套加入外层 / 自持失败整体回滚 /
//! COMMIT 失败尽力回滚清理 / ROLLBACK 自身失败不遮蔽原错误（失败注入用纯测试侧
//! 手段：SQLite 触发器 RAISE(ABORT)、延迟外键、第二连接持锁）。

use rusqlite::Connection;

use crate::error::Result;

/// 「保证处于事务中」（嵌套感知，ADR-0033 决策 2 / issue #1013）：连接 autocommit
/// 则自持 BEGIN/COMMIT/ROLLBACK（`f` 中途失败整体回滚）；已在事务中则加入外层、
/// 失败直接返回错误——回滚归外层持有者（批量导入的批次事务与余额调整的外层事务壳，
/// issue #310，是嵌套模式的合法使用者）。
///
/// `pub(crate)`（issue #855 / #1013）：`sync_engine::apply_ops` 重放外来 op 时
/// 复用同一事务原语（命令执行 + op 落日志同事务原子），不另造第二份嵌套感知实现。
pub(crate) fn ensure_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    // is_autocommit()=true ⇔ 连接不在事务中（rusqlite 语义），据此选分支。
    if !conn.is_autocommit() {
        return f();
    }
    conn.execute("BEGIN", [])?;
    match f() {
        Ok(v) => match conn.execute("COMMIT", []) {
            Ok(_) => Ok(v),
            // COMMIT 失败：尽力回滚清理残留（与批量编排同款），再上抛提交错误。
            Err(e) => {
                let _ = conn.execute("ROLLBACK", []);
                Err(e.into())
            }
        },
        // 自持事务中途失败：整体回滚，不留已落库交易行与半套副作用；
        // ROLLBACK 自身失败不遮蔽业务错误（与 COMMIT 失败分支同款，尽力回滚后上抛原错误）。
        Err(e) => {
            let _ = conn.execute("ROLLBACK", []);
            Err(e)
        }
    }
}
