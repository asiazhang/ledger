//! 事务作用域原语（ADR-0033 决策 2 / issue #1013、#1014）：数据库连接的事务能力
//! 原语，住址在基础设施（ADR-0056：事务原语是 DB 能力、不是交易域资产，`db/` 在
//! 结构守门白名单）。
//!
//! 双原语、不暴露事务状态查询（issue #1014 / #1003 grilling 定案）：
//! - [`ensure_transaction`]：「保证处于事务中」（嵌套感知）——连接 autocommit 则
//!   自持事务（自持分支基于 [`hold_transaction`]），已在事务中则加入外层；
//!   行为层编排入口、同步引擎等跨域消费点使用；
//! - [`hold_transaction`]：无条件自持事务壳——BEGIN → f → COMMIT / 尽力回滚，
//!   供外层持有者使用（批量导入批次事务、定时引擎期次、余额调整事务壳）；已在
//!   事务中的连接上保持自然 BEGIN 报错，不加码化错误。
//!
//! 跨域消费经 `crate::db::tx_scope::` 显式 import，交易域不再出口事务原语（#1013）。
//!
//! 行为单测：`tests/tx_scope.rs` 锁定两原语的行为——嵌套加入外层 / 自持失败整体
//! 回滚 / COMMIT 失败尽力回滚清理 / ROLLBACK 自身失败不遮蔽原错误 / 无条件自持提交
//! 与已在事务中自然报错（失败注入用纯测试侧手段：SQLite 触发器 RAISE(ABORT) /
//! RAISE(ROLLBACK)、延迟外键），产品代码零 hook。

use rusqlite::Connection;

use crate::error::Result;

/// 「保证处于事务中」（嵌套感知，ADR-0033 决策 2 / issue #1013）：连接 autocommit
/// 则自持 BEGIN/COMMIT/ROLLBACK（`f` 中途失败整体回滚）；已在事务中则加入外层、
/// 失败直接返回错误——回滚归外层持有者（批量导入的批次事务与余额调整的外层事务壳，
/// issue #310，是嵌套模式的合法使用者）。
///
/// 跨 crate 消费面（issue #855 / #1013；#1088 随基础设施 crate 归位由
/// `pub(crate)` 提为 `pub`）：域侧（`accounts` / `policy` / `sync_engine` 等）
/// 复用同一事务原语（命令执行 + op 落日志同事务原子），不另造第二份嵌套感知实现。
///
/// 自持分支基于 [`hold_transaction`]（issue #1014）：事务壳与失败语义只有一处实现。
pub fn ensure_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
    // is_autocommit()=true ⇔ 连接不在事务中（rusqlite 语义），据此选分支。
    if !conn.is_autocommit() {
        return f();
    }
    hold_transaction(conn, f)
}

/// 无条件自持事务壳（issue #1014 / #1003 grilling 定案）：BEGIN → f → COMMIT，
/// 任一步失败尽力回滚。无条件——不检测连接当前事务状态、不暴露事务状态查询（防
/// 协议知识泄漏回调用方）；已在事务中的连接上 BEGIN 自然报错（SQLite
/// "cannot start a transaction within a transaction"），不加码化错误。
///
/// 失败语义与 [`ensure_transaction`] 的自持分支统一（issue #1014）：`f` 中途失败
/// 尽力回滚、不遮蔽原错误（ROLLBACK 自身失败时上抛的仍是业务错误）；COMMIT 失败
/// 尽力回滚清理残留后上抛提交错误。现实触发路径是 SQLite 隐式回滚（如磁盘满）后
/// 显式 ROLLBACK 报「cannot rollback」——统一后调用方看到真因。
///
/// 外层持有者（BEGIN 无条件，非同款形状的嵌套感知拷贝）：批量导入批次事务
/// （`transaction::write::batch`）、定时引擎期次执行（`scheduled_transactions::engine`，
/// ADR-0033 决策 6）、余额调整事务壳（`accounts::core`）。批次层语义（`PRAGMA
/// optimize` / 汇总日志 / 期次日志 / 状态回填）保留在各自调用点外，不进本原语。
pub fn hold_transaction<T>(conn: &Connection, f: impl FnOnce() -> Result<T>) -> Result<T> {
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
